// SPDX-License-Identifier: GPL-3.0-only

mod border;
pub mod clipped_surface;
pub mod cursor;
mod shaders;

use std::borrow::BorrowMut;
use std::time::Duration;

use smithay::{
    backend::renderer::{
        element::{
            Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{
            GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName, UniformType,
            element::PixelShaderElement,
        },
        glow::GlowRenderer,
    },
    desktop::{PopupManager, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Scale},
    wayland::{seat::WaylandFocus, shell::wlr_layer::Layer},
};

use crate::{
    config::{Config, RenderStep},
    shell::{Monitor, WindowElement, Windows},
};
use clipped_surface::ClippedSurface;

render_elements! {
    pub MonotileElement<=GlowRenderer>;
    Surface=WaylandSurfaceRenderElement<GlowRenderer>,
    Clipped=ClippedSurface,
    Decoration=PixelShaderElement,
    Memory=MemoryRenderBufferRenderElement<GlowRenderer>,
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
    scale: Scale<f64>,
) -> Vec<MonotileElement> {
    let map = layer_map_for_output(output);
    let mut elems = Vec::new();
    for layer in layers {
        for surface in map.layers_on(*layer).rev() {
            let geo = map.layer_geometry(surface).unwrap();
            let surfs = render_elements_from_surface_tree(
                renderer,
                surface.wl_surface(),
                geo.loc.to_physical_precise_round(scale),
                scale,
                1.0,
                Kind::Unspecified,
            );
            elems.extend(surfs.into_iter().map(MonotileElement::Surface));
        }
    }
    elems
}

fn layer_popup_elements(
    renderer: &mut GlowRenderer,
    output: &Output,
    layers: &[Layer],
    scale: Scale<f64>,
) -> Vec<MonotileElement> {
    let map = layer_map_for_output(output);
    let mut elems = Vec::new();
    for layer in layers {
        for surface in map.layers_on(*layer).rev() {
            let geo = map.layer_geometry(surface).unwrap();
            elems.extend(popup_elements(
                renderer,
                surface.wl_surface(),
                geo.loc,
                scale,
            ));
        }
    }
    elems
}

fn popup_elements(
    renderer: &mut GlowRenderer,
    surface: &WlSurface,
    origin: Point<i32, Logical>,
    scale: Scale<f64>,
) -> Vec<MonotileElement> {
    let mut elems = Vec::new();
    for (popup, offset) in PopupManager::popups_for_surface(surface) {
        let pos = origin + offset - popup.geometry().loc;
        let surfs = render_elements_from_surface_tree(
            renderer,
            popup.wl_surface(),
            pos.to_physical_precise_round(scale),
            scale,
            1.0,
            Kind::Unspecified,
        );
        elems.extend(surfs.into_iter().map(MonotileElement::Surface));
    }
    elems
}

pub fn output_elements(
    renderer: &mut GlowRenderer,
    mon: &Monitor,
    ws: &Windows,
    shaders: &Shaders,
    config: &Config,
) -> Vec<MonotileElement> {
    let output = &mon.output;
    let out_scale = output.current_scale().fractional_scale();
    let scale = Scale::from(out_scale);
    let scale_f32 = out_scale as f32;
    let mut elems = Vec::new();

    if let Some(we) = mon.tag().fullscreen.and_then(|id| ws.get(id)) {
        elems.extend(layer_popup_elements(
            renderer,
            output,
            &[Layer::Overlay],
            scale,
        ));
        elems.extend(layer_elements(renderer, output, &[Layer::Overlay], scale));

        let win = we.geo();
        let buf = we.window.geometry();
        let wl = we.window.wl_surface().unwrap();
        elems.extend(popup_elements(renderer, &wl, win.loc - buf.loc, scale));

        let surfs = render_elements_from_surface_tree(
            renderer,
            &wl,
            (win.loc - buf.loc).to_physical_precise_round(scale),
            scale,
            1.0,
            Kind::ScanoutCandidate,
        );
        elems.extend(surfs.into_iter().map(MonotileElement::Surface));
    } else {
        let all = &[Layer::Overlay, Layer::Top, Layer::Bottom, Layer::Background];
        elems.extend(layer_popup_elements(renderer, output, all, scale));
        elems.extend(layer_elements(
            renderer,
            output,
            &[Layer::Overlay, Layer::Top],
            scale,
        ));

        let windows = ws.visible(mon.tag());
        let tiled = windows.iter().filter(|w| !w.floating).count();

        for we in windows.iter().rev() {
            let win = we.geo();
            let buf = we.window.geometry();
            let wl = we.window.wl_surface().unwrap();

            let single_tiled = tiled == 1 && !we.floating;
            let disable_gaps = config.layout.smart_gaps && single_tiled;
            let disable_border = config.layout.smart_borders && single_tiled;

            let radius = we.render.iter().find_map(|s| match s {
                RenderStep::WindowSurface { radius, .. } => Some(*radius),
                _ => None,
            }).unwrap_or(0.0);

            // rev: render pipeline is back-to-front
            for step in we.render.iter().rev() {
                match step {
                    RenderStep::FocusRing { width, color } if we.focused && !disable_border && *width > 0 => {
                        for piece in border::elements(&shaders.rect, win, radius, *width, color.0, scale_f32) {
                            elems.push(MonotileElement::Decoration(piece));
                        }
                    }
                    RenderStep::Border { width, color } if !disable_border && *width > 0 => {
                        for piece in border::elements(&shaders.rect, win, radius, *width, color.0, scale_f32) {
                            elems.push(MonotileElement::Decoration(piece));
                        }
                    }
                    RenderStep::WindowSurface { fill, .. } => {
                        let clip_r = if disable_gaps { 0.0 } else { radius };
                        // popups on top of surface
                        elems.extend(popup_elements(renderer, &wl, win.loc - buf.loc, scale));

                        // surfaces (clipped if radius > 0)
                        let surfs = render_elements_from_surface_tree(
                            renderer,
                            &wl,
                            (win.loc - buf.loc).to_physical_precise_round(scale),
                            scale,
                            1.0,
                            Kind::Unspecified,
                        );
                        for s in surfs {
                            if !ClippedSurface::will_clip(&s, win, clip_r, scale) {
                                elems.push(MonotileElement::Surface(s));
                            } else {
                                elems.push(MonotileElement::Clipped(ClippedSurface::new(
                                    s, shaders.clip.clone(), win, clip_r, scale,
                                )));
                            }
                        }

                        // background fill
                        elems.push(MonotileElement::Decoration(PixelShaderElement::new(
                            shaders.rect.clone(),
                            win,
                            None,
                            1.0,
                            vec![
                                Uniform::new("outer_size", (win.size.w as f32, win.size.h as f32)),
                                Uniform::new("border_width", 0.0f32),
                                Uniform::new("outer_radius", clip_r),
                                Uniform::new("border_color", fill.0),
                                Uniform::new("piece_offset", (0.0f32, 0.0f32)),
                                Uniform::new("scale", scale_f32),
                            ],
                            Kind::Unspecified,
                        )));
                    }
                    RenderStep::Shadow { softness, spread, offset, color } if !disable_gaps => {
                        let sigma = *softness as f32 / 2.0;
                        let blur = (sigma * 3.0).ceil() as i32;
                        let pad_x = blur + spread + offset.0.abs();
                        let pad_y = blur + spread + offset.1.abs();

                        elems.push(MonotileElement::Decoration(PixelShaderElement::new(
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
                                Uniform::new("outer_radius", radius),
                                Uniform::new("shadow_box_size", (
                                    (win.size.w + 2 * spread) as f32,
                                    (win.size.h + 2 * spread) as f32,
                                )),
                                Uniform::new("shadow_box_offset", (
                                    (pad_x - spread + offset.0) as f32,
                                    (pad_y - spread + offset.1) as f32,
                                )),
                                Uniform::new("shadow_sigma", sigma),
                                Uniform::new("shadow_color", color.0),
                                Uniform::new("scale", scale_f32),
                            ],
                            Kind::Unspecified,
                        )));
                    }
                    _ => {}
                }
            }
        }

        elems.extend(layer_elements(
            renderer,
            output,
            &[Layer::Bottom, Layer::Background],
            scale,
        ));
    }

    elems
}

pub fn send_frame_callbacks<'a>(
    windows: impl IntoIterator<Item = &'a WindowElement>,
    output: &Output,
    elapsed: Duration,
    popups: &mut PopupManager,
) {
    // TODO: use predicted frame timing instead of ZERO
    let time = Some(Duration::ZERO);
    for we in windows {
        we.window
            .send_frame(output, elapsed, time, |_, _| Some(output.clone()));
    }
    let mut map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.send_frame(output, elapsed, time, |_, _| Some(output.clone()));
    }
    popups.cleanup();
    map.cleanup();
}
