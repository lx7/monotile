// SPDX-License-Identifier: GPL-3.0-only

mod dwl_ipc;
mod dwl_ipc_protocol;
mod monotile_ipc;
mod monotile_ipc_protocol;
use wayland_server::DisplayHandle;

use crate::handlers::screencopy::ScreencopyState;
use crate::shell::{Monitor, Windows};
use crate::state::State;
pub use dwl_ipc::DwlIpcState;
pub use monotile_ipc::MonotileIpcState;

pub struct IpcState {
    pub dwl: DwlIpcState,
    pub monotile: MonotileIpcState,
    pub dirty: bool,
}

impl IpcState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            dwl: DwlIpcState::new(dh),
            monotile: MonotileIpcState::new(dh),
            dirty: false,
        }
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

    pub screencast: bool,
}

impl Monitor {
    pub fn snapshot(&self, windows: &Windows, screencopy: &ScreencopyState) -> TagSnapshot {
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
        let mut urgent_tags = 0u32;
        for (i, t) in self.tags.iter().enumerate() {
            if !t.focus_stack.is_empty() {
                occupied_tags |= 1 << i;
            }
            if t.focus_stack
                .iter()
                .any(|&id| windows.get(id).is_some_and(|w| w.urgent))
            {
                urgent_tags |= 1 << i;
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

            screencast: screencopy.output_captured(&self.output),
        }
    }
}

impl State {
    pub(crate) fn flush_ipc(&mut self) {
        if !self.ipc.dirty {
            return;
        }
        self.ipc.dirty = false;
        for (i, mon) in self.monitors.iter().enumerate() {
            let snap = mon.snapshot(&self.windows, &self.screencopy);
            self.ipc.monotile.notify_output(&mon.output, &snap);
            if i == self.active_monitor {
                self.ipc.monotile.notify_seat(&snap, &mon.output);
            }
            self.ipc.dwl.notify(mon, &snap, i == self.active_monitor);
        }
    }
}
