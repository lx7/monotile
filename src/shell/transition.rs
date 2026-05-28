// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, Instant};

use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Serial},
    wayland::{
        compositor::{Blocker, BlockerState, add_blocker, with_states},
        shell::xdg::XdgToplevelSurfaceData,
    },
};

use super::{Tag, WindowElement};

const ACK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub struct LayoutBlocker {
    pending: Vec<(WlSurface, Serial)>,
    start: Instant,
}

impl LayoutBlocker {
    pub fn new(pending: Vec<(WlSurface, Serial)>) -> Self {
        Self {
            pending,
            start: Instant::now(),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.start.elapsed() >= ACK_TIMEOUT
            || self
                .pending
                .iter()
                .all(|(s, serial)| !s.alive() || serial_acked(s, *serial))
    }

    pub fn surfaces(&self) -> impl Iterator<Item = &WlSurface> {
        self.pending.iter().map(|(s, _)| s)
    }
}

impl Blocker for LayoutBlocker {
    fn state(&self) -> BlockerState {
        if self.is_ready() { BlockerState::Released } else { BlockerState::Pending }
    }
}

#[derive(Debug)]
pub struct LayoutTransition {
    pub blocker: LayoutBlocker,
    pub outgoing: Tag,
    pub closing: Vec<WindowElement>,
}

impl LayoutTransition {
    pub fn new(configured: Vec<(WlSurface, Serial)>, outgoing: Tag) -> Option<Self> {
        if configured.is_empty() {
            return None;
        }
        let blocker = LayoutBlocker::new(configured);
        for surface in blocker.surfaces().cloned().collect::<Vec<_>>() {
            add_blocker(&surface, blocker.clone());
        }
        Some(Self {
            blocker,
            outgoing,
            closing: Vec::new(),
        })
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
