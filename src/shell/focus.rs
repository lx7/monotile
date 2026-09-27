// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::input::InputTime,
    input::pointer::MotionEvent,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use super::{MonitorsExt, SeatExt, WindowId};
use crate::Monotile;

impl Monotile {
    pub fn update_keyboard_focus(&mut self) {
        self.set_keyboard_focus(self.state.seat_mon().tag().focused_id());
    }

    pub fn set_keyboard_focus(&mut self, id: Option<WindowId>) {
        let previous = self.state.windows.focused;
        if let Some(old) = self.state.windows.focused
            && Some(old) != id
        {
            if let Some(we) = self.state.windows.get_mut(old) {
                we.set_focused(false);
            }
            self.state.windows.focused = None;
        }

        // if locked, focus the lock surface
        if self.state.locked() {
            let surface = self
                .state
                .seat_mon()
                .lock_surface
                .as_ref()
                .map(|ls| ls.wl_surface().clone());
            if let Some(kb) = self.state.seat.get_keyboard() {
                kb.set_focus(self, surface, SERIAL_COUNTER.next_serial());
            }
            return;
        }

        // if exclusive layer exists, focus it
        if let Some(surface) = self.state.seat.exclusive_layer() {
            if let Some(kb) = self.state.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
            return;
        }

        // if none of the above, focus window
        if let Some(id) = id {
            self.state.seat_mon_mut().tag_mut().promote(id);
            if let Some(we) = self.state.windows.get_mut(id) {
                we.set_focused(true);
            }
            self.state.windows.focused = id.into();
        }

        let target = self.state.windows.focused_surface();
        if let Some(kb) = self.state.seat.get_keyboard() {
            kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }

        // warp the cursor
        if self.state.config.seats["seat0"].cursor_warp
            && previous != id
            && let Some(id) = id
        {
            self.warp_cursor(id);
        }
        self.state.ipc.dirty = true;
    }

    pub(crate) fn warp_cursor(&mut self, id: WindowId) {
        let Some(rect) = self.state.monitors.window_rect(&self.state.windows, id) else {
            return;
        };
        let pointer = self.state.seat.get_pointer().unwrap();
        if rect.to_f64().contains(pointer.current_location()) {
            return;
        }

        let center: Point<f64, Logical> = (
            rect.loc.x as f64 + rect.size.w as f64 / 2.0,
            rect.loc.y as f64 + rect.size.h as f64 / 2.0,
        )
            .into();
        self.send_pointer_motion(center);
    }

    pub(crate) fn update_pointer_focus(&mut self) {
        let location = self.state.seat.get_pointer().unwrap().current_location();
        self.send_pointer_motion(location);
    }

    fn send_pointer_motion(&mut self, location: Point<f64, Logical>) {
        let pointer = self.state.seat.get_pointer().unwrap();
        let under = self.state.surface_under(location);
        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: InputTime::now(),
            },
        );
        pointer.frame(self);
    }
}
