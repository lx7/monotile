// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use smithay::{
    delegate_foreign_toplevel_list,
    wayland::foreign_toplevel_list::{
        ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
    },
};
use wayland_server::DisplayHandle;

use crate::{
    Monotile,
    shell::{WindowId, Windows},
};

pub struct ForeignToplevelState {
    list: ForeignToplevelListState,
    handles: HashMap<WindowId, ForeignToplevelHandle>,
}

impl ForeignToplevelState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            list: ForeignToplevelListState::new::<Monotile>(dh),
            handles: HashMap::new(),
        }
    }

    pub fn add(&mut self, id: WindowId, title: &str, app_id: &str) {
        let handle = self.list.new_toplevel::<Monotile>(title, app_id);
        handle.user_data().insert_if_missing(|| id);
        self.handles.insert(id, handle);
    }

    pub fn remove(&mut self, id: WindowId) {
        if let Some(handle) = self.handles.remove(&id) {
            self.list.remove_toplevel(&handle);
        }
    }

    pub fn flush(&mut self, windows: &Windows) {
        for (id, handle) in &self.handles {
            let Some(we) = windows.get(*id) else { continue };
            let mut changed = false;
            if handle.title() != we.title {
                handle.send_title(&we.title);
                changed = true;
            }
            if handle.app_id() != we.app_id {
                handle.send_app_id(&we.app_id);
                changed = true;
            }
            if changed {
                handle.send_done();
            }
        }
        self.list.cleanup_closed_handles();
    }
}

impl ForeignToplevelListHandler for Monotile {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.state.foreign_toplevel.list
    }
}
delegate_foreign_toplevel_list!(Monotile);
