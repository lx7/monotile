// SPDX-License-Identifier: GPL-3.0-only

mod border;
pub mod clipped_surface;
pub mod cursor;
mod shaders;
pub mod window;

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
            GlesPixelProgram, GlesRenderer, GlesTexProgram, UniformName, UniformType,
            element::PixelShaderElement,
        },
        glow::GlowRenderer,
    },
    desktop::{PopupManager, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Scale},
    wayland::{seat::WaylandFocus, shell::wlr_layer::Layer},
};

pub use window::RenderStep;

use crate::{
    config::Config,
    shell::{Monitor, Tag, Windows},
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
    windows: &mut Windows,
    shaders: &Shaders,
    config: &Config,
    locked: bool,
) -> Vec<MonotileElement> {
    let output = &mon.output;
    let out_scale = output.current_scale().fractional_scale();
    let scale = Scale::from(out_scale);

    if let Some(lock) = &mon.lock_surface {
        let surfs = render_elements_from_surface_tree(
            renderer,
            lock.wl_surface(),
            (0, 0),
            scale,
            1.0,
            Kind::Unspecified,
        );
        return surfs.into_iter().map(MonotileElement::Surface).collect();
    }
    if locked {
        return vec![];
    }

    let n = mon.tag().tiled.len() + mon.tag().floating.len();
    let mut elems = Vec::with_capacity(n * 20 + 32);

    if let Some(we) = mon.tag().fullscreen.and_then(|id| windows.get(id)) {
        elems.extend(layer_popup_elements(
            renderer,
            output,
            &[Layer::Overlay],
            scale,
        ));
        elems.extend(layer_elements(renderer, output, &[Layer::Overlay], scale));

        let wl = we.window.wl_surface().unwrap();
        elems.extend(popup_elements(renderer, &wl, we.geo().loc, scale));

        let surfs = render_elements_from_surface_tree(
            renderer,
            &wl,
            we.surface_loc().to_physical_precise_round(scale),
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

        let tag = mon.tag();
        let tiled = tag.tiled.len();

        let ids: Vec<_> = tag.window_ids().rev().collect();
        for id in ids {
            let Some(we) = windows.get_mut(id) else {
                continue;
            };
            let single_tiled = tiled == 1 && !we.floating;
            we.render_elements(
                &mut elems,
                renderer,
                shaders,
                scale,
                config.layout.smart_borders && single_tiled,
                config.layout.smart_gaps && single_tiled,
            );
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

pub fn send_frame_callbacks(
    windows: &mut Windows,
    tag: &Tag,
    output: &Output,
    elapsed: Duration,
    throttle: Option<Duration>,
    popups: &mut PopupManager,
) {
    for id in tag.window_ids() {
        if let Some(we) = windows.get_mut(id)
            && we.committed
        {
            we.committed = false;
            we.window
                .send_frame(output, elapsed, throttle, |_, _| Some(output.clone()));
        }
    }
    let mut map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.send_frame(output, elapsed, throttle, |_, _| Some(output.clone()));
    }
    popups.cleanup();
    map.cleanup();
}
