// SPDX-License-Identifier: GPL-3.0-only
// Based on smithay's smallvil example (MIT licensed)

use crate::{Monotile, shell::WindowId};
use smithay::{
    input::pointer::*,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
};

pub struct MoveSurfaceGrab {
    start_data: GrabStartData<Monotile>,
    window_id: WindowId,
    initial_loc: Point<i32, Logical>,
}

impl MoveSurfaceGrab {
    pub fn start(
        start_data: GrabStartData<Monotile>,
        window_id: WindowId,
        initial_rect: Rectangle<i32, Logical>,
    ) -> Self {
        Self {
            start_data,
            window_id,
            initial_loc: initial_rect.loc,
        }
    }
}

impl PointerGrab<Monotile> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        monotile: &mut Monotile,
        handle: &mut PointerInnerHandle<'_, Monotile>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // While the grab is active, no client has pointer focus
        handle.motion(monotile, None, event);

        if let Some(we) = monotile.state.windows.get_mut(self.window_id) {
            let delta = event.location - self.start_data.location;
            we.float_geo.loc = (self.initial_loc.to_f64() + delta).to_i32_round();
            we.configure();
        }
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

    // Gesture forwarding (required by trait)
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

    fn unset(&mut self, mt: &mut Monotile) {
        mt.state.cursor.override_icon = None;
    }
}
