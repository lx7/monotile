// SPDX-License-Identifier: GPL-3.0-only

use super::RenderCtx;
use crate::shell::{View, Windows};

impl View {
    pub fn render_popups(&self, ctx: &mut RenderCtx, ws: &Windows) {
        for element in self.stack(ws) {
            if let Some(we) = ws.get(element.id) {
                we.render_popups(ctx, element.rect);
            }
        }
    }

    pub fn render(&self, ctx: &mut RenderCtx, ws: &mut Windows) {
        if let Some(element) = self.fullscreen {
            if let Some(we) = ws.get(element.id) {
                we.render_content(ctx, element.rect);
            }
            return;
        }
        let lone = self.tiled.len() == 1;
        for element in self.stack(ws) {
            if let Some(we) = ws.get_mut(element.id) {
                we.render_elements(ctx, element.rect, lone && !we.floating);
            }
        }
    }
}
