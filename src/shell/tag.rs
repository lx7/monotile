// SPDX-License-Identifier: GPL-3.0-only

use super::{TilingLayout, WindowId};

#[derive(Debug, Default)]
pub struct Tag {
    pub floating: Vec<WindowId>,
    pub focus_stack: Vec<WindowId>,
    pub layout: TilingLayout,
    pub fullscreen: Option<WindowId>,
}

impl Tag {
    pub fn contains(&self, id: WindowId) -> bool {
        self.focus_stack.contains(&id)
    }

    pub fn remove(&mut self, id: WindowId) {
        self.layout.remove(id);
        self.floating.retain(|&wid| wid != id);
        self.focus_stack.retain(|&wid| wid != id);
        if self.fullscreen == Some(id) {
            self.fullscreen = None;
        }
    }

    pub fn add(&mut self, id: WindowId) {
        self.remove(id);

        if self.fullscreen.is_some() {
            let pos = 1.min(self.focus_stack.len());
            self.focus_stack.insert(pos, id);
        } else {
            self.focus_stack.insert(0, id);
        }
    }

    pub fn window_ids(&self) -> Vec<WindowId> {
        if let Some(fs) = self.fullscreen {
            vec![fs]
        } else {
            self.layout.ids().chain(self.floating.iter().copied()).collect()
        }
    }

    pub fn promote(&mut self, id: WindowId) {
        self.focus_stack.retain(|&x| x != id);
        self.focus_stack.insert(0, id);
    }

    pub fn focused_id(&self) -> Option<WindowId> {
        self.focus_stack.first().copied()
    }

    pub fn raise(&mut self, id: WindowId) {
        if let Some(pos) = self.floating.iter().position(|&wid| wid == id) {
            let id = self.floating.remove(pos);
            self.floating.push(id);
        }
    }
}
