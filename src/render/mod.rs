// SPDX-License-Identifier: GPL-3.0-only

mod border;
pub mod clipped_surface;
pub mod cursor;
mod shaders;
pub mod window;

use std::borrow::BorrowMut;
use std::time::Duration;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, BufferType, Color32F, ExportMem, Offscreen, buffer_type,
            damage::OutputDamageTracker,
            element::{
                Kind, RenderElement,
                memory::MemoryRenderBufferRenderElement,
                render_elements,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
                texture::TextureRenderElement,
            },
            gles::{
                GlesPixelProgram, GlesRenderer, GlesTexProgram, GlesTexture, UniformName,
                UniformType, element::PixelShaderElement,
            },
            glow::GlowRenderer,
        },
    },
    desktop::{PopupManager, layer_map_for_output, utils::send_frames_surface_tree},
    output::Output,
    reexports::wayland_server::protocol::{wl_buffer, wl_surface::WlSurface},
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
    wayland::{
        dmabuf::get_dmabuf, seat::WaylandFocus, shell::wlr_layer::Layer,
        shm::with_buffer_contents_mut,
    },
};

pub use window::RenderStep;

use crate::{
    config::{Config, Layout},
    shell::{Monitor, Windows},
};
use clipped_surface::Clipped;

render_elements! {
    pub MonotileElement<=GlowRenderer>;
    Surface=WaylandSurfaceRenderElement<GlowRenderer>,
    Texture=TextureRenderElement<GlesTexture>,
    Clipped=Clipped,
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

pub struct RenderCtx<'a> {
    pub(crate) renderer: &'a mut GlowRenderer,
    pub(crate) shaders: &'a Shaders,
    pub(crate) layout: &'a Layout,
    pub(crate) scale: Scale<f64>,
    pub(crate) elems: Vec<MonotileElement>,
    output: &'a Output,
}

impl RenderCtx<'_> {
    fn layers(&mut self, layers: &[Layer]) {
        let map = layer_map_for_output(self.output);
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let geo = map.layer_geometry(surface).unwrap();
                let surfs = render_elements_from_surface_tree(
                    self.renderer,
                    surface.wl_surface(),
                    geo.loc.to_physical_precise_round(self.scale),
                    self.scale,
                    1.0,
                    Kind::Unspecified,
                );
                self.elems
                    .extend(surfs.into_iter().map(MonotileElement::Surface));
            }
        }
    }

    fn layer_popups(&mut self, layers: &[Layer]) {
        let map = layer_map_for_output(self.output);
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let origin = map.layer_geometry(surface).unwrap().loc;
                let popups =
                    popup_elements(self.renderer, surface.wl_surface(), origin, self.scale);
                self.elems.extend(popups);
            }
        }
    }

    pub(crate) fn popups(&mut self, surface: &WlSurface, origin: Point<i32, Logical>) {
        let popups = popup_elements(self.renderer, surface, origin, self.scale);
        self.elems.extend(popups);
    }

    fn surface_tree(&mut self, surface: &WlSurface, loc: Point<i32, Logical>, kind: Kind) {
        let surfs = render_elements_from_surface_tree(
            self.renderer,
            surface,
            loc.to_physical_precise_round(self.scale),
            self.scale,
            1.0,
            kind,
        );
        self.elems
            .extend(surfs.into_iter().map(MonotileElement::Surface));
    }
}

