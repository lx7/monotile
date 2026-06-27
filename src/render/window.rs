// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::renderer::{
        Renderer,
        element::{
            Kind,
            surface::render_elements_from_surface_tree,
            texture::{TextureBuffer, TextureRenderElement},
        },
        gles::{Uniform, element::PixelShaderElement},
        glow::GlowRenderer,
        utils::with_renderer_surface_state,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Logical, Point, Rectangle, Scale},
    wayland::seat::WaylandFocus,
};

use super::{
    MonotileElement, RenderCtx, border,
    clipped_surface::{Clippable, Clipped},
    popup_elements,
};
use crate::{
    config,
    shell::{View, WindowElement, Windows},
};

#[derive(Debug)]
pub enum RenderStep {
    Border {
        width: i32,
        color: [f32; 4],
        elements: Vec<PixelShaderElement>,
    },
    WindowSurface {
        fill: [f32; 4],
        radius: f32,
        background: Option<PixelShaderElement>,
    },
    Shadow {
        softness: i32,
        spread: i32,
        offset: (i32, i32),
        color: [f32; 4],
        element: Option<PixelShaderElement>,
    },
}

impl RenderStep {
    pub fn from_config(step: &config::RenderStep) -> Option<Self> {
        Some(match step {
            config::RenderStep::Noop => return None,
            config::RenderStep::Border { width, color } => Self::Border {
                width: *width,
                color: color.0,
                elements: Vec::new(),
            },
            config::RenderStep::WindowSurface { fill, radius } => Self::WindowSurface {
                fill: fill.0,
                radius: *radius,
                background: None,
            },
            config::RenderStep::Shadow {
                softness,
                spread,
                offset,
                color,
            } => Self::Shadow {
                softness: *softness,
                spread: *spread,
                offset: *offset,
                color: color.0,
                element: None,
            },
        })
    }

    fn clear(&mut self) {
        match self {
            Self::Border { elements, .. } => elements.clear(),
            Self::WindowSurface { background, .. } => *background = None,
            Self::Shadow { element, .. } => *element = None,
        }
    }

    fn render_elements(
        &mut self,
        ctx: &mut RenderCtx,
        content: &mut Vec<Clippable>,
        win_geo: Rectangle<i32, Logical>,
        radius: f32,
        surface_fills_win: bool,
    ) {
        let scale_f32 = ctx.scale.x as f32;
        match self {
            RenderStep::Border {
                width,
                color,
                elements,
            } => {
                if elements.is_empty() {
                    *elements = border::create_elements(
                        &ctx.shaders.rect,
                        win_geo,
                        radius,
                        *width,
                        *color,
                        scale_f32,
                    );
                }
                for d in elements.iter() {
                    ctx.elems.push(MonotileElement::Decoration(d.clone()));
                }
            }
            RenderStep::WindowSurface {
                fill,
                radius: r,
                background,
            } => {
                let clip_r = if radius == 0.0 { 0.0 } else { *r };
                for clippable in content.drain(..) {
                    ctx.elems.push(Clipped::wrap(
                        clippable,
                        &ctx.shaders.clip,
                        win_geo,
                        clip_r,
                        ctx.scale,
                    ));
                }
                if !surface_fills_win {
                    let bg = background.get_or_insert_with(|| {
                        PixelShaderElement::new(
                            ctx.shaders.rect.clone(),
                            win_geo,
                            None,
                            1.0,
                            vec![
                                Uniform::new(
                                    "outer_size",
                                    (win_geo.size.w as f32, win_geo.size.h as f32),
                                ),
                                Uniform::new("border_width", 0.0f32),
                                Uniform::new("outer_radius", clip_r),
                                Uniform::new("border_color", *fill),
                                Uniform::new("piece_offset", (0.0f32, 0.0f32)),
                                Uniform::new("scale", scale_f32),
                            ],
                            Kind::Unspecified,
                        )
                    });
                    ctx.elems.push(MonotileElement::Decoration(bg.clone()));
                } else {
                    *background = None;
                }
            }
            RenderStep::Shadow {
                softness,
                spread,
                offset,
                color,
                element,
            } => {
                let sigma = *softness as f32 / 2.0;
                let blur = (sigma * 3.0).ceil() as i32;
                let pad_x = blur + *spread + offset.0.abs();
                let pad_y = blur + *spread + offset.1.abs();
                let rect = Rectangle::new(
                    (win_geo.loc.x - pad_x, win_geo.loc.y - pad_y).into(),
                    (win_geo.size.w + 2 * pad_x, win_geo.size.h + 2 * pad_y).into(),
                );
                let shadow = element.get_or_insert_with(|| {
                    PixelShaderElement::new(
                        ctx.shaders.shadow.clone(),
                        rect,
                        None,
                        1.0,
                        vec![
                            Uniform::new(
                                "win_size",
                                (win_geo.size.w as f32, win_geo.size.h as f32),
                            ),
                            Uniform::new("win_offset", (pad_x as f32, pad_y as f32)),
                            Uniform::new("outer_radius", radius),
                            Uniform::new(
                                "shadow_box_size",
                                (
                                    (win_geo.size.w + 2 * *spread) as f32,
                                    (win_geo.size.h + 2 * *spread) as f32,
                                ),
                            ),
                            Uniform::new(
                                "shadow_box_offset",
                                (
                                    (pad_x - *spread + offset.0) as f32,
                                    (pad_y - *spread + offset.1) as f32,
                                ),
                            ),
                            Uniform::new("shadow_sigma", sigma),
                            Uniform::new("shadow_color", *color),
                            Uniform::new("scale", scale_f32),
                        ],
                        Kind::Unspecified,
                    )
                });
                ctx.elems.push(MonotileElement::Decoration(shadow.clone()));
            }
        }
    }
}

