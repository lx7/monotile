// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    delegate_idle_inhibit, delegate_idle_notify,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{
        idle_inhibit::IdleInhibitHandler,
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
    },
};

use crate::Monotile;

impl IdleNotifierHandler for Monotile {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.state.idle_notifier_state
    }
}

delegate_idle_notify!(Monotile);

impl IdleInhibitHandler for Monotile {
    fn inhibit(&mut self, _surface: WlSurface) {
        self.state.idle_notifier_state.set_is_inhibited(true);
    }

    fn uninhibit(&mut self, _surface: WlSurface) {
        self.state.idle_notifier_state.set_is_inhibited(false);
    }
}

delegate_idle_inhibit!(Monotile);
