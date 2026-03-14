// SPDX-License-Identifier: GPL-3.0-only

use crate::config::Rel;

use super::{TilingLayout, WindowId};

#[derive(Debug, Default)]
pub struct Tag {
    pub tiled: Vec<WindowId>,
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
        self.tiled.retain(|&wid| wid != id);
        self.floating.retain(|&wid| wid != id);
        self.focus_stack.retain(|&wid| wid != id);
        if self.fullscreen == Some(id) {
            self.fullscreen = None;
        }
    }

    pub fn add(&mut self, id: WindowId) {
        self.remove(id);
        self.focus_stack.insert(0, id);
    }

    pub fn window_ids(&self) -> impl DoubleEndedIterator<Item = WindowId> + '_ {
        self.tiled.iter().chain(self.floating.iter()).copied()
    }

    pub fn promote(&mut self, id: WindowId) {
        self.focus_stack.retain(|&x| x != id);
        self.focus_stack.insert(0, id);
    }

    pub fn target(&self, from: WindowId, to: Rel) -> Option<WindowId> {
        let cur = self.tiled_pos(from)?;
        match to {
            Rel::Next => Some(self.tiled[(cur + 1) % self.tiled.len()]),
            Rel::Prev => Some(self.tiled[(cur + self.tiled.len() - 1) % self.tiled.len()]),
            Rel::First => self.tiled.first().copied(),
            Rel::Last => self.tiled.last().copied(),
        }
    }

    pub fn swap(&mut self, from: WindowId, to: Rel) {
        let Some(cur) = self.tiled_pos(from) else {
            return;
        };
        let target = match to {
            Rel::Next => (cur + 1) % self.tiled.len(),
            Rel::Prev => (cur + self.tiled.len() - 1) % self.tiled.len(),
            Rel::First => 0,
            Rel::Last => self.tiled.len() - 1,
        };
        if cur != target {
            self.tiled.swap(cur, target);
        }
    }

    pub fn focused_id(&self) -> Option<WindowId> {
        self.focus_stack.first().copied()
    }

    fn tiled_pos(&self, id: WindowId) -> Option<usize> {
        self.tiled.iter().position(|&x| x == id)
    }

    pub fn raise(&mut self, id: WindowId) {
        if let Some(pos) = self.floating.iter().position(|&wid| wid == id) {
            let id = self.floating.remove(pos);
            self.floating.push(id);
        }
    }

    pub fn adjust_main_factor(&mut self, delta: f32) {
        self.layout.main_factor = (self.layout.main_factor + delta).clamp(0.1, 0.9);
    }

    pub fn set_main_factor(&mut self, ratio: f32) {
        self.layout.main_factor = ratio.clamp(0.1, 0.9);
    }

    pub fn adjust_main_count(&mut self, delta: i32) {
        self.layout.main_count = (self.layout.main_count as i32 + delta).max(1) as usize;
    }

    pub fn set_main_count(&mut self, count: usize) {
        self.layout.main_count = count.max(1);
    }
}