pub fn popup_elements(
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
    mon: &mut Monitor,
    windows: &mut Windows,
    shaders: &Shaders,
    config: &Config,
    locked: bool,
) -> Vec<MonotileElement> {
    let output = &mon.output;
    let out_scale = output.current_scale().fractional_scale();
    let scale = Scale::from(out_scale);

    let mut ctx = RenderCtx {
        renderer,
        shaders,
        layout: &config.layout,
        scale,
        elems: Vec::with_capacity(windows.len() * 20 + 32),
        output,
    };

    if let Some(lock) = &mon.lock_surface {
        ctx.surface_tree(lock.wl_surface(), (0, 0).into(), Kind::Unspecified);
        return ctx.elems;
    }
    if locked {
        return ctx.elems;
    }

    let view = mon.views.front();

    if let Some(we) = view
        .and_then(|v| v.fullscreen)
        .and_then(|id| windows.get(id))
    {
        let geo = mon.geometry();
        ctx.layer_popups(&[Layer::Overlay]);
        ctx.layers(&[Layer::Overlay]);

        let wl = we.window.wl_surface().unwrap();
        ctx.popups(&wl, geo.loc);
        ctx.surface_tree(&wl, we.surface_loc(geo), Kind::ScanoutCandidate);
    } else {
        ctx.layer_popups(&[Layer::Overlay, Layer::Top, Layer::Bottom, Layer::Background]);
        ctx.layers(&[Layer::Overlay, Layer::Top]);

        if let Some(view) = view {
            view.render_elements(&mut ctx, windows);
        }

        ctx.layers(&[Layer::Bottom, Layer::Background]);
    }

    ctx.elems
}

pub fn send_frame_callbacks(
    windows: &mut Windows,
    mon: &Monitor,
    output: &Output,
    elapsed: Duration,
    throttle: Option<Duration>,
    popups: &mut PopupManager,
) {
    if let Some(ls) = &mon.lock_surface {
        send_frames_surface_tree(ls.wl_surface(), output, elapsed, None, |_, _| {
            Some(output.clone())
        });
    }
    for id in mon.tag().window_ids() {
        if let Some(we) = windows.get_mut(id)
            && we.buffer_committed
        {
            we.buffer_committed = false;
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

pub fn render_to_buffer<E: RenderElement<GlowRenderer>>(
    renderer: &mut GlowRenderer,
    tracker: &mut OutputDamageTracker,
    buf: &wl_buffer::WlBuffer,
    elems: &[E],
    background: impl Into<Color32F> + Copy,
    transform: Transform,
    buffer_size: Size<i32, BufferCoords>,
) -> anyhow::Result<Option<Vec<Rectangle<i32, BufferCoords>>>> {
    match buffer_type(buf) {
        Some(BufferType::Shm) => {
            let mut tex: GlesTexture = renderer.create_buffer(Fourcc::Argb8888, buffer_size)?;
            let mut fb = renderer.bind(&mut tex)?;
            let result = tracker.render_output(renderer, &mut fb, 0, elems, background)?;
            let damage = damage_to_buffer(result.damage, transform, buffer_size);
            let mapping = renderer.copy_framebuffer(
                &fb,
                Rectangle::from_size(buffer_size),
                Fourcc::Argb8888,
            )?;
            let pixels = renderer.map_texture(&mapping)?;
            let (src_w, src_h) = (buffer_size.w as usize, buffer_size.h as usize);
            let src_stride = src_w * 4;
            with_buffer_contents_mut(buf, |ptr, len, data| {
                let offset = data.offset as usize;
                let dst_stride = data.stride as usize;
                let row = src_stride.min(dst_stride);
                let end = offset + src_h.saturating_sub(1) * dst_stride + row;
                if end > len {
                    return;
                }
                for line in 0..src_h {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            pixels[line * src_stride..].as_ptr(),
                            ptr.add(offset + line * dst_stride),
                            row,
                        );
                    }
                }
            })?;
            Ok(damage)
        }
        Some(BufferType::Dma) => {
            let mut dmabuf = get_dmabuf(buf)?.clone();
            let mut fb = renderer.bind(&mut dmabuf)?;
            let result = tracker.render_output(renderer, &mut fb, 0, elems, background)?;
            Ok(damage_to_buffer(result.damage, transform, buffer_size))
        }
        _ => anyhow::bail!("unsupported buffer type"),
    }
}

fn damage_to_buffer(
    damage: Option<&Vec<Rectangle<i32, Physical>>>,
    transform: Transform,
    size: Size<i32, BufferCoords>,
) -> Option<Vec<Rectangle<i32, BufferCoords>>> {
    let logical_size = size.to_logical(1, transform);
    let inv = transform.invert();
    damage.map(|rects| {
        rects
            .iter()
            .map(|r| r.to_logical(1).to_buffer(1, inv, &logical_size))
            .collect()
    })
}
