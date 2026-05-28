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
    utils::{IsAlive, Logical, Rectangle, Scale},
    wayland::seat::WaylandFocus,
};

use super::{
    MonotileElement, Shaders, border,
    clipped_surface::{Clippable, Clipped},
    popup_elements,
};
use crate::{config, shell::WindowElement};

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

    pub fn clear(&mut self) {
        match self {
            Self::Border { elements, .. } => elements.clear(),
            Self::WindowSurface { background, .. } => *background = None,
            Self::Shadow { element, .. } => *element = None,
        }
    }

    fn render_elements(
        &mut self,
        out: &mut Vec<MonotileElement>,
        content: &mut Vec<Clippable>,
        shaders: &Shaders,
        win_geo: Rectangle<i32, Logical>,
        radius: f32,
        surface_fills_win: bool,
        scale: Scale<f64>,
    ) {
        let scale_f32 = scale.x as f32;
        match self {
            RenderStep::Border {
                width,
                color,
                elements,
            } => {
                if elements.is_empty() {
                    *elements = border::create_elements(
                        &shaders.rect,
                        win_geo,
                        radius,
                        *width,
                        *color,
                        scale_f32,
                    );
                }
                for d in elements.iter() {
                    out.push(MonotileElement::Decoration(d.clone()));
                }
            }
            RenderStep::WindowSurface {
                fill,
                radius: r,
                background,
            } => {
                let clip_r = if radius == 0.0 { 0.0 } else { *r };
                for clippable in content.drain(..) {
                    out.push(Clipped::wrap(
                        clippable,
                        &shaders.clip,
                        win_geo,
                        clip_r,
                        scale,
                    ));
                }
                if !surface_fills_win {
                    let bg = background.get_or_insert_with(|| {
                        PixelShaderElement::new(
                            shaders.rect.clone(),
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
                    out.push(MonotileElement::Decoration(bg.clone()));
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
                        shaders.shadow.clone(),
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
                out.push(MonotileElement::Decoration(shadow.clone()));
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

    pub fn render_elements(
        &mut self,
        out: &mut Vec<MonotileElement>,
        renderer: &mut GlowRenderer,
        shaders: &Shaders,
        scale: Scale<f64>,
        disable_border: bool,
        disable_gaps: bool,
    ) {
        let win_geo = self.render_geo;
        let surf_loc = self.surface_loc().to_physical_precise_round(scale);
        let live = self.live_surface();

        let mut content: Vec<Clippable> = match &live {
            // render live surface
            Some(wl) => render_elements_from_surface_tree(
                renderer,
                wl,
                surf_loc,
                scale,
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
            out.extend(popup_elements(renderer, wl, win_geo.loc, scale));
        }

        for &key in self.render_pipeline.iter().rev() {
            let step = self.render_steps.get_mut(&key).expect("render_step exists");
            let skip = match step {
                RenderStep::Border { width, .. } => disable_border || *width <= 0,
                RenderStep::Shadow { .. } => disable_gaps,
                RenderStep::WindowSurface { .. } => false,
            };
            if !skip {
                step.render_elements(
                    out,
                    &mut content,
                    shaders,
                    win_geo,
                    self.radius,
                    surface_fills_win,
                    scale,
                );
            }
        }

        // snapshot the live texture for layout transitions
        if let Some(wl) = &live
            && let Some(tex) = with_renderer_surface_state(wl, |state| {
                Some(TextureBuffer::from_texture(
                    renderer,
                    state.texture(renderer.context_id())?.clone(),
                    state.buffer_scale(),
                    state.buffer_transform(),
                    None,
                ))
            })
            .flatten()
        {
            self.last_texture = Some(tex);
        }
    }
}
