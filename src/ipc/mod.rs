// SPDX-License-Identifier: GPL-3.0-only

mod dwl_ipc;
mod dwl_ipc_protocol;
mod monotile_ipc;
mod monotile_ipc_protocol;

use wayland_server::DisplayHandle;

use crate::shell::{Monitor, Monitors, Windows};

pub use dwl_ipc::DwlIpcState;
pub use monotile_ipc::MonotileIpcState;

pub struct IpcState {
    pub dwl: DwlIpcState,
    pub monotile: MonotileIpcState,
    dirty: bool,
}

impl IpcState {
    pub fn new(dh: &DisplayHandle, tag_count: usize) -> Self {
        Self {
            dwl: DwlIpcState::new(dh, tag_count as u32),
            monotile: MonotileIpcState::new(dh, tag_count),
            dirty: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

pub struct TagSnapshot {
    pub active_tag: usize,
    pub focused_tags: u32,
    pub occupied_tags: u32,
    pub urgent_tags: u32,

    pub layout_name: String,
    pub layout_symbol: String,

    pub title: String,
    pub app_id: String,
    pub fullscreen: bool,
    pub floating: bool,
}

impl Monitor {
    pub fn snapshot(&self, windows: &Windows) -> TagSnapshot {
        let tag = self.tag();
        let focused = tag.focused_id();
        let (title, app_id, fullscreen, floating) = focused
            .and_then(|id| windows.get(id))
            .map(|we| {
                (
                    we.title.clone(),
                    we.app_id.clone(),
                    we.fullscreen,
                    we.floating,
                )
            })
            .unwrap_or_default();

        let focused_tags = 1u32 << self.active_tag;
        let mut occupied_tags = 0u32;
        let urgent_tags = 0u32; // TODO: urgent hints
        for (i, t) in self.tags.iter().enumerate() {
            if !t.focus_stack.is_empty() {
                occupied_tags |= 1 << i;
            }
        }

        TagSnapshot {
            active_tag: self.active_tag,
            focused_tags,
            occupied_tags,
            urgent_tags,

            layout_name: tag.layout.name().to_string(),
            layout_symbol: tag.layout.symbol().to_string(),

            title,
            app_id,
            fullscreen,
            floating,
        }
    }
}

impl IpcState {
    pub fn flush(&mut self, monitors: &Monitors, windows: &Windows, active_monitor: usize) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        for (i, mon) in monitors.iter().enumerate() {
            let snap = mon.snapshot(windows);
            self.monotile.notify_output(&mon.output, &snap);

            let active = i == active_monitor;
            if active {
                self.monotile.notify_seat(&snap, &mon.output);
            }

            self.dwl.notify(mon, &snap, active);
        }
    }
}
