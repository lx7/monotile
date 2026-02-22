// SPDX-License-Identifier: GPL-3.0-only

pub mod clipped_surface;
mod shaders;

use std::borrow::BorrowMut;

use smithay::{
    backend::renderer::{
        RendererSuper,
        damage::{OutputDamageTracker, RenderOutputResult},
        element::{
            Id, Kind, render_elements,
            solid::SolidColorRenderElement,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{
            GlesPixelProgram, GlesRenderer, GlesTarget, GlesTexProgram, Uniform, UniformName,
            UniformType, element::PixelShaderElement,
        },
        glow::GlowRenderer,
        utils::CommitCounter,
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
    SolidColor=SolidColorRenderElement,
}

pub fn compile_shaders(renderer: &mut GlowRenderer) -> (GlesPixelProgram, GlesTexProgram) {
    let gles: &mut GlesRenderer = renderer.borrow_mut();

    // TODO: optimize shader structure
    let deco = gles
        .compile_custom_pixel_shader(
            shaders::DECORATION_FRAG,
            &[
                UniformName::new("border_width", UniformType::_1f),
                UniformName::new("border_color", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("shadow_sigma", UniformType::_1f),
                UniformName::new("shadow_color", UniformType::_4f),
                UniformName::new("bg_color", UniformType::_4f),
                UniformName::new("shadow_box_size", UniformType::_2f),
                UniformName::new("shadow_box_offset", UniformType::_2f),
                UniformName::new("win_size", UniformType::_2f),
                UniformName::new("win_offset", UniformType::_2f),
            ],
        )
        .expect("decoration shader");
    let clip = gles
        .compile_custom_texture_shader(
            shaders::CLIPPED_SURFACE_FRAG,
            &[
                UniformName::new("geo_size", UniformType::_2f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("input_to_geo", UniformType::Matrix3x3),
            ],
        )
        .expect("clip shader");
    (deco, clip)
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
    deco_shader: &GlesPixelProgram,
    clip_shader: &GlesTexProgram,
) -> RenderResult<'a> {
    let sigma = SHADOW_SOFTNESS / 2.0;
    let blur = (sigma * 3.0).ceil();
    let pad_x = (BORDER_WIDTH as f32 + blur + SHADOW_SPREAD + SHADOW_OFFSET.0.abs()).ceil() as i32;
    let pad_y = (BORDER_WIDTH as f32 + blur + SHADOW_SPREAD + SHADOW_OFFSET.1.abs()).ceil() as i32;
    let scale = Scale::from(SCALE);

    let tiled = windows.iter().filter(|w| !w.floating).count();
    let solo = !SINGLE_BORDER && tiled == 1;

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

        // solo tiled: no decoration, just bg
        if solo && !we.floating {
            elems.extend(surfs.into_iter().map(MonotileElement::Surface));
            elems.push(MonotileElement::SolidColor(SolidColorRenderElement::new(
                Id::new(),
                win.to_physical_precise_round(scale),
                CommitCounter::default(),
                ROOT_COLOR,
                Kind::Unspecified,
            )));
            continue;
        }

        let border_color = if we.focused {
            FOCUS_COLOR
        } else {
            BORDER_COLOR
        };
        let radius = if we.floating {
            FLOATING_RADIUS
        } else {
            TILED_RADIUS
        };
        let border_width = if we.floating && !we.focused {
            0.0
        } else {
            BORDER_WIDTH as f32
        };
        let shadow_sigma = if we.floating { sigma } else { 0.0 };
        let win_w = win.size.w as f32;
        let win_h = win.size.h as f32;
        let pad_xf = pad_x as f32;
        let pad_yf = pad_y as f32;

        let deco = PixelShaderElement::new(
            deco_shader.clone(),
            Rectangle::new(
                (win.loc.x - pad_x, win.loc.y - pad_y).into(),
                (win.size.w + 2 * pad_x, win.size.h + 2 * pad_y).into(),
            ),
            None,
            1.0,
            vec![
                Uniform::new("border_width", border_width),
                Uniform::new("border_color", border_color),
                Uniform::new("radius", radius),
                Uniform::new("shadow_sigma", shadow_sigma),
                Uniform::new("shadow_color", SHADOW_COLOR),
                Uniform::new("bg_color", ROOT_COLOR),
                Uniform::new(
                    "shadow_box_size",
                    (win_w + 2.0 * SHADOW_SPREAD, win_h + 2.0 * SHADOW_SPREAD),
                ),
                Uniform::new(
                    "shadow_box_offset",
                    (
                        pad_xf - SHADOW_SPREAD + SHADOW_OFFSET.0,
                        pad_yf - SHADOW_SPREAD + SHADOW_OFFSET.1,
                    ),
                ),
                Uniform::new("win_size", (win_w, win_h)),
                Uniform::new("win_offset", (pad_xf, pad_yf)),
            ],
            Kind::Unspecified,
        );

        elems.extend(surfs.into_iter().map(|s| {
            MonotileElement::Clipped(ClippedSurface::new(
                s,
                clip_shader.clone(),
                win,
                radius,
                scale,
            ))
        }));
        elems.push(MonotileElement::Decoration(deco));
    }

    // bottom + background layers (below windows)
    elems.extend(layer_elements(
        renderer,
        output,
        &[Layer::Bottom, Layer::Background],
    ));

    tracker.render_output(renderer, target, age, &elems, BG_COLOR)
}
