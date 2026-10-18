// SPDX-License-Identifier: GPL-3.0-only

use super::RenderCtx;
use crate::shell::{View, Windows};

impl View {
    pub fn render_elements(&self, ctx: &mut RenderCtx, windows: &mut Windows) {
        let lone = self.tiled.len() == 1;

        for &id in self.floating.iter().rev() {
            if let Some(we) = windows.get(id) {
                we.render_popups(ctx, we.float_geo.loc);
            }
        }
        for tile in self.tiled.iter().rev() {
            if let Some(we) = windows.get(tile.id) {
                we.render_popups(ctx, tile.rect.loc);
            }
        }

        for &id in self.floating.iter().rev() {
            if let Some(we) = windows.get_mut(id) {
                let geo = we.float_geo;
                we.render_elements(ctx, geo, false);
            }
        }
        for tile in self.tiled.iter().rev() {
            if let Some(we) = windows.get_mut(tile.id) {
                we.render_elements(ctx, tile.rect, lone);
            }
        }
    }
}
