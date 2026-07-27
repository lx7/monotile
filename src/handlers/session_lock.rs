// SPDX-License-Identifier: GPL-3.0-only

use crate::{Monotile, shell::OutputExt, state::State};
use smithay::{
    delegate_session_lock,
    output::Output,
    reexports::wayland_server::protocol::wl_output::WlOutput,
    wayland::session_lock::{
        LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
    },
};
use std::collections::HashSet;
use tracing::info;

impl SessionLockHandler for Monotile {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.state.session_lock_state
    }

    fn lock(&mut self, locker: SessionLocker) {
        if self.state.locked || self.state.pending_lock.is_some() {
            return;
        }

        self.state.locked = true;
        self.set_focus(None);
        // Output is hashed by identity, the clippy warning is not relevant here.
        #[allow(clippy::mutable_key_type)]
        let outputs: HashSet<_> = self.state.monitors.keys().cloned().collect();
        if outputs.is_empty() {
            locker.lock();
            info!("session locked (no outputs)");
        } else {
            info!("session locking ({} outputs pending)", outputs.len());
            self.state.pending_lock = Some((locker, outputs));
            self.backend.schedule_render_all();
        }
    }

    fn unlock(&mut self) {
        self.state.locked = false;
        for mon in self.state.monitors.values_mut() {
            mon.lock_surface = None;
        }
        self.update_focus();
        info!("session unlocked");
        self.backend.schedule_render_all();
    }

    fn new_surface(&mut self, surface: LockSurface, wl_output: WlOutput) {
        let Some(output) = Output::from_resource(&wl_output) else {
            return;
        };
        let Some(mon) = self.state.monitors.get_mut(&output) else {
            return;
        };

        let size = output.geometry().size;
        surface.with_pending_state(|s| {
            s.size = Some((size.w as u32, size.h as u32).into());
        });
        surface.send_configure();
        mon.lock_surface = Some(surface);

        self.update_focus();
        self.backend.schedule_render(&output);
    }
}

delegate_session_lock!(Monotile);

impl State {
    pub fn confirm_lock(&mut self, output: &Output) {
        if let Some((_, remaining)) = &mut self.pending_lock {
            remaining.remove(output);
            if remaining.is_empty() {
                let (locker, _) = self.pending_lock.take().unwrap();
                locker.lock();
                info!("session locked");
            }
        }
    }
}
