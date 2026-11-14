// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;

use derive_more::{Deref, DerefMut};
use smithay::utils::{Logical, Point, Rectangle};

use crate::render::SurfaceUnder;

use super::{LayoutBlocker, WindowId, Windows};

#[derive(Debug, Deref, DerefMut)]
pub struct Views(VecDeque<View>);

impl Views {
    pub fn new(view: View) -> Self {
        Self(VecDeque::from([view]))
    }

    pub fn front(&self) -> &View {
        &self[0]
    }

    pub fn pop_ready(&mut self) -> bool {
        let before = self.len();
        while self.len() > 1 && self[1].blocker.is_committed() {
            self.pop_front();
        }
        self.len() != before
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewElement {
    pub id: WindowId,
    pub rect: Rectangle<i32, Logical>,
}

#[derive(Debug)]
pub struct View {
    pub fullscreen: Option<ViewElement>,
    pub tiled: Vec<ViewElement>,
    pub floating: Vec<WindowId>,
    pub blocker: LayoutBlocker,
}

impl View {
    pub fn stack(&self, ws: &Windows) -> Vec<ViewElement> {
        let floating = self.floating.iter().rev().filter_map(|&id| {
            let rect = ws.get(id)?.float_geo;
            Some(ViewElement { id, rect })
        });
        let tiled = self.tiled.iter().rev().copied();
        let fullscreen = self.fullscreen.into_iter();
        fullscreen.chain(floating).chain(tiled).collect()
    }

    pub fn popups_under(&self, ws: &Windows, pos: Point<f64, Logical>) -> Option<SurfaceUnder> {
        let stack = self.stack(ws);
        stack
            .iter()
            .find_map(|e| ws.get(e.id)?.popups_under(e.rect, pos))
    }

    pub fn surface_under(&self, ws: &Windows, pos: Point<f64, Logical>) -> Option<SurfaceUnder> {
        let stack = self.stack(ws);
        stack
            .iter()
            .find_map(|e| ws.get(e.id)?.surface_under(e.rect, pos))
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.fullscreen.map(|e| e.id) == Some(id)
            || self.tiled.iter().any(|e| e.id == id)
            || self.floating.contains(&id)
    }
}
