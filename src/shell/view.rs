// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;

use derive_more::{Deref, DerefMut};
use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle, Serial},
};

use super::{LayoutBlocker, Tag, WindowId};

#[derive(Debug, Default, Deref, DerefMut)]
pub struct Views(VecDeque<View>);

impl Views {
    pub fn pop_ready(&mut self) -> bool {
        let before = self.len();
        while self.len() > 1 && self[1].blocker.is_committed() {
            self.pop_front();
        }
        self.len() != before
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub id: WindowId,
    pub rect: Rectangle<i32, Logical>,
}

#[derive(Debug)]
pub struct View {
    pub fullscreen: Option<WindowId>,
    pub tiled: Vec<Tile>,
    pub floating: Vec<WindowId>,
    pub blocker: LayoutBlocker,
}

impl View {
    pub fn project(tag: &Tag, configured: Vec<(WlSurface, Serial)>) -> Self {
        let (fullscreen, tiled, floating) = if let Some(id) = tag.fullscreen {
            (Some(id), Vec::new(), Vec::new())
        } else {
            (None, tag.layout.tiles().to_vec(), tag.floating.clone())
        };
        Self {
            fullscreen,
            tiled,
            floating,
            blocker: LayoutBlocker::install(configured),
        }
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.fullscreen == Some(id)
            || self.tiled.iter().any(|t| t.id == id)
            || self.floating.contains(&id)
    }
}
