// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;

use smithay::{
    desktop::PopupGrab, input::Seat, output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
};

use super::OutputExt;
use crate::Monotile;

struct ActiveOutput(RefCell<Output>);

#[derive(Default)]
struct ActivePopupGrab(RefCell<Option<PopupGrab<Monotile>>>);

pub trait SeatExt {
    fn active_output(&self) -> Output;
    fn set_active_output(&self, output: &Output);
    fn set_popup_grab(&self, grab: PopupGrab<Monotile>);
    fn take_popup_grab(&self) -> Option<PopupGrab<Monotile>>;

    // TODO multi-seat: resolve via the pointer's position instead
    fn pointer_output(&self) -> Output {
        self.active_output()
    }

    fn exclusive_layer(&self) -> Option<WlSurface> {
        self.active_output().exclusive_layer()
    }
}

impl SeatExt for Seat<Monotile> {
    fn active_output(&self) -> Output {
        self.user_data()
            .get::<ActiveOutput>()
            .expect("set when the first monitor was added")
            .0
            .borrow()
            .clone()
    }

    fn set_active_output(&self, output: &Output) {
        let data = self.user_data();
        data.insert_if_missing(|| ActiveOutput(RefCell::new(output.clone())));
        *data.get::<ActiveOutput>().unwrap().0.borrow_mut() = output.clone();
    }

    fn set_popup_grab(&self, grab: PopupGrab<Monotile>) {
        let data = self.user_data();
        data.insert_if_missing(ActivePopupGrab::default);
        data.get::<ActivePopupGrab>().unwrap().0.replace(Some(grab));
    }

    fn take_popup_grab(&self) -> Option<PopupGrab<Monotile>> {
        self.user_data().get::<ActivePopupGrab>()?.0.take()
    }
}
