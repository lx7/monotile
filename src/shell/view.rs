// SPDX-License-Identifier: GPL-3.0-only

use smithay::utils::{Logical, Rectangle};

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
}
