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
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Scale, Size, Transform},
    wayland::{
        foreign_toplevel_list::ForeignToplevelHandle,
        image_capture_source::{
            ImageCaptureSource, ImageCaptureSourceHandler, OutputCaptureSourceHandler,
            OutputCaptureSourceState, ToplevelCaptureSourceHandler, ToplevelCaptureSourceState,
        },
        image_copy_capture::{
            BufferConstraints, CaptureFailureReason, CursorSession, CursorSessionRef, Frame,
            ImageCopyCaptureHandler, ImageCopyCaptureState, Session, SessionRef,
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
    pub pending_frame: Option<(Frame, CaptureKind)>,
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

pub struct CursorScreencopySession {
    pub session: CursorSession,
    pub damage_tracker: OutputDamageTracker,
    pub output: WeakOutput,
    pub pending_frame: Option<Frame>,
}

pub struct ScreencopyState {
    pub output_capture_source: OutputCaptureSourceState,
    pub toplevel_capture_source: ToplevelCaptureSourceState,
    pub image_copy_capture: ImageCopyCaptureState,
    pub sessions: Vec<ScreencopySession>,
    pub cursor_sessions: Vec<CursorScreencopySession>,
}

fn source_output(source: &ImageCaptureSource) -> Option<Output> {
    source.user_data().get::<WeakOutput>()?.upgrade()
}

fn source_toplevel(source: &ImageCaptureSource) -> Option<WindowId> {
    source.user_data().get::<WindowId>().copied()
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
            cursor_sessions: Vec::new(),
        }
    }

    pub fn cleanup(&mut self) {
        self.image_copy_capture.cleanup();
    }

    pub fn fail_pending_for_output(&mut self, output: &Output) {
        let weak = output.downgrade();
        for s in &mut self.sessions {
            if s.output == weak {
                if let Some((frame, _)) = s.pending_frame.take() {
                    frame.fail(CaptureFailureReason::Unknown);
                }
            }
        }
    }

    pub fn remove_output(&mut self, output: &Output) {
        let weak = output.downgrade();
        self.sessions.retain(|s| s.output != weak);
        self.cursor_sessions.retain(|cs| cs.output != weak);
    }

    pub fn remove_toplevel(&mut self, id: WindowId) {
        self.sessions.retain(|s| !matches_toplevel(&s.session, id));
    }

    pub fn remove_session(&mut self, session: &SessionRef) {
        self.sessions.retain(|s| s.session != *session);
    }

    pub fn output_captured(&self, output: &Output) -> bool {
        let weak = output.downgrade();
        self.sessions.iter().any(|s| s.output == weak)
    }

    pub fn update_cursor(
        &self,
        pos: Option<Point<f64, Logical>>,
        hotspot: Point<i32, Logical>,
        output: &Output,
    ) {
        if self.cursor_sessions.is_empty() {
            return;
        }
        let weak = output.downgrade();
        let scale = output.current_scale().fractional_scale();
        let transform = output.current_transform();
        let output_size = output
            .current_mode()
            .map(|m| m.size.to_f64().to_logical(scale))
            .unwrap_or_default();
        let buf_pos = pos.map(|p| p.to_buffer(scale, transform, &output_size).to_i32_round());
        let buf_hotspot: Point<i32, BufferCoords> = (hotspot.x, hotspot.y).into();
        for cs in &self.cursor_sessions {
            if cs.output == weak {
                cs.session.set_cursor_pos(buf_pos);
                cs.session.set_cursor_hotspot(buf_hotspot);
            }
        }
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
            pending_frame: None,
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

        let kind = if let Some(output) = source_output(&source) {
            let Some(mode) = output.current_mode() else {
                frame.fail(CaptureFailureReason::Unknown);
                return;
            };
            CaptureKind::Output {
                transform: output.current_transform(),
                size: (mode.size.w, mode.size.h).into(),
            }
        } else if let Some(id) = source_toplevel(&source) {
            let Some((_, size, scale)) =
                toplevel_capture_info(&self.state.windows, &self.state.monitors, id)
            else {
                frame.fail(CaptureFailureReason::Unknown);
                return;
            };
            CaptureKind::Toplevel { id, size, scale }
        } else {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        };

        let Some(s) = self
            .state
            .screencopy
            .sessions
            .iter_mut()
            .find(|s| s.session == *session)
        else {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        };
        s.pending_frame = Some((frame, kind));
        if let Some(output) = s.output.upgrade() {
            self.backend.schedule_render(&output);
        }
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

    fn cursor_capture_constraints(
        &mut self,
        source: &ImageCaptureSource,
    ) -> Option<BufferConstraints> {
        if self.state.locked {
            return None;
        }
        source_output(source)?;
        let size = self.state.cursor.size as i32;
        Some(BufferConstraints {
            size: (size, size).into(),
            shm: SHM_FORMATS.to_vec(),
            dma: self.backend.dma_constraints(),
        })
    }

    fn new_cursor_session(&mut self, session: CursorSession) {
        let Some(output) = source_output(&session.source()) else {
            return;
        };
        let size = self.state.cursor.size as i32;
        let phys_size: Size<i32, Physical> = (size, size).into();
        self.state
            .screencopy
            .cursor_sessions
            .push(CursorScreencopySession {
                session,
                damage_tracker: OutputDamageTracker::new(phys_size, 1.0, Transform::Normal),
                pending_frame: None,
                output: output.downgrade(),
            });
    }

    fn cursor_frame(&mut self, session: &CursorSessionRef, frame: Frame) {
        if self.state.locked {
            frame.fail(CaptureFailureReason::Unknown);
            return;
        }
        if let Some(cs) = self
            .state
            .screencopy
            .cursor_sessions
            .iter_mut()
            .find(|cs| *cs.session == *session)
        {
            cs.pending_frame = Some(frame);
            if let Some(output) = cs.output.upgrade() {
                self.backend.schedule_render(&output);
            }
        }
    }

    fn cursor_session_destroyed(&mut self, session: CursorSessionRef) {
        self.state
            .screencopy
            .cursor_sessions
            .retain(|cs| *cs.session != session);
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
    let weak = output.downgrade();
    for s in &mut state.screencopy.sessions {
        if s.output != weak {
            continue;
        }
        let Some((frame, kind)) = s.pending_frame.take() else {
            continue;
        };
        if state.locked {
            frame.fail(CaptureFailureReason::Unknown);
            continue;
        }
        let buf = frame.buffer();
        match kind {
            CaptureKind::Output { transform, size } => {
                let elems = if s.session.draw_cursor() {
                    output_elems
                } else {
                    &output_elems[cursor_count..]
                };
                match crate::render::render_to_buffer(
                    renderer,
                    &mut s.damage_tracker,
                    &buf,
                    elems,
                    background,
                    transform,
                    size,
                ) {
                    Ok(damage) => frame.success(transform, damage, elapsed),
                    Err(err) => {
                        warn!(?err, "screencopy: output capture failed");
                        frame.fail(CaptureFailureReason::Unknown);
                    }
                }
            }
            CaptureKind::Toplevel { id, size, scale } => {
                let Some(we) = state.windows.get(id) else {
                    frame.fail(CaptureFailureReason::Unknown);
                    continue;
                };
                let Some(wl) = we.window.wl_surface() else {
                    frame.fail(CaptureFailureReason::Unknown);
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
                if s.session.draw_cursor() {
                    let ptr_pos = state.seat.get_pointer().unwrap().current_location();
                    let local_pos = ptr_pos - window_loc.to_f64();
                    elems.splice(0..0, state.cursor.elements(renderer, local_pos));
                }
                match crate::render::render_to_buffer(
                    renderer,
                    &mut s.damage_tracker,
                    &buf,
                    &elems,
                    Color32F::TRANSPARENT,
                    Transform::Normal,
                    size,
                ) {
                    Ok(damage) => frame.success(Transform::Normal, damage, elapsed),
                    Err(err) => {
                        warn!(?err, "screencopy: toplevel capture failed");
                        frame.fail(CaptureFailureReason::Unknown);
                    }
                }
            }
        }
    }
}

pub fn capture_cursor(
    renderer: &mut GlowRenderer,
    state: &mut State,
    output: &Output,
    elapsed: std::time::Duration,
) {
    let weak = output.downgrade();
    for cs in &mut state.screencopy.cursor_sessions {
        if cs.output != weak {
            continue;
        }
        let Some(frame) = cs.pending_frame.take() else {
            continue;
        };
        if state.locked {
            frame.fail(CaptureFailureReason::Unknown);
            continue;
        }
        let hotspot = state.cursor.hotspot.to_f64();
        let elems = state.cursor.elements(renderer, hotspot);
        let size = state.cursor.size as i32;
        match crate::render::render_to_buffer(
            renderer,
            &mut cs.damage_tracker,
            &frame.buffer(),
            &elems,
            Color32F::TRANSPARENT,
            Transform::Normal,
            (size, size).into(),
        ) {
            Ok(damage) => frame.success(Transform::Normal, damage, elapsed),
            Err(err) => {
                warn!(?err, "screencopy: cursor capture failed");
                frame.fail(CaptureFailureReason::Unknown);
            }
        }
    }
}