impl WindowElement {
    fn live_surface(&self) -> Option<WlSurface> {
        let wl = self.window.wl_surface()?.into_owned();
        let has_buffer =
            with_renderer_surface_state(&wl, |st| st.buffer_size().is_some()).unwrap_or(false);
        (wl.alive() && has_buffer).then_some(wl)
    }

    fn sync_render_cache(&mut self, win_geo: Rectangle<i32, Logical>) {
        if win_geo != self.cache_geo {
            for step in self.render_steps.values_mut() {
                step.clear();
            }
            self.cache_geo = win_geo;
        }
    }

    pub fn render_content(
        &self,
        renderer: &mut GlowRenderer,
        origin: Point<i32, Logical>,
        scale: Scale<f64>,
        kind: Kind,
    ) -> Vec<MonotileElement> {
        let Some(wl) = self.window.wl_surface() else {
            return Vec::new();
        };
        let mut elems = popup_elements(renderer, &wl, origin, scale);
        let surf_loc = self.surface_loc(origin).to_physical_precise_round(scale);
        let surfs = render_elements_from_surface_tree(renderer, &wl, surf_loc, scale, 1.0, kind);
        elems.extend(surfs.into_iter().map(MonotileElement::Surface));
        elems
    }

    pub fn render_elements(
        &mut self,
        ctx: &mut RenderCtx,
        win_geo: Rectangle<i32, Logical>,
        lone: bool,
    ) {
        let disable_border = ctx.layout.smart_borders && lone;
        let disable_gaps = ctx.layout.smart_gaps && lone;

        self.sync_render_cache(win_geo);
        let surf_loc = self
            .surface_loc(win_geo.loc)
            .to_physical_precise_round(ctx.scale);
        let live = self.live_surface();

        let mut content: Vec<Clippable> = match &live {
            // render live surface
            Some(wl) => render_elements_from_surface_tree(
                ctx.renderer,
                wl,
                surf_loc,
                ctx.scale,
                1.0,
                Kind::Unspecified,
            )
            .into_iter()
            .map(Clippable::Surface)
            .collect(),
            // no live surface, use the last snapshot
            None => self
                .last_texture
                .iter()
                .map(|buf| {
                    Clippable::Texture(TextureRenderElement::from_texture_buffer(
                        surf_loc.to_f64(),
                        buf,
                        None,
                        None,
                        None,
                        Kind::Unspecified,
                    ))
                })
                .collect(),
        };
        if content.is_empty() {
            return;
        }

        let surface_fills_win = live.is_some() && self.window.geometry().size == win_geo.size;

        if let Some(wl) = &live {
            let popups = popup_elements(ctx.renderer, wl, win_geo.loc, ctx.scale);
            ctx.elems.extend(popups);
        }

        for &key in self.render_pipeline.iter().rev() {
            let step = self.render_steps.get_mut(&key).expect("render_step exists");
            let skip = match step {
                RenderStep::Border { width, .. } => disable_border || *width <= 0,
                RenderStep::Shadow { .. } => disable_gaps,
                RenderStep::WindowSurface { .. } => false,
            };
            if !skip {
                step.render_elements(ctx, &mut content, win_geo, self.radius, surface_fills_win);
            }
        }

        // refresh the snapshot when the client committed a new buffer
        if self.texture_dirty
            && let Some(wl) = &live
            && let Some(tex) = with_renderer_surface_state(wl, |state| {
                let cid = ctx.renderer.context_id();
                Some(TextureBuffer::from_texture(
                    ctx.renderer,
                    state.texture(cid)?.clone(),
                    state.buffer_scale(),
                    state.buffer_transform(),
                    None,
                ))
            })
            .flatten()
        {
            self.last_texture = Some(tex);
            self.texture_dirty = false;
        }
    }
}

impl View {
    pub fn render_elements(&self, ctx: &mut RenderCtx, windows: &mut Windows) {
        let lone = self.tiled.len() == 1;
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
