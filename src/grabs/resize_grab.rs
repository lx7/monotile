// SPDX-License-Identifier: GPL-3.0-only
// Based on smithay's smallvil example (MIT licensed)

use crate::{Monotile, shell::WindowId};
use smithay::{
    input::pointer::*,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{compositor, shell::xdg::SurfaceCachedState},
};

pub struct ResizeSurfaceGrab {
    start_data: GrabStartData<Monotile>,
    window_id: WindowId,
    initial_rect: Rectangle<i32, Logical>,
}

impl ResizeSurfaceGrab {
    pub fn start(
        start_data: GrabStartData<Monotile>,
        window_id: WindowId,
        initial_rect: Rectangle<i32, Logical>,
    ) -> Self {
        Self {
            start_data,
            window_id,
            initial_rect,
        }
    }
}

impl PointerGrab<Monotile> for ResizeSurfaceGrab {
    fn motion(
        &mut self,
        monotile: &mut Monotile,
        handle: &mut PointerInnerHandle<'_, Monotile>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(monotile, None, event);

        let Some(we) = monotile.state.mon_mut().get_mut(self.window_id) else {
            return;
        };

        let delta = event.location - self.start_data.location;
        let new_w = self.initial_rect.size.w + delta.x as i32;
        let new_h = self.initial_rect.size.h + delta.y as i32;

        let surface = we.window.toplevel().unwrap();
        let (min, max) = compositor::with_states(surface.wl_surface(), |states| {
            let mut data = states.cached_state.get::<SurfaceCachedState>();
            let cur = data.current();
            (cur.min_size, cur.max_size)
        });

        // 0 means unconstrained in xdg-shell spec
        let clamp = |v: i32, lo: i32, hi: i32| {
            let lo = lo.max(1);
            let hi = if hi == 0 { i32::MAX } else { hi };
            v.clamp(lo, hi)
        };
        we.float_geo.size = Size::from((clamp(new_w, min.w, max.w), clamp(new_h, min.h, max.h)));
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
            state.size = Some(we.float_geo.size);
        });
        surface.send_pending_configure();
    }

    fn relative_motion(
        &mut self,
        monotile: &mut Monotile,
        handle: &mut PointerInnerHandle<'_, Monotile>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(monotile, focus, event);
    }

    fn button(
        &mut self,
        monotile: &mut Monotile,
        handle: &mut PointerInnerHandle<'_, Monotile>,
        event: &ButtonEvent,
    ) {
        handle.button(monotile, event);

        if !handle.current_pressed().contains(&self.start_data.button) {
            handle.unset_grab(self, monotile, event.serial, event.time, true);

            if let Some(we) = monotile.state.mon().get(self.window_id) {
                let xdg = we.window.toplevel().unwrap();
                xdg.with_pending_state(|state| {
                    state.states.unset(xdg_toplevel::State::Resizing);
                    state.size = Some(we.float_geo.size);
                });
                xdg.send_pending_configure();
            }
        }
    }

    fn axis(
        &mut self,
        monotile: &mut Monotile,
        handle: &mut PointerInnerHandle<'_, Monotile>,
        details: AxisFrame,
    ) {
        handle.axis(monotile, details)
    }

    fn frame(&mut self, data: &mut Monotile, handle: &mut PointerInnerHandle<'_, Monotile>) {
        handle.frame(data);
    }

    forward_gesture!(gesture_swipe_begin, GestureSwipeBeginEvent);
    forward_gesture!(gesture_swipe_update, GestureSwipeUpdateEvent);
    forward_gesture!(gesture_swipe_end, GestureSwipeEndEvent);
    forward_gesture!(gesture_pinch_begin, GesturePinchBeginEvent);
    forward_gesture!(gesture_pinch_update, GesturePinchUpdateEvent);
    forward_gesture!(gesture_pinch_end, GesturePinchEndEvent);
    forward_gesture!(gesture_hold_begin, GestureHoldBeginEvent);
    forward_gesture!(gesture_hold_end, GestureHoldEndEvent);

    fn start_data(&self) -> &GrabStartData<Monotile> {
        &self.start_data
    }

    fn unset(&mut self, _: &mut Monotile) {}
}
