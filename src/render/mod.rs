// SPDX-License-Identifier: GPL-3.0-only

mod border;
pub mod clipped_surface;
mod shaders;

use std::borrow::BorrowMut;

use smithay::{
    backend::renderer::{
        RendererSuper,
        damage::{OutputDamageTracker, RenderOutputResult},
        element::{
            Kind, render_elements,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{
            GlesPixelProgram, GlesRenderer, GlesTarget, GlesTexProgram, Uniform, UniformName,
            UniformType, element::PixelShaderElement,
        },
        glow::GlowRenderer,
    },
    desktop::{PopupManager, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Scale},
    wayland::{seat::WaylandFocus, shell::wlr_layer::Layer},
};

use crate::{config::*, shell::WindowElement};
use clipped_surface::ClippedSurface;

type RenderResult<'a> = Result<
    RenderOutputResult<'a>,
    smithay::backend::renderer::damage::Error<<GlowRenderer as RendererSuper>::Error>,
>;

render_elements! {
    pub MonotileElement<=GlowRenderer>;
    Surface=WaylandSurfaceRenderElement<GlowRenderer>,
    Clipped=ClippedSurface,
    Decoration=PixelShaderElement,
}

#[derive(Debug)]
pub struct Shaders {
    pub rect: GlesPixelProgram,
    pub shadow: GlesPixelProgram,
    pub clip: GlesTexProgram,
}

pub fn compile_shaders(renderer: &mut GlowRenderer) -> Shaders {
    let gles: &mut GlesRenderer = renderer.borrow_mut();

    let rect = gles
        .compile_custom_pixel_shader(
            shaders::ROUNDED_RECT_FRAG,
            &[
                UniformName::new("outer_size", UniformType::_2f),
                UniformName::new("border_width", UniformType::_1f),
                UniformName::new("border_color", UniformType::_4f),
                UniformName::new("outer_radius", UniformType::_1f),
                UniformName::new("piece_offset", UniformType::_2f),
                UniformName::new("scale", UniformType::_1f),
            ],
        )
        .expect("rounded rectangle shader");
    let shadow = gles
        .compile_custom_pixel_shader(
            shaders::SHADOW_FRAG,
            &[
                UniformName::new("win_size", UniformType::_2f),
                UniformName::new("win_offset", UniformType::_2f),
                UniformName::new("outer_radius", UniformType::_1f),
                UniformName::new("shadow_box_size", UniformType::_2f),
                UniformName::new("shadow_box_offset", UniformType::_2f),
                UniformName::new("shadow_sigma", UniformType::_1f),
                UniformName::new("shadow_color", UniformType::_4f),
                UniformName::new("scale", UniformType::_1f),
            ],
        )
        .expect("shadow shader");
    let clip = gles
        .compile_custom_texture_shader(
            shaders::CLIPPED_SURFACE_FRAG,
            &[
                UniformName::new("geo_size", UniformType::_2f),
                UniformName::new("inner_radius", UniformType::_1f),
                UniformName::new("scale", UniformType::_1f),
                UniformName::new("input_to_geo", UniformType::Matrix3x3),
            ],
        )
        .expect("clip shader");
    Shaders { rect, shadow, clip }
}

fn layer_elements(
    renderer: &mut GlowRenderer,
    output: &Output,
    layers: &[Layer],
) -> Vec<MonotileElement> {
    let map = layer_map_for_output(output);
    let scale = Scale::from(SCALE);
    let mut elems = Vec::new();
    for layer in layers {
        for surface in map.layers_on(*layer).rev() {
            let geo = map.layer_geometry(surface).unwrap();
            let surfs = render_elements_from_surface_tree(
                renderer,
                surface.wl_surface(),
                geo.loc.to_physical_precise_round(SCALE),
                scale,
                1.0,
                Kind::Unspecified,
            );
            elems.extend(surfs.into_iter().map(MonotileElement::Surface));
        }
    }
    elems
}

fn layer_popup_elements(renderer: &mut GlowRenderer, output: &Output) -> Vec<MonotileElement> {
    let map = layer_map_for_output(output);
    let mut elems = Vec::new();
    for layer in [Layer::Overlay, Layer::Top, Layer::Bottom, Layer::Background] {
        for surface in map.layers_on(layer).rev() {
            let geo = map.layer_geometry(surface).unwrap();
            elems.extend(popup_elements(renderer, surface.wl_surface(), geo.loc));
        }
    }
    elems
}

fn popup_elements(
    renderer: &mut GlowRenderer,
    surface: &WlSurface,
    origin: Point<i32, Logical>,
) -> Vec<MonotileElement> {
    let scale = Scale::from(SCALE);
    let mut elems = Vec::new();
    for (popup, offset) in PopupManager::popups_for_surface(surface) {
        let pos = origin + offset - popup.geometry().loc;
        let surfs = render_elements_from_surface_tree(
            renderer,
            popup.wl_surface(),
            pos.to_physical_precise_round(SCALE),
            scale,
            1.0,
            Kind::Unspecified,
        );
        elems.extend(surfs.into_iter().map(MonotileElement::Surface));
    }
    elems
}

pub fn render_output<'a>(
    renderer: &mut GlowRenderer,
    target: &mut GlesTarget<'_>,
    tracker: &'a mut OutputDamageTracker,
    age: usize,
    windows: Vec<&WindowElement>,
    output: &Output,
    shaders: &Shaders,
) -> RenderResult<'a> {
    let sigma = SHADOW_SOFTNESS as f32 / 2.0;
    let blur = (sigma * 3.0).ceil() as i32;
    let pad_x = BORDER_WIDTH + blur + SHADOW_SPREAD + SHADOW_OFFSET.0.abs();
    let pad_y = BORDER_WIDTH + blur + SHADOW_SPREAD + SHADOW_OFFSET.1.abs();
    let scale = Scale::from(SCALE);

    let tiled = windows.iter().filter(|w| !w.floating).count();

    let mut elems = Vec::new();

    // layer surface popups (above everything)
    elems.extend(layer_popup_elements(renderer, output));

    // overlay + top layers (above windows)
    elems.extend(layer_elements(
        renderer,
        output,
        &[Layer::Overlay, Layer::Top],
    ));

    for we in windows.iter().rev() {
        let win = we.geo();
        let buf = we.window.geometry();
        let wl = we.window.wl_surface().unwrap();
        let single_no_border = !SINGLE_BORDER && tiled == 1 && !we.floating;

        let surfs = render_elements_from_surface_tree(
            renderer,
            &wl,
            (win.loc - buf.loc).to_physical_precise_round(SCALE),
            scale,
            1.0,
            Kind::Unspecified,
        );

        // popups (unclipped, on top of window)
        elems.extend(popup_elements(renderer, &wl, win.loc - buf.loc));

        #[rustfmt::skip]
        let (color, radius, bw) = match (we.floating, we.focused) {
            (true,  true)  => (FOCUS_COLOR,  FLOATING_RADIUS, BORDER_WIDTH),
            (true,  false) => (BORDER_COLOR, FLOATING_RADIUS, 0),
            (false, true)  => (FOCUS_COLOR,  TILED_RADIUS,    BORDER_WIDTH),
            (false, false) => (BORDER_COLOR, TILED_RADIUS,    BORDER_WIDTH),
        };

        // surfaces
        for s in surfs {
            if single_no_border || !ClippedSurface::will_clip(&s, win, radius, scale) {
                elems.push(MonotileElement::Surface(s));
            } else {
                elems.push(MonotileElement::Clipped(ClippedSurface::new(
                    s,
                    shaders.clip.clone(),
                    win,
                    radius,
                    scale,
                )));
            }
        }

        // border (8 pieces)
        if bw > 0 && !single_no_border {
            for piece in border::elements(&shaders.rect, win, radius, bw, color, SCALE as f32) {
                elems.push(MonotileElement::Decoration(piece));
            }
        }

        // background
        let bg = PixelShaderElement::new(
            shaders.rect.clone(),
            win,
            None,
            1.0,
            vec![
                Uniform::new("outer_size", (win.size.w as f32, win.size.h as f32)),
                Uniform::new("border_width", 0.0f32),
                Uniform::new("outer_radius", radius),
                Uniform::new("border_color", ROOT_COLOR),
                Uniform::new("piece_offset", (0.0f32, 0.0f32)),
                Uniform::new("scale", SCALE as f32),
            ],
            Kind::Unspecified,
        );
        elems.push(MonotileElement::Decoration(bg));

        // shadow (floating only)
        if we.floating {
            let outer_r = radius + bw as f32;

            let shadow = PixelShaderElement::new(
                shaders.shadow.clone(),
                Rectangle::new(
                    (win.loc.x - pad_x, win.loc.y - pad_y).into(),
                    (win.size.w + 2 * pad_x, win.size.h + 2 * pad_y).into(),
                ),
                None,
                1.0,
                vec![
                    Uniform::new("win_size", (win.size.w as f32, win.size.h as f32)),
                    Uniform::new("win_offset", (pad_x as f32, pad_y as f32)),
                    Uniform::new("outer_radius", outer_r),
                    Uniform::new(
                        "shadow_box_size",
                        (
                            (win.size.w + 2 * SHADOW_SPREAD) as f32,
                            (win.size.h + 2 * SHADOW_SPREAD) as f32,
                        ),
                    ),
                    Uniform::new(
                        "shadow_box_offset",
                        (
                            (pad_x - SHADOW_SPREAD + SHADOW_OFFSET.0) as f32,
                            (pad_y - SHADOW_SPREAD + SHADOW_OFFSET.1) as f32,
                        ),
                    ),
                    Uniform::new("shadow_sigma", sigma),
                    Uniform::new("shadow_color", SHADOW_COLOR),
                    Uniform::new("scale", SCALE as f32),
                ],
                Kind::Unspecified,
            );
            elems.push(MonotileElement::Decoration(shadow));
        }
    }

    // bottom + background layers (below windows)
    elems.extend(layer_elements(
        renderer,
        output,
        &[Layer::Bottom, Layer::Background],
    ));

    tracker.render_output(renderer, target, age, &elems, BG_COLOR)
}
