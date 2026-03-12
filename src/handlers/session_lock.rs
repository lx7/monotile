// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    delegate_session_lock,
    output::Output,
    reexports::wayland_server::protocol::wl_output::WlOutput,
    wayland::session_lock::{
        LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
    },
};
use tracing::info;

use crate::Monotile;

impl SessionLockHandler for Monotile {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.state.session_lock_state
    }

    fn lock(&mut self, locker: SessionLocker) {
        if self.state.locked {
            return;
        }

        self.state.locked = true;
        self.set_focus(None);
        locker.lock();
        info!("session locked");

        let outputs: Vec<_> = self
            .state
            .monitors
            .iter()
            .map(|m| m.output.clone())
            .collect();
        for output in &outputs {
            self.backend.schedule_render(output);
        }
    }

    fn unlock(&mut self) {
        self.state.locked = false;
        for mon in self.state.monitors.iter_mut() {
            mon.lock_surface = None;
        }
        self.update_focus();
        info!("session unlocked");

        let outputs: Vec<_> = self
            .state
            .monitors
            .iter()
            .map(|m| m.output.clone())
            .collect();
        for output in &outputs {
            self.backend.schedule_render(output);
        }
    }

    fn new_surface(&mut self, surface: LockSurface, wl_output: WlOutput) {
        let output = Output::from_resource(&wl_output);
        let mon = output
            .as_ref()
            .and_then(|o| self.state.monitors.iter_mut().find(|m| m.output == *o));
        let Some(mon) = mon else { return };

        let size = mon.output.current_mode().unwrap().size.to_logical(1);
        surface.with_pending_state(|s| {
            s.size = Some((size.w as u32, size.h as u32).into());
        });
        surface.send_configure();
        mon.lock_surface = Some(surface);
        let output = mon.output.clone();

        self.update_focus();
        self.backend.schedule_render(&output);
    }
}

delegate_session_lock!(Monotile);
