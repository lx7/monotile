// SPDX-License-Identifier: GPL-3.0-only

use smithay::utils::{Logical, Rectangle};

use crate::config;

#[derive(Debug, Clone)]
pub struct TilingLayout {
    pub main_count: usize,
    pub main_factor: f32,
}

impl Default for TilingLayout {
    fn default() -> Self {
        Self {
            main_count: config::TileConfig::default().main_count,
            main_factor: config::TileConfig::default().main_factor,
        }
    }
}

impl TilingLayout {
    pub fn name(&self) -> &str {
        "tile"
    }

    pub fn symbol(&self) -> &str {
        "[]=".into()
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
                Rectangle::new((area.loc.x, y).into(), (area.size.w, h).into())
            })
            .collect()
    }
}
