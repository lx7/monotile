// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::render_elements_from_surface_tree,
            },
            glow::GlowRenderer,
        },
    },
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    reexports::wayland_server::Resource,
    utils::{Logical, Point, Scale, Transform},
    wayland::compositor,
};

use tracing::warn;
use xcursor::{CursorTheme, parser::parse_xcursor};

use super::MonotileElement;

static FALLBACK_CURSOR_DATA: &[u8] = include_bytes!("../../resources/cursor.rgba");

struct Cursor {
    buffer: MemoryRenderBuffer,
    hotspot: Point<i32, Logical>,
}

pub struct CursorManager {
    pub status: CursorImageStatus,
    pub scale: f32,
    theme: CursorTheme,
    size: u32,
    cache: HashMap<String, Cursor>,
}

impl CursorManager {
    pub fn new(scale: f32) -> Self {
        let name = std::env::var("XCURSOR_THEME").unwrap_or("default".into());
        let size = std::env::var("XCURSOR_SIZE").unwrap_or("24".into());
        let size = size.parse().unwrap_or(24);

        let theme = CursorTheme::load(&name);
        let mut cursor_manager = Self {
            status: CursorImageStatus::default_named(),
            scale,
            theme,
            size,
            cache: HashMap::new(),
        };

        if !cursor_manager.load_icon(CursorIcon::Default) {
            warn!("failed to load xcursor theme, using fallback");
            let buffer = MemoryRenderBuffer::from_slice(
                FALLBACK_CURSOR_DATA,
                Fourcc::Argb8888,
                (64, 64),
                1,
                Transform::Normal,
                None,
            );
            cursor_manager.cache.insert(
                "default".to_string(),
                Cursor {
                    buffer,
                    hotspot: (1, 1).into(),
                },
            );
        }
        cursor_manager
    }

    fn load_icon(&mut self, icon: CursorIcon) -> bool {
        let name = icon.name();
        if self.cache.contains_key(name) {
            return true;
        }
        if let Some(cached) = load_xcursor(&self.theme, name, self.size) {
            self.cache.insert(name.to_string(), cached);
            return true;
        }
        false
    }

    fn get_icon(&mut self, icon: CursorIcon) -> &Cursor {
        if !self.load_icon(icon) {
            return &self.cache["default"];
        }
        &self.cache[icon.name()]
    }

    pub fn elements(
        &mut self,
        renderer: &mut GlowRenderer,
        pos: Point<f64, Logical>,
    ) -> Vec<MonotileElement> {
        if let CursorImageStatus::Surface(s) = &self.status
            && !s.is_alive()
        {
            self.status = CursorImageStatus::default_named();
        }
        let scale = Scale::from(self.scale as f64);
        match &self.status {
            CursorImageStatus::Hidden => vec![],
            CursorImageStatus::Named(icon) => {
                let cached = self.get_icon(*icon);
                let loc = (pos - cached.hotspot.to_f64()).to_physical_precise_round(scale);
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    loc,
                    &cached.buffer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    Ok(elem) => vec![MonotileElement::Memory(elem)],
                    Err(_) => vec![],
                }
            }
            CursorImageStatus::Surface(surface) => {
                let hotspot = compositor::with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .map(|d| d.lock().unwrap().hotspot)
                        .unwrap_or_default()
                });
                let loc = (pos - hotspot.to_f64()).to_physical_precise_round(scale);
                render_elements_from_surface_tree(renderer, surface, loc, scale, 1.0, Kind::Cursor)
            }
        }
    }
}

fn load_xcursor(theme: &CursorTheme, name: &str, size: u32) -> Option<Cursor> {
    let path = theme.load_icon(name)?;
    let data = std::fs::read(path).ok()?;

    let images = parse_xcursor(&data)?;
    let nearest = images
        .iter()
        .min_by_key(|img| (size as i32 - img.size as i32).abs())?;
    let (w, h) = (nearest.width, nearest.height);
    let img = images
        .into_iter()
        .find(|img| img.width == w && img.height == h)?;

    let buffer = MemoryRenderBuffer::from_slice(
        &img.pixels_rgba,
        Fourcc::Argb8888,
        (w as i32, h as i32),
        1,
        Transform::Normal,
        None,
    );
    Some(Cursor {
        buffer,
        hotspot: (img.xhot as i32, img.yhot as i32).into(),
    })
}
