// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::renderer::{element::Kind, glow::GlowRenderer},
    desktop::{
        LayerMap, PopupManager, WindowSurfaceType, layer_map_for_output,
        utils::under_from_surface_tree,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Scale},
    wayland::shell::wlr_layer::Layer,
};

use super::{MonotileElement, RenderCtx, Shaders};
use crate::{
    config::Layout,
    shell::{Monitor, WindowId, Windows},
};

#[derive(Default)]
pub struct SurfaceUnder {
    pub surface: Option<(WlSurface, Point<f64, Logical>)>,
    pub window: Option<WindowId>,
}

impl SurfaceUnder {
    pub fn surface_tree(
        surface: &WlSurface,
        loc: Point<i32, Logical>,
        pos: Point<f64, Logical>,
        window: Option<WindowId>,
    ) -> Option<Self> {
        let (surface, loc) = under_from_surface_tree(surface, pos, loc, WindowSurfaceType::ALL)?;
        Some(Self {
            surface: Some((surface, loc.to_f64())),
            window,
        })
    }

    pub fn popups(
        parent: &WlSurface,
        origin: Point<i32, Logical>,
        pos: Point<f64, Logical>,
        window: Option<WindowId>,
    ) -> Option<Self> {
        PopupManager::popups_for_surface(parent).find_map(|(popup, offset)| {
            let loc = origin + offset - popup.geometry().loc;
            Self::surface_tree(popup.wl_surface(), loc, pos, window)
        })
    }

    fn layers(map: &LayerMap, layer: Layer, pos: Point<f64, Logical>) -> Option<Self> {
        map.layers_on(layer).rev().find_map(|ls| {
            let loc = map.layer_geometry(ls).unwrap().loc;
            Self::surface_tree(ls.wl_surface(), loc, pos, None)
        })
    }

    fn layer_popups(map: &LayerMap, layer: Layer, pos: Point<f64, Logical>) -> Option<Self> {
        map.layers_on(layer).rev().find_map(|ls| {
            let loc = map.layer_geometry(ls).unwrap().loc;
            Self::popups(ls.wl_surface(), loc, pos, None)
        })
    }
}

impl Monitor {
    pub fn render_elements(
        &self,
        renderer: &mut GlowRenderer,
        ws: &mut Windows,
        shaders: &Shaders,
        layout: &Layout,
        locked: bool,
    ) -> Vec<MonotileElement> {
        let mut ctx = RenderCtx {
            renderer,
            shaders,
            layout,
            scale: Scale::from(self.output.current_scale().fractional_scale()),
            elems: Vec::with_capacity(ws.len() * 20 + 32),
        };
        if locked {
            if let Some(ls) = &self.lock_surface {
                ctx.surface_tree(ls.wl_surface(), Point::default(), Kind::Unspecified);
            }
            return ctx.elems;
        }
        let map = layer_map_for_output(&self.output);
        let view = self.views.front();

        ctx.layer_popups(&map, Layer::Overlay);
        ctx.layers(&map, Layer::Overlay);
        if view.fullscreen.is_some() {
            view.render_popups(&mut ctx, ws);
            view.render(&mut ctx, ws);
            return ctx.elems;
        }
        ctx.layer_popups(&map, Layer::Top);
        view.render_popups(&mut ctx, ws);
        ctx.layer_popups(&map, Layer::Bottom);
        ctx.layer_popups(&map, Layer::Background);
        ctx.layers(&map, Layer::Top);
        view.render(&mut ctx, ws);
        ctx.layers(&map, Layer::Bottom);
        ctx.layers(&map, Layer::Background);
        ctx.elems
    }

    pub fn surface_under(
        &self,
        ws: &Windows,
        pos: Point<f64, Logical>,
        locked: bool,
    ) -> SurfaceUnder {
        if locked {
            let surface = self.lock_surface.as_ref();
            let surface = surface.map(|ls| (ls.wl_surface().clone(), Point::default()));
            return SurfaceUnder {
                surface,
                window: None,
            };
        }
        let map = layer_map_for_output(&self.output);
        let view = self.views.front();
        let layers = |layer| SurfaceUnder::layers(&map, layer, pos);
        let layer_popups = |layer| SurfaceUnder::layer_popups(&map, layer, pos);

        let overlay = layer_popups(Layer::Overlay).or_else(|| layers(Layer::Overlay));
        if view.fullscreen.is_some() {
            return overlay
                .or_else(|| view.popups_under(ws, pos))
                .or_else(|| view.surface_under(ws, pos))
                .unwrap_or_default();
        }
        overlay
            .or_else(|| layer_popups(Layer::Top))
            .or_else(|| view.popups_under(ws, pos))
            .or_else(|| layer_popups(Layer::Bottom))
            .or_else(|| layer_popups(Layer::Background))
            .or_else(|| layers(Layer::Top))
            .or_else(|| view.surface_under(ws, pos))
            .or_else(|| layers(Layer::Bottom))
            .or_else(|| layers(Layer::Background))
            .unwrap_or_default()
    }
}
