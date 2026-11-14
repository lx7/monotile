// SPDX-License-Identifier: GPL-3.0-only

use super::RenderCtx;
use crate::shell::{View, Windows};

impl View {
    pub fn render_popups(&self, ctx: &mut RenderCtx, ws: &Windows) {
        for placed in self.stack(ws) {
            if let Some(we) = ws.get(placed.id) {
                we.render_popups(ctx, placed.rect);
            }
        }
    }

    pub fn render(&self, ctx: &mut RenderCtx, ws: &mut Windows) {
        if let Some(placed) = self.fullscreen {
            if let Some(we) = ws.get(placed.id) {
                we.render_content(ctx, placed.rect);
            }
            return;
        }
        let lone = self.tiled.len() == 1;
        for placed in self.stack(ws) {
            if let Some(we) = ws.get_mut(placed.id) {
                we.render_elements(ctx, placed.rect, lone && !we.floating);
            }
        }
    }
}
