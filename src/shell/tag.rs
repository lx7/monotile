// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    output::Output,
    utils::{Logical, Rectangle, Size},
};

use crate::config::Rel;

use super::{LayoutBlocker, OutputExt, Placed, TilingLayout, View, WindowId, Windows};

#[derive(Debug, Default, Clone)]
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
            let ids = self.tiled.iter().chain(&self.floating);
            ids.copied().collect()
        }
    }

    fn tiled_index(&self, cur: usize, to: Rel) -> usize {
        let n = self.tiled.len();
        match to {
            Rel::Next => (cur + 1) % n,
            Rel::Prev => (cur + n - 1) % n,
            Rel::First => 0,
            Rel::Last => n - 1,
        }
    }

    pub fn target(&self, from: WindowId, to: Rel) -> Option<WindowId> {
        let cur = self.tiled.iter().position(|&id| id == from)?;
        Some(self.tiled[self.tiled_index(cur, to)])
    }

    pub fn swap(&mut self, from: WindowId, to: Rel) {
        let Some(cur) = self.tiled.iter().position(|&id| id == from) else {
            return;
        };
        let target = self.tiled_index(cur, to);
        self.tiled.swap(cur, target);
    }

    pub fn next_tiled_size(&self, area: Rectangle<i32, Logical>) -> Size<i32, Logical> {
        let rects = self.layout.compute_rects(self.tiled.len() + 1, area);
        rects.last().map(|r| r.size).unwrap_or(area.size)
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

    pub fn recompute_layout(&mut self, ws: &mut Windows, output: &Output) -> View {
        self.tiled
            .retain(|&id| ws.get(id).is_some_and(|we| !we.floating));
        self.floating
            .retain(|&id| ws.get(id).is_some_and(|we| we.floating));

        for &id in &self.focus_stack {
            let Some(we) = ws.get(id) else { continue };
            let group = if we.floating { &mut self.floating } else { &mut self.tiled };
            if !group.contains(&id) {
                group.push(id);
            }
        }
        self.fullscreen = self
            .focus_stack
            .iter()
            .copied()
            .find(|&id| ws.get(id).is_some_and(|we| we.fullscreen));

        let mut configured = Vec::new();
        let rect = output.geometry();
        let fullscreen = self.fullscreen.map(|id| Placed { id, rect });
        let (tiled, floating) = if let Some(placed) = fullscreen {
            if let Some(we) = ws.get_mut(placed.id) {
                configured.extend(we.configure(placed.rect.size));
            }
            (Vec::new(), Vec::new())
        } else {
            let area = output.usable_area();
            let rects = self.layout.compute_rects(self.tiled.len(), area);
            let tiled = self.tiled.iter().zip(rects);
            let tiled: Vec<Placed> = tiled.map(|(&id, rect)| Placed { id, rect }).collect();
            for placed in &tiled {
                if let Some(we) = ws.get_mut(placed.id) {
                    configured.extend(we.configure(placed.rect.size));
                }
            }
            for &id in &self.floating {
                if let Some(we) = ws.get_mut(id) {
                    configured.extend(we.configure(we.float_geo.size));
                }
            }
            (tiled, self.floating.clone())
        };
        View {
            fullscreen,
            tiled,
            floating,
            blocker: LayoutBlocker::install(configured),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_with_tiled(n: usize) -> Tag {
        Tag {
            tiled: (0..n).map(WindowId).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn target_cycles_with_rel() {
        let tag = tag_with_tiled(3);
        let v = &tag.tiled;
        assert_eq!(tag.target(v[0], Rel::Next), Some(v[1]));
        assert_eq!(tag.target(v[2], Rel::Next), Some(v[0]));
        assert_eq!(tag.target(v[0], Rel::Prev), Some(v[2]));
        assert_eq!(tag.target(v[1], Rel::First), Some(v[0]));
        assert_eq!(tag.target(v[1], Rel::Last), Some(v[2]));
    }

    #[test]
    fn target_of_a_window_that_is_not_tiled_is_none() {
        let tag = tag_with_tiled(2);
        assert_eq!(tag.target(WindowId(7), Rel::Next), None);
    }

    #[test]
    fn swap_reorders_tiled() {
        let mut tag = tag_with_tiled(3);
        let v = tag.tiled.clone();
        tag.swap(v[0], Rel::Next);
        assert_eq!(tag.tiled, vec![v[1], v[0], v[2]]);
    }

    #[test]
    fn swap_self_is_noop() {
        let mut tag = tag_with_tiled(1);
        let a = tag.tiled[0];
        tag.swap(a, Rel::First);
        assert_eq!(tag.tiled, vec![a]);
    }
}
