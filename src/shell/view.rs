// SPDX-License-Identifier: GPL-3.0-only

use smithay::utils::{Logical, Point, Rectangle};

use super::{Tag, WindowId, Windows};

#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub id: WindowId,
    pub rect: Rectangle<i32, Logical>,
}

#[derive(Debug, Default, Clone)]
pub struct View {
    pub fullscreen: Option<Tile>,
    pub tiled: Vec<Tile>,
    pub floating: Vec<Tile>,
}

impl View {
    pub fn project(tag: &Tag, ws: &Windows, fs_geo: Rectangle<i32, Logical>) -> Self {
        if let Some(id) = tag.fullscreen {
            return Self {
                fullscreen: Some(Tile { id, rect: fs_geo }),
                tiled: Vec::new(),
                floating: Vec::new(),
            };
        }
        let tiled = tag.layout.tiles().to_vec();
        let floating = tag
            .floating
            .iter()
            .filter_map(|&id| {
                ws.get(id).map(|we| Tile {
                    id,
                    rect: we.float_geo,
                })
            })
            .collect();
        Self {
            fullscreen: None,
            tiled,
            floating,
        }
    }

    pub fn rect_of(&self, id: WindowId) -> Option<Rectangle<i32, Logical>> {
        self.fullscreen
            .iter()
            .chain(&self.tiled)
            .chain(&self.floating)
            .find(|t| t.id == id)
            .map(|t| t.rect)
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<Tile> {
        self.floating
            .iter()
            .rev()
            .chain(self.tiled.iter().rev())
            .find(|t| t.rect.to_f64().contains(pos))
            .copied()
    }
}
