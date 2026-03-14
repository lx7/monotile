// SPDX-License-Identifier: GPL-3.0-only

use derive_more::{Deref, DerefMut};
use smithay::{
    desktop::layer_map_for_output,
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::{
        session_lock::LockSurface,
        shell::wlr_layer::{KeyboardInteractivity, Layer},
    },
};

use crate::config::{self, Config};

use super::{Tag, WindowId, Windows};

#[derive(Debug)]
pub struct Monitor {
    pub output: Output,
    pub tag_names: Vec<String>,
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub prev_tag: usize,
    pub background: [f32; 4],
    pub exclusive_layer: Option<WlSurface>,
    pub lock_surface: Option<LockSurface>,
}

impl Monitor {
    pub fn new(output: Output, config: &Config) -> Self {
        let mut mon = Self {
            output,
            tag_names: Vec::new(),
            tags: Vec::new(),
            active_tag: 0,
            exclusive_layer: None,
            lock_surface: None,
            prev_tag: 0,
            background: [0.0, 0.0, 0.0, 1.0],
        };
        mon.resolve(&config.outputs);
        mon
    }

    pub fn resolve(&mut self, rules: &[config::OutputRule]) {
        let name = self.output.name();
        let props = self.output.physical_properties();
        let mut tags = None;
        let mut bg = None;
        for rule in rules {
            if rule
                .r#match
                .matches(&name, &props.make, &props.model, &props.serial_number)
            {
                if let Some(t) = &rule.tags {
                    tags = Some(t.clone());
                }
                if let Some(c) = rule.background {
                    bg = Some(c);
                }
            }
        }
        let tag_names = tags.unwrap_or_else(config::default_tags);
        self.tags.resize_with(tag_names.len(), Tag::default);
        self.tag_names = tag_names;
        self.background = bg.map_or([0.0, 0.0, 0.0, 1.0], |c| c.0);
        // TODO: use scale, pos, mode, transform from config
    }

    pub fn tag(&self) -> &Tag {
        &self.tags[self.active_tag]
    }

    pub fn tag_mut(&mut self) -> &mut Tag {
        &mut self.tags[self.active_tag]
    }

    pub fn map(&mut self, ws: &mut Windows, id: WindowId, tags: Option<Vec<usize>>) {
        let area = layer_map_for_output(&self.output).non_exclusive_zone();
        let we = &mut ws[id];

        let fw = if we.float_geo.size.w > 0 {
            we.float_geo.size.w
        } else {
            area.size.w * 3 / 4
        };
        let fh = if we.float_geo.size.h > 0 {
            we.float_geo.size.h
        } else {
            area.size.h * 3 / 4
        };

        let has_pos = we.float_geo.loc != Point::default();
        let x = if has_pos {
            we.float_geo.loc.x
        } else {
            area.loc.x + (area.size.w - fw) / 2
        };
        let y = if has_pos {
            we.float_geo.loc.y
        } else {
            area.loc.y + (area.size.h - fh) / 2
        };
        we.float_geo = Rectangle::new((x, y).into(), (fw, fh).into());

        if let Some(tags) = tags {
            for t in tags {
                if t < self.tags.len() {
                    self.tags[t].add(id);
                }
            }
        } else {
            self.tag_mut().add(id);
        }
    }

    pub fn unmap(&mut self, ws: &mut Windows, id: WindowId) {
        for tag in &mut self.tags {
            tag.remove(id);
        }
        ws.remove(id);
    }

    pub fn move_to_tag(&mut self, ws: &mut Windows, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        let Some(id) = self.tag().focused_id() else {
            return;
        };
        if let Some(we) = ws.get_mut(id) {
            we.set_fullscreen(None);
        }
        for t in &mut self.tags {
            t.remove(id);
        }
        self.tags[tag].add(id);
    }

    pub fn toggle_tag(&mut self, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        let Some(id) = self.tag().focused_id() else {
            return;
        };
        if self.tags[tag].contains(id) {
            let count = self.tags.iter().filter(|t| t.contains(id)).count();
            if count > 1 {
                self.tags[tag].remove(id);
            }
        } else {
            self.tags[tag].add(id);
        }
    }

    pub fn set_active_tag(&mut self, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        self.prev_tag = self.active_tag;
        self.active_tag = tag;
    }

    pub fn toggle_prev_tag(&mut self) {
        std::mem::swap(&mut self.active_tag, &mut self.prev_tag);
    }

    pub fn output_geometry(&self) -> Rectangle<i32, Logical> {
        let size = self.output.current_mode().unwrap().size;
        Rectangle::new((0, 0).into(), size.to_logical(1))
    }

    pub fn recompute_layout(&mut self, ws: &mut Windows, config: &Config) {
        let tag = &mut self.tags[self.active_tag];

        tag.tiled
            .retain(|&id| ws.get(id).is_some_and(|we| !we.floating));

        tag.floating
            .retain(|&id| ws.get(id).is_some_and(|we| we.floating));

        for &id in &tag.focus_stack {
            let Some(we) = ws.get(id) else { continue };
            if we.floating {
                if !tag.floating.contains(&id) {
                    tag.floating.push(id);
                }
            } else if !tag.tiled.contains(&id) {
                tag.tiled.push(id);
            }
        }
        tag.fullscreen = tag
            .focus_stack
            .iter()
            .copied()
            .find(|&id| ws.get(id).is_some_and(|we| we.fullscreen));

        let geo = layer_map_for_output(&self.output).non_exclusive_zone();
        let rects = tag
            .layout
            .compute_rects(tag.tiled.len(), geo, &config.layout);
        for (&id, rect) in tag.tiled.iter().zip(rects) {
            if let Some(we) = ws.get_mut(id) {
                we.tiled_geo = rect;
            }
        }
    }

    pub fn update_exclusive_layer(&mut self) {
        let map = layer_map_for_output(&self.output);
        self.exclusive_layer = None;
        for l in [Layer::Overlay, Layer::Top] {
            for s in map.layers_on(l).rev() {
                if s.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive {
                    self.exclusive_layer = Some(s.wl_surface().clone());
                    return;
                }
            }
        }
    }
}

#[derive(Debug, Default, Deref, DerefMut)]
pub struct Monitors(pub Vec<Monitor>);

impl Monitors {
    pub fn by_output(&self, output: &Output) -> Option<(usize, &Monitor)> {
        self.iter().enumerate().find(|(_, m)| m.output == *output)
    }

    pub fn update_rules(&mut self, rules: &[config::OutputRule]) {
        for mon in self.iter_mut() {
            mon.resolve(rules);
        }
    }
}
