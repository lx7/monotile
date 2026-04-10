// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::renderer::{
        Color32F,
        damage::OutputDamageTracker,
        element::{
            Kind,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        glow::GlowRenderer,
    },
    delegate_image_capture_source, delegate_image_copy_capture, delegate_output_capture_source,
    delegate_toplevel_capture_source,
    output::{Output, WeakOutput},
    reexports::wayland_server::{DisplayHandle, protocol::wl_shm},
    utils::{Buffer as BufferCoords, Physical, Point, Scale, Size, Transform},
    wayland::{
        foreign_toplevel_list::ForeignToplevelHandle,
        image_capture_source::{
            ImageCaptureSource, ImageCaptureSourceHandler, OutputCaptureSourceHandler,
            OutputCaptureSourceState, ToplevelCaptureSourceHandler, ToplevelCaptureSourceState,
        },
        image_copy_capture::{
            BufferConstraints, CaptureFailureReason, Frame, ImageCopyCaptureHandler,
            ImageCopyCaptureState, Session, SessionRef,
        },
        seat::WaylandFocus,
    },
};

use tracing::warn;

use crate::{
    Monotile,
    render::MonotileElement,
    shell::{Monitors, WindowId, Windows},
    state::State,
};

pub struct ScreencopySession {
    pub session: Session,
    pub damage_tracker: OutputDamageTracker,
    pub output: WeakOutput,
}

pub struct PendingCapture {
    pub session: SessionRef,
    pub frame: Frame,
    pub output: WeakOutput,
    pub kind: CaptureKind,
}

pub enum CaptureKind {
    Output {
        transform: Transform,
        size: Size<i32, BufferCoords>,
    },
    Toplevel {
        id: WindowId,
        size: Size<i32, BufferCoords>,
        scale: Scale<f64>,
    },
}

pub struct ScreencopyState {
    pub output_capture_source: OutputCaptureSourceState,
    pub toplevel_capture_source: ToplevelCaptureSourceState,
    pub image_copy_capture: ImageCopyCaptureState,
    pub sessions: Vec<ScreencopySession>,
    pub pending: Vec<PendingCapture>,
}

fn source_output(source: &ImageCaptureSource) -> Option<Output> {
    source.user_data().get::<WeakOutput>()?.upgrade()
}

fn source_toplevel(source: &ImageCaptureSource) -> Option<WindowId> {
    source.user_data().get::<WindowId>().copied()
}

fn matches_output(sref: &SessionRef, output: &WeakOutput) -> bool {
    sref.source()
        .user_data()
        .get::<WeakOutput>()
        .is_some_and(|w| w == output)
}

fn matches_toplevel(sref: &SessionRef, id: WindowId) -> bool {
    sref.source()
        .user_data()
        .get::<WindowId>()
        .is_some_and(|&wid| wid == id)
}

const SHM_FORMATS: [wl_shm::Format; 2] = [wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888];

// TODO: reconsider when multi-monitor and the output hashmap are implemented
fn toplevel_capture_info(
    windows: &Windows,
    monitors: &Monitors,
    id: WindowId,
) -> Option<(Output, Size<i32, BufferCoords>, Scale<f64>)> {
    let we = windows.get(id)?;
    let mon = &monitors[we.monitor];
    let scale = mon.output.current_scale().fractional_scale();
    let geo = we.render_geo.to_f64().to_physical(scale);
    Some((
        mon.output.clone(),
        (geo.size.w as i32, geo.size.h as i32).into(),
        Scale::from(scale),
    ))
}

