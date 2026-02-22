// SPDX-License-Identifier: GPL-3.0-only

use crate::config::{BORDER_WIDTH, GAP, MASTER_COUNT, MASTER_FACTOR, SINGLE_BORDER};
use smithay::utils::{Logical, Rectangle};

#[derive(Debug, Clone)]
pub struct TilingLayout {
    pub master_count: usize,
    pub master_factor: f32,
}

impl Default for TilingLayout {
    fn default() -> Self {
        Self {
            master_count: MASTER_COUNT,
            master_factor: MASTER_FACTOR,
        }
    }
}

impl TilingLayout {
    pub fn compute_rects(
        &self,
        count: usize,
        area: Rectangle<i32, Logical>,
    ) -> Vec<Rectangle<i32, Logical>> {
        if count == 0 {
            return vec![];
        }

        let master_count = self.master_count.min(count);
        let stack_count = count - master_count;

        let edge = GAP + BORDER_WIDTH;
        let inner = GAP + 2 * BORDER_WIDTH;

        let usable = if !SINGLE_BORDER && count == 1 {
            area
        } else {
            Rectangle {
                loc: (area.loc.x + edge, area.loc.y + edge).into(),
                size: (area.size.w - 2 * edge, area.size.h - 2 * edge).into(),
            }
        };

        if stack_count == 0 {
            Self::stack_rects(count, usable, inner)
        } else {
            let half = inner / 2;
            let mw = (usable.size.w as f32 * self.master_factor) as i32;
            let master_area = Rectangle {
                loc: usable.loc,
                size: (mw - half, usable.size.h).into(),
            };
            let stack_area = Rectangle {
                loc: (usable.loc.x + mw + inner - half, usable.loc.y).into(),
                size: (usable.size.w - mw - inner + half, usable.size.h).into(),
            };
            let mut rects = Self::stack_rects(master_count, master_area, inner);
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
