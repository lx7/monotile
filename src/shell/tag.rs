// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle, Serial},
};

use crate::config;

use super::{TilingLayout, WindowId, Windows};

#[derive(Debug, Default, Clone)]
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
            self.layout
                .ids()
                .chain(self.floating.iter().copied())
                .collect()
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

    pub fn recompute_layout(
        &mut self,
        ws: &mut Windows,
        area: Rectangle<i32, Logical>,
        fs_geo: Rectangle<i32, Logical>,
        cfg: &config::Layout,
    ) -> Vec<(WlSurface, Serial)> {
        self.layout
            .retain(|id| ws.get(id).is_some_and(|we| !we.floating));
        self.floating
            .retain(|&id| ws.get(id).is_some_and(|we| we.floating));

        for &id in &self.focus_stack {
            let Some(we) = ws.get(id) else { continue };
            if we.floating {
                if !self.floating.contains(&id) {
                    self.floating.push(id);
                }
            } else {
                self.layout.add(id);
            }
        }
        self.fullscreen = self
            .focus_stack
            .iter()
            .copied()
            .find(|&id| ws.get(id).is_some_and(|we| we.fullscreen));

        self.layout.recompute(area, cfg);
        let mut configured = Vec::new();
        for &id in &self.focus_stack {
            let Some(we) = ws.get_mut(id) else { continue };
            let target = if we.fullscreen {
                fs_geo
            } else if we.floating {
                we.float_geo
            } else {
                let Some(rect) = self.layout.position_of(id) else {
                    continue;
                };
                rect
            };
            if let Some(serial) = we.configure(target)
                && let Some(tl) = we.window.toplevel()
            {
                configured.push((tl.wl_surface().clone(), serial));
            }
        }
        configured
    }
}