impl ScreencopyState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            output_capture_source: OutputCaptureSourceState::new::<Monotile>(dh),
            toplevel_capture_source: ToplevelCaptureSourceState::new::<Monotile>(dh),
            image_copy_capture: ImageCopyCaptureState::new::<Monotile>(dh),
            sessions: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn cleanup(&mut self) {
        self.image_copy_capture.cleanup();
    }

    fn take_pending_where(
        &mut self,
        pred: impl Fn(&PendingCapture) -> bool,
    ) -> Vec<PendingCapture> {
        let mut taken = Vec::new();
        let mut i = 0;
        while i < self.pending.len() {
            if pred(&self.pending[i]) {
                taken.push(self.pending.swap_remove(i));
            } else {
                i += 1;
            }
        }
        taken
    }

    pub fn take_pending_for_output(&mut self, output: &Output) -> Vec<PendingCapture> {
        let weak = output.downgrade();
        self.take_pending_where(|p| p.output == weak)
    }

    pub fn fail_pending_for_output(&mut self, output: &Output) {
        for p in self.take_pending_for_output(output) {
            p.frame.fail(CaptureFailureReason::Unknown);
        }
    }

    pub fn damage_tracker_mut(&mut self, sref: &SessionRef) -> &mut OutputDamageTracker {
        &mut self
            .sessions
            .iter_mut()
            .find(|s| s.session == *sref)
            .expect("session must exist for pending capture")
            .damage_tracker
    }

    pub fn remove_output(&mut self, output: &Output) {
        let weak = output.downgrade();
        for p in self.take_pending_where(|p| matches_output(&p.session, &weak)) {
            p.frame.fail(CaptureFailureReason::Unknown);
        }
        self.sessions.retain(|s| !matches_output(&s.session, &weak));
    }

    pub fn remove_toplevel(&mut self, id: WindowId) {
        for p in self.take_pending_where(|p| matches_toplevel(&p.session, id)) {
            p.frame.fail(CaptureFailureReason::Stopped);
        }
        self.sessions.retain(|s| !matches_toplevel(&s.session, id));
    }

    pub fn remove_session(&mut self, session: &SessionRef) {
        self.sessions.retain(|s| s.session != *session);
        for p in self.take_pending_where(|p| p.session == *session) {
            p.frame.fail(CaptureFailureReason::Stopped);
        }
    }

    pub fn output_captured(&self, output: &Output) -> bool {
        let weak = output.downgrade();
        self.sessions.iter().any(|s| s.output == weak)
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

impl ToplevelCaptureSourceHandler for Monotile {
    fn toplevel_capture_source_state(&mut self) -> &mut ToplevelCaptureSourceState {
        &mut self.state.screencopy.toplevel_capture_source
    }

    fn toplevel_source_created(
        &mut self,
        source: ImageCaptureSource,
        toplevel: &ForeignToplevelHandle,
    ) {
        if let Some(&id) = toplevel.user_data().get::<WindowId>() {
            source.user_data().insert_if_missing(|| id);
        }
    }
}
delegate_toplevel_capture_source!(Monotile);

impl ImageCopyCaptureHandler for Monotile {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.state.screencopy.image_copy_capture
    }

    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        if self.state.locked {
            return None;
        }

        let dma = self.backend.dma_constraints();

        let size = if let Some(output) = source_output(source) {
            let mode = output.current_mode()?;
            (mode.size.w, mode.size.h).into()
        } else if let Some(id) = source_toplevel(source) {
            toplevel_capture_info(&self.state.windows, &self.state.monitors, id)?.1
        } else {
            return None;
        };
        Some(BufferConstraints {
            size,
            shm: SHM_FORMATS.to_vec(),
            dma,
        })
    }

    fn new_session(&mut self, session: Session) {
        let source = session.source();
        let target = source_toplevel(&source);
        let (output, tracker) = if let Some(output) = source_output(&source) {
            (output.clone(), OutputDamageTracker::from_output(&output))
        } else if let Some(id) = target {
            let Some((output, buf_size, scale)) =
                toplevel_capture_info(&self.state.windows, &self.state.monitors, id)
            else {
                return;
            };
            let size: Size<i32, Physical> = (buf_size.w, buf_size.h).into();
            (
                output,
                OutputDamageTracker::new(size, scale, Transform::Normal),
            )
        } else {
            return;
        };
        self.state.screencopy.sessions.push(ScreencopySession {
            session,
            damage_tracker: tracker,
            output: output.downgrade(),
        });
        if let Some(id) = target
            && let Some(we) = self.state.windows.get_mut(id)
        {
            we.mark_screencast();
        }
        self.state.ipc.dirty = true;
        self.backend.schedule_render_all();
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        if self.state.locked {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        }
        let source = session.source();

        let (output, kind) = if let Some(output) = source_output(&source) {
            let Some(mode) = output.current_mode() else {
                frame.fail(CaptureFailureReason::Unknown);
                return;
            };
            let size = (mode.size.w, mode.size.h).into();
            let transform = output.current_transform();
            (output, CaptureKind::Output { transform, size })
        } else if let Some(id) = source_toplevel(&source) {
            let Some((output, size, scale)) =
                // TODO: get output from WindowElement when multi-monitor is implemented
                toplevel_capture_info(&self.state.windows, &self.state.monitors, id)
            else {
                frame.fail(CaptureFailureReason::Unknown);
                return;
            };
            (output, CaptureKind::Toplevel { id, size, scale })
        } else {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        };

        self.state.screencopy.pending.push(PendingCapture {
            session: session.clone(),
            frame,
            output: output.downgrade(),
            kind,
        });
        self.backend.schedule_render(&output);
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        let target = source_toplevel(&session.source());
        self.state.screencopy.remove_session(&session);
        if let Some(id) = target
            && let Some(we) = self.state.windows.get_mut(id)
        {
            we.unmark_screencast();
        }
        self.state.ipc.dirty = true;
        self.backend.schedule_render_all();
    }
}
delegate_image_copy_capture!(Monotile);

