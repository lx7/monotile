// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, Instant};

use smithay::{
    reexports::wayland_server::{Client, Resource, protocol::wl_surface::WlSurface},
    utils::{IsAlive, Serial},
    wayland::{
        compositor::{Blocker, BlockerState, add_blocker, with_states},
        shell::xdg::{ToplevelCachedState, XdgToplevelSurfaceData},
    },
};

const ACK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub struct LayoutBlocker {
    pending: Vec<(WlSurface, Serial)>,
    start: Instant,
}

impl LayoutBlocker {
    pub fn install(configured: Vec<(WlSurface, Serial)>) -> Self {
        let blocker = Self {
            pending: configured,
            start: Instant::now(),
        };
        for surface in blocker.surfaces().cloned().collect::<Vec<_>>() {
            add_blocker(&surface, blocker.clone());
        }
        blocker
    }

    pub fn is_committed(&self) -> bool {
        self.start.elapsed() >= ACK_TIMEOUT
            || self
                .pending
                .iter()
                .all(|(s, serial)| !s.alive() || serial_committed(s, *serial))
    }

    fn is_ready(&self) -> bool {
        self.start.elapsed() >= ACK_TIMEOUT
            || self
                .pending
                .iter()
                .all(|(s, serial)| !s.alive() || serial_acked(s, *serial))
    }

    fn surfaces(&self) -> impl Iterator<Item = &WlSurface> {
        self.pending.iter().map(|(s, _)| s)
    }

    pub fn ready_clients(&self) -> impl Iterator<Item = Client> + '_ {
        self.is_ready()
            .then(|| self.surfaces().filter_map(|s| s.client()))
            .into_iter()
            .flatten()
    }
}

impl Blocker for LayoutBlocker {
    fn state(&self) -> BlockerState {
        if self.is_ready() { BlockerState::Released } else { BlockerState::Pending }
    }
}

fn serial_acked(surface: &WlSurface, serial: Serial) -> bool {
    with_states(surface, |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|d| {
                d.lock()
                    .unwrap()
                    .last_acked
                    .as_ref()
                    .is_some_and(|c| c.serial >= serial)
            })
            .unwrap_or(false)
    })
}

fn serial_committed(surface: &WlSurface, serial: Serial) -> bool {
    with_states(surface, |states| {
        states
            .cached_state
            .get::<ToplevelCachedState>()
            .current()
            .last_acked
            .as_ref()
            .is_some_and(|c| c.serial >= serial)
    })
}
