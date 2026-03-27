// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, BufferType, Color32F, ExportMem, Offscreen, buffer_type,
            damage::OutputDamageTracker, gles::GlesTexture, glow::GlowRenderer,
        },
    },
    delegate_image_capture_source, delegate_image_copy_capture, delegate_output_capture_source,
    output::{Output, WeakOutput},
    reexports::wayland_server::{DisplayHandle, protocol::wl_shm},
    utils::{Buffer as BufferCoords, Physical, Rectangle, Size, Transform},
    wayland::{
        dmabuf::get_dmabuf,
        image_capture_source::{
            ImageCaptureSource, ImageCaptureSourceHandler, OutputCaptureSourceHandler,
            OutputCaptureSourceState,
        },
        image_copy_capture::{
            BufferConstraints, CaptureFailureReason, Frame, ImageCopyCaptureHandler,
            ImageCopyCaptureState, Session, SessionRef,
        },
        shm::with_buffer_contents_mut,
    },
};

use anyhow::anyhow;
use tracing::warn;

use crate::{Monotile, backend::Backend, render::MonotileElement};

pub struct ScreencopySession {
    pub session: Session,
    pub damage_tracker: OutputDamageTracker,
}

pub struct ScreencopyState {
    pub output_capture_source: OutputCaptureSourceState,
    pub image_copy_capture: ImageCopyCaptureState,
    pub sessions: Vec<ScreencopySession>,
    pub pending: Vec<(SessionRef, Frame)>,
}

fn source_output(source: &ImageCaptureSource) -> Option<Output> {
    source.user_data().get::<WeakOutput>()?.upgrade()
}

fn matches_output(sref: &SessionRef, output: &WeakOutput) -> bool {
    sref.source()
        .user_data()
        .get::<WeakOutput>()
        .map_or(false, |w| w == output)
}

impl ScreencopyState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            output_capture_source: OutputCaptureSourceState::new::<Monotile>(dh),
            image_copy_capture: ImageCopyCaptureState::new::<Monotile>(dh),
            sessions: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn cleanup(&mut self) {
        self.image_copy_capture.cleanup();
    }

    pub fn take_pending(&mut self, output: &WeakOutput) -> Vec<(SessionRef, Frame)> {
        let mut keep = Vec::new();
        let mut taken = Vec::new();
        for (sref, frame) in self.pending.drain(..) {
            if matches_output(&sref, output) {
                taken.push((sref, frame));
            } else {
                keep.push((sref, frame));
            }
        }
        self.pending = keep;
        taken
    }

    pub fn remove_output(&mut self, output: &WeakOutput) {
        for (_, frame) in self.take_pending(output) {
            frame.fail(CaptureFailureReason::Unknown);
        }
        self.sessions
            .retain(|s| !matches_output(&s.session, output));
    }
}

impl ImageCaptureSourceHandler for Monotile {}
delegate_image_capture_source!(Monotile);

impl OutputCaptureSourceHandler for Monotile {
    fn output_capture_source_state(&mut self) -> &mut OutputCaptureSourceState {
        &mut self.state.screencopy.output_capture_source
    }

    fn output_source_created(&mut self, source: ImageCaptureSource, output: &Output) {
        source.user_data().insert_if_missing(|| output.downgrade());
    }
}
delegate_output_capture_source!(Monotile);