pub fn capture_frame(
    renderer: &mut GlowRenderer,
    state: &mut State,
    output: &Output,
    output_elems: &[MonotileElement],
    cursor_count: usize,
    background: impl Into<Color32F> + Copy,
    elapsed: std::time::Duration,
) {
    for p in state.screencopy.take_pending_for_output(output) {
        if state.locked {
            p.frame.fail(CaptureFailureReason::Unknown);
            continue;
        }

        let tracker = state.screencopy.damage_tracker_mut(&p.session);
        let buf = p.frame.buffer();

        match p.kind {
            CaptureKind::Output { transform, size } => {
                let elems = if p.session.draw_cursor() {
                    output_elems
                } else {
                    &output_elems[cursor_count..]
                };
                match crate::render::render_to_buffer(
                    renderer, tracker, &buf, elems, background, transform, size,
                ) {
                    Ok(damage) => p.frame.success(transform, damage, elapsed),
                    Err(err) => {
                        warn!(?err, "screencopy: output capture failed");
                        p.frame.fail(CaptureFailureReason::Unknown);
                    }
                }
            }
            CaptureKind::Toplevel { id, size, scale } => {
                let Some(we) = state.windows.get(id) else {
                    p.frame.fail(CaptureFailureReason::Unknown);
                    continue;
                };
                let Some(wl) = we.window.wl_surface() else {
                    p.frame.fail(CaptureFailureReason::Unknown);
                    continue;
                };
                let window_loc = we.render_geo.loc;
                let loc = Point::from((0, 0)) - we.window.geometry().loc;
                let surf_loc = loc.to_physical_precise_round(scale);
                let surfs: Vec<WaylandSurfaceRenderElement<GlowRenderer>> =
                    render_elements_from_surface_tree(
                        renderer,
                        &wl,
                        surf_loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    );
                let mut elems: Vec<MonotileElement> =
                    crate::render::popup_elements(renderer, &wl, loc, scale);
                elems.extend(surfs.into_iter().map(MonotileElement::Surface));
                if p.session.draw_cursor() {
                    let ptr_pos = state.seat.get_pointer().unwrap().current_location();
                    let local_pos = ptr_pos - window_loc.to_f64();
                    elems.splice(0..0, state.cursor.elements(renderer, local_pos));
                }
                match crate::render::render_to_buffer(
                    renderer,
                    tracker,
                    &buf,
                    &elems,
                    Color32F::TRANSPARENT,
                    Transform::Normal,
                    size,
                ) {
                    Ok(damage) => p.frame.success(Transform::Normal, damage, elapsed),
                    Err(err) => {
                        warn!(?err, "screencopy: toplevel capture failed");
                        p.frame.fail(CaptureFailureReason::Unknown);
                    }
                }
            }
        }
    }
}
