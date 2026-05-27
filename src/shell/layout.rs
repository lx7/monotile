// SPDX-License-Identifier: GPL-3.0-only

use smithay::utils::{Logical, Rectangle};

use crate::config::{self, Rel};

use super::WindowId;

#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub id: WindowId,
    pub rect: Rectangle<i32, Logical>,
}

#[derive(Debug, Clone)]
pub struct TilingLayout {
    pub main_count: usize,
    pub main_factor: f32,
    tiles: Vec<Tile>,
}

impl Default for TilingLayout {
    fn default() -> Self {
        Self {
            main_count: config::TileConfig::default().main_count,
            main_factor: config::TileConfig::default().main_factor,
            tiles: Vec::new(),
        }
    }
}

impl TilingLayout {
    pub fn name(&self) -> &str {
        "tile"
    }

    pub fn symbol(&self) -> &str {
        "[]="
    }

    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.tiles.iter().any(|t| t.id == id)
    }

    pub fn position_of(&self, id: WindowId) -> Option<Rectangle<i32, Logical>> {
        self.tiles.iter().find(|t| t.id == id).map(|t| t.rect)
    }

    pub fn add(&mut self, id: WindowId) {
        if !self.contains(id) {
            self.tiles.push(Tile {
                id,
                rect: Rectangle::default(),
            });
        }
    }

    pub fn remove(&mut self, id: WindowId) {
        self.tiles.retain(|t| t.id != id);
    }

    pub fn retain(&mut self, mut keep: impl FnMut(WindowId) -> bool) {
        self.tiles.retain(|t| keep(t.id));
    }

    pub fn ids(&self) -> impl DoubleEndedIterator<Item = WindowId> + '_ {
        self.tiles.iter().map(|t| t.id)
    }

    pub fn target(&self, from: WindowId, to: Rel) -> Option<WindowId> {
        let cur = self.tiles.iter().position(|t| t.id == from)?;
        let n = self.tiles.len();
        let idx = match to {
            Rel::Next => (cur + 1) % n,
            Rel::Prev => (cur + n - 1) % n,
            Rel::First => 0,
            Rel::Last => n - 1,
        };
        Some(self.tiles[idx].id)
    }

    pub fn swap(&mut self, from: WindowId, to: Rel) {
        let Some(cur) = self.tiles.iter().position(|t| t.id == from) else {
            return;
        };
        let n = self.tiles.len();
        let target = match to {
            Rel::Next => (cur + 1) % n,
            Rel::Prev => (cur + n - 1) % n,
            Rel::First => 0,
            Rel::Last => n - 1,
        };
        if cur != target {
            self.tiles.swap(cur, target);
        }
    }

    pub fn adjust_main_factor(&mut self, delta: f32) {
        self.main_factor = (self.main_factor + delta).clamp(0.1, 0.9);
    }

    pub fn set_main_factor(&mut self, ratio: f32) {
        self.main_factor = ratio.clamp(0.1, 0.9);
    }

    pub fn adjust_main_count(&mut self, delta: i32) {
        self.main_count = (self.main_count as i32 + delta).max(1) as usize;
    }

    pub fn set_main_count(&mut self, count: usize) {
        self.main_count = count.max(1);
    }

    pub fn recompute(&mut self, area: Rectangle<i32, Logical>, cfg: &config::Layout) {
        let rects = self.compute_rects(self.tiles.len(), area, cfg);
        for (tile, rect) in self.tiles.iter_mut().zip(rects) {
            tile.rect = rect;
        }
    }

    pub fn compute_rects(
        &self,
        count: usize,
        area: Rectangle<i32, Logical>,
        layout: &config::Layout,
    ) -> Vec<Rectangle<i32, Logical>> {
        if count == 0 {
            return vec![];
        }

        let main_count = self.main_count.min(count);
        let stack_count = count - main_count;

        let disable_gaps = layout.smart_gaps && count == 1;
        let outer = if disable_gaps { 0 } else { layout.outer_gap };
        let inner = if disable_gaps { 0 } else { layout.inner_gap };

        let usable = if outer == 0 {
            area
        } else {
            Rectangle {
                loc: (area.loc.x + outer, area.loc.y + outer).into(),
                size: (area.size.w - 2 * outer, area.size.h - 2 * outer).into(),
            }
        };

        if stack_count == 0 {
            Self::stack_rects(count, usable, inner)
        } else {
            let half = inner / 2;
            let mw = (usable.size.w as f32 * self.main_factor) as i32;
            let main_area = Rectangle {
                loc: usable.loc,
                size: (mw - half, usable.size.h).into(),
            };
            let stack_area = Rectangle {
                loc: (usable.loc.x + mw + inner - half, usable.loc.y).into(),
                size: (usable.size.w - mw - inner + half, usable.size.h).into(),
            };
            let mut rects = Self::stack_rects(main_count, main_area, inner);
            rects.extend(Self::stack_rects(stack_count, stack_area, inner));
            rects
        }
    }

    fn stack_rects(
        count: usize,
        area: Rectangle<i32, Logical>,
        gap: i32,
    ) -> Vec<Rectangle<i32, Logical>> {
        if count == 0 {
            return vec![];
        }
        let gap_total = gap * (count as i32 - 1);
        let h = (area.size.h - gap_total) / count as i32;
        (0..count)
            .map(|i| {
                let y = area.loc.y + i as i32 * (h + gap);
                let remaining_h = area.loc.y + area.size.h - y;
                let h = if i == count - 1 { remaining_h } else { h };
                Rectangle::new((area.loc.x, y).into(), (area.size.w, h).into())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn ids(n: usize) -> Vec<WindowId> {
        let mut sm: SlotMap<WindowId, ()> = SlotMap::with_key();
        (0..n).map(|_| sm.insert(())).collect()
    }

    #[test]
    fn add_appends_in_order() {
        let mut l = TilingLayout::default();
        let [a, b, c] = ids(3).try_into().unwrap();
        l.add(a);
        l.add(b);
        l.add(c);
        assert_eq!(l.ids().collect::<Vec<_>>(), vec![a, b, c]);
    }

    #[test]
    fn add_is_idempotent() {
        let mut l = TilingLayout::default();
        let [a, b] = ids(2).try_into().unwrap();
        l.add(a);
        l.add(b);
        l.add(a);
        assert_eq!(l.len(), 2);
        assert_eq!(l.ids().collect::<Vec<_>>(), vec![a, b]);
    }

    #[test]
    fn target_cycles_with_rel() {
        let mut l = TilingLayout::default();
        let v = ids(3);
        for &id in &v {
            l.add(id);
        }
        assert_eq!(l.target(v[0], Rel::Next), Some(v[1]));
        assert_eq!(l.target(v[2], Rel::Next), Some(v[0]));
        assert_eq!(l.target(v[0], Rel::Prev), Some(v[2]));
        assert_eq!(l.target(v[1], Rel::First), Some(v[0]));
        assert_eq!(l.target(v[1], Rel::Last), Some(v[2]));
    }

    #[test]
    fn swap_reorders_tiles() {
        let mut l = TilingLayout::default();
        let v = ids(3);
        for &id in &v {
            l.add(id);
        }
        l.swap(v[0], Rel::Next);
        assert_eq!(l.ids().collect::<Vec<_>>(), vec![v[1], v[0], v[2]]);
    }

    #[test]
    fn swap_self_is_noop() {
        let mut l = TilingLayout::default();
        let a = ids(1)[0];
        l.add(a);
        l.swap(a, Rel::First);
        assert_eq!(l.ids().collect::<Vec<_>>(), vec![a]);
    }

    #[test]
    fn main_factor_clamps_above_and_below() {
        let mut l = TilingLayout::default();
        l.set_main_factor(0.05);
        assert_eq!(l.main_factor, 0.1);
        l.set_main_factor(0.99);
        assert_eq!(l.main_factor, 0.9);
        l.set_main_factor(0.5);
        l.adjust_main_factor(1.0);
        assert_eq!(l.main_factor, 0.9);
        l.adjust_main_factor(-1.0);
        assert_eq!(l.main_factor, 0.1);
    }

    #[test]
    fn main_count_clamps_to_at_least_one() {
        let mut l = TilingLayout::default();
        l.set_main_count(0);
        assert_eq!(l.main_count, 1);
        l.adjust_main_count(-10);
        assert_eq!(l.main_count, 1);
        l.adjust_main_count(3);
        assert_eq!(l.main_count, 4);
    }

    const W: i32 = 1000;
    const H: i32 = 800;

    fn area() -> Rectangle<i32, Logical> {
        Rectangle::from_size((W, H).into())
    }

    fn with_main(mcount: usize, mfact: f32) -> TilingLayout {
        let mut l = TilingLayout::default();
        l.main_count = mcount;
        l.main_factor = mfact;
        l
    }

    fn compute(layout: &TilingLayout, count: usize) -> Vec<Rectangle<i32, Logical>> {
        layout.compute_rects(count, area(), &config::Layout::default())
    }

    #[test]
    fn zero_windows() {
        let rects = compute(&TilingLayout::default(), 0);
        assert!(rects.is_empty(), "zero windows should produce no rects");
    }

    #[test]
    fn single_window_fills_area() {
        let c = config::Layout::default();
        let outer = c.outer_gap;
        let rects = compute(&TilingLayout::default(), 1);
        assert_eq!(rects.len(), 1, "single window should produce 1 rect");
        let expected = Rectangle::new((outer, outer).into(), (W - 2 * outer, H - 2 * outer).into());
        assert_eq!(rects[0], expected, "single window should fill usable area");
    }

    #[test]
    fn two_windows_even_split() {
        let c = config::Layout::default();
        let outer = c.outer_gap;
        let inner = c.inner_gap;
        let mfact = 0.5_f32;
        let half = inner / 2;
        let usable_w = W - 2 * outer;
        let usable_h = H - 2 * outer;
        let mw = (usable_w as f32 * mfact) as i32;

        let rects = compute(&with_main(1, mfact), 2);
        assert_eq!(rects.len(), 2, "two windows should produce 2 rects");

        assert_eq!(rects[0].loc, (outer, outer).into(), "main loc");
        assert_eq!(rects[0].size, (mw - half, usable_h).into(), "main size");

        let stack_x = outer + mw + inner - half;
        let stack_w = usable_w - mw - inner + half;
        assert_eq!(rects[1].loc, (stack_x, outer).into(), "stack loc");
        assert_eq!(rects[1].size, (stack_w, usable_h).into(), "stack size");
    }

    #[test]
    fn three_windows_stack_splits_vertically() {
        let c = config::Layout::default();
        let outer = c.outer_gap;
        let inner = c.inner_gap;
        let mfact = 0.5_f32;
        let half = inner / 2;
        let usable_w = W - 2 * outer;
        let usable_h = H - 2 * outer;
        let mw = (usable_w as f32 * mfact) as i32;
        let stack_x = outer + mw + inner - half;
        let stack_w = usable_w - mw - inner + half;
        let stack_h = (usable_h - inner) / 2;

        let rects = compute(&with_main(1, mfact), 3);
        assert_eq!(rects.len(), 3, "three windows should produce 3 rects");

        assert_eq!(rects[0].loc, (outer, outer).into(), "main loc");
        assert_eq!(rects[0].size, (mw - half, usable_h).into(), "main size");

        assert_eq!(rects[1].loc, (stack_x, outer).into(), "stack[0] loc");
        assert_eq!(rects[1].size, (stack_w, stack_h).into(), "stack[0] size");

        let y2 = outer + stack_h + inner;
        assert_eq!(rects[2].loc, (stack_x, y2).into(), "stack[1] loc");
        assert_eq!(rects[2].size, (stack_w, stack_h).into(), "stack[1] size");
    }

    #[test]
    fn main_count_two() {
        let rects = compute(&with_main(2, 0.5), 3);
        assert_eq!(rects.len(), 3, "should produce 3 rects");

        assert_eq!(rects[0].loc.x, rects[1].loc.x, "mains should share x");
        assert!(
            rects[0].loc.y < rects[1].loc.y,
            "main[0] should be above main[1]"
        );

        assert!(
            rects[2].loc.x > rects[0].loc.x,
            "stack should be right of main"
        );
    }

    #[test]
    fn main_count_exceeds_window_count() {
        let rects = compute(&with_main(3, 0.5), 2);
        assert_eq!(rects.len(), 2, "should produce 2 rects");
        assert_eq!(rects[0].loc.x, rects[1].loc.x, "both should share x");
        assert!(
            rects[0].loc.y < rects[1].loc.y,
            "first should be above second"
        );
    }

    #[test]
    fn mfact_extremes() {
        let rects = compute(&with_main(1, 0.1), 2);
        let main_w = rects[0].size.w;
        let stack_w = rects[1].size.w;
        assert!(
            stack_w > main_w * 5,
            "stack should be much wider: m={main_w} s={stack_w}",
        );

        let rects = compute(&with_main(1, 0.9), 2);
        let main_w = rects[0].size.w;
        let stack_w = rects[1].size.w;
        assert!(
            main_w > stack_w * 5,
            "main should be much wider: m={main_w} s={stack_w}",
        );
    }

    #[test]
    fn windows_cover_area_without_overlap() {
        let rects = compute(&with_main(1, 0.5), 4);
        assert_eq!(rects.len(), 4, "should produce 4 rects");

        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = rects[i];
                let b = rects[j];
                let overlap = a.intersection(b);
                assert!(
                    overlap.is_none() || overlap.unwrap().is_empty(),
                    "rects {i} and {j} overlap: {a:?} ∩ {b:?}",
                );
            }
        }
    }

    #[test]
    fn all_rects_fit_within_area() {
        for count in 1..=6 {
            let rects = compute(&with_main(1, 0.5), count);
            for (i, r) in rects.iter().enumerate() {
                assert!(
                    r.loc.x >= 0 && r.loc.y >= 0,
                    "count={count} rect {i} has negative loc: {r:?}",
                );
                assert!(
                    r.loc.x + r.size.w <= W && r.loc.y + r.size.h <= H,
                    "count={count} rect {i} exceeds area: {r:?}",
                );
            }
        }
    }
}