impl ImageCopyCaptureHandler for Monotile {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.state.screencopy.image_copy_capture
    }

    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        if self.state.locked || matches!(self.backend, Backend::Winit(_)) {
            return None;
        }

        let output = source_output(source)?;
        let mode = output.current_mode()?;
        let size: Size<i32, BufferCoords> = (mode.size.w, mode.size.h).into();
        let dma = match &self.backend {
            Backend::Drm(drm) => drm.dma_constraints.clone(),
            _ => None,
        };

        Some(BufferConstraints {
            size,
            shm: vec![wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
            dma,
        })
    }

    fn new_session(&mut self, session: Session) {
        let Some(output) = source_output(&session.source()) else {
            return;
        };
        let tracker = OutputDamageTracker::from_output(&output);
        self.state.screencopy.sessions.push(ScreencopySession {
            session,
            damage_tracker: tracker,
        });
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        let Some(output) = source_output(&session.source()) else {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        };

        match &self.backend {
            Backend::Drm(_) => {
                self.state.screencopy.pending.push((session.clone(), frame));
                self.backend.schedule_render(&output);
            }
            _ => {
                frame.fail(CaptureFailureReason::Unknown);
            }
        }
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        self.state
            .screencopy
            .sessions
            .retain(|s| s.session != session);
        self.state.screencopy.pending.retain(|(s, _)| *s != session);
    }
}
delegate_image_copy_capture!(Monotile);

type Damage = Vec<Rectangle<i32, BufferCoords>>;

fn damage_to_buffer(
    damage: Option<&Vec<Rectangle<i32, Physical>>>,
    transform: Transform,
    size: Size<i32, BufferCoords>,
) -> Option<Damage> {
    Some(
        damage?
            .iter()
            .map(|r| {
                r.to_logical(1)
                    .to_buffer(1, transform.invert(), &size.to_logical(1, transform))
            })
            .collect(),
    )
}

pub fn capture_output(
    renderer: &mut GlowRenderer,
    screencopy: &mut ScreencopyState,
    output: &Output,
    elems: &[MonotileElement],
    background: impl Into<Color32F> + Copy,
    elapsed: std::time::Duration,
) {
    let transform = output.current_transform();
    let mode_size: Size<i32, BufferCoords> = output
        .current_mode()
        .map(|m| (m.size.w, m.size.h).into())
        .unwrap_or_default();

    for (sref, frame) in screencopy.take_pending(&output.downgrade()) {
        match (|| {
            let session = screencopy
                .sessions
                .iter_mut()
                .find(|s| s.session == sref)
                .ok_or(anyhow!("no session"))?;
            let buf = frame.buffer();
            let tracker = &mut session.damage_tracker;

            match buffer_type(&buf) {
                Some(BufferType::Shm) => {
                    let mut tex: GlesTexture =
                        renderer.create_buffer(Fourcc::Argb8888, mode_size)?;
                    let mut fb = renderer.bind(&mut tex)?;
                    let r = tracker.render_output(renderer, &mut fb, 0, elems, background)?;
                    let damage = damage_to_buffer(r.damage, transform, mode_size);
                    let mapping = renderer.copy_framebuffer(
                        &fb,
                        Rectangle::from_size(mode_size),
                        Fourcc::Argb8888,
                    )?;
                    let pixels = renderer.map_texture(&mapping)?;
                    with_buffer_contents_mut(&buf, |ptr, len, data| {
                        let dst = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
                        let bpp = 4; // ARGB8888 / XRGB8888

                        // due to padding, strides might differ
                        let src_stride = data.width as usize * bpp;
                        let dst_stride = data.stride as usize;

                        for y in 0..data.height as usize {
                            let src_off = y * src_stride;
                            let dst_off = y * dst_stride;
                            let row = src_stride.min(dst_stride);
                            dst[dst_off..dst_off + row]
                                .copy_from_slice(&pixels[src_off..src_off + row]);
                        }
                    })?;
                    Ok(damage)
                }
                Some(BufferType::Dma) => {
                    let mut dmabuf = get_dmabuf(&buf)?.clone();
                    let mut fb = renderer.bind(&mut dmabuf)?;
                    let result = tracker.render_output(renderer, &mut fb, 0, elems, background)?;
                    Ok(damage_to_buffer(result.damage, transform, mode_size))
                }
                _ => Err(anyhow!("unsupported buffer type")),
            }
        })() {
            Ok(damage) => frame.success(transform, damage, elapsed),
            Err(err) => {
                warn!(?err, "screencopy: capture failed");
                frame.fail(CaptureFailureReason::Unknown);
            }
        }
    }
}
