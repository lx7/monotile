// SPDX-License-Identifier: GPL-3.0-only

mod layout;
pub use layout::TilingLayout;

use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use slotmap::{SlotMap, new_key_type};
use smithay::{
    desktop::{Window, layer_map_for_output},
    output::Output,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        shell::{
            wlr_layer::{KeyboardInteractivity, Layer},
            xdg::{SurfaceCachedState, ToplevelSurface},
        },
    },
};

use crate::config::{self, Config};

new_key_type! {
    pub struct WindowId;
}

#[derive(Debug)]
pub struct WindowElement {
    pub id: WindowId,
    pub window: Window,

    pub app_id: String,
    pub title: String,
    pub floating: bool,
    pub fullscreen: bool,
    pub focused: bool,

    pub tiled_geo: Rectangle<i32, Logical>,
    pub float_geo: Rectangle<i32, Logical>,
    fullscreen_geo: Rectangle<i32, Logical>,

    pub render: Vec<config::RenderStep>,
    pub radius: f32,
    rules: Vec<config::WindowRule>,

    pre_resize_buf: Option<(Size<i32, Logical>, Instant)>,
}

impl WindowElement {
    pub fn new(
        id: WindowId,
        window: Window,
        should_float: bool,
        rules: &[config::WindowRule],
    ) -> Self {
        let (app_id, title) = window
            .toplevel()
            .map(|tl| {
                with_states(tl.wl_surface(), |s| {
                    s.data_map
                        .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                        .and_then(|d| d.lock().ok())
                        .map(|d| {
                            (
                                d.app_id.clone().unwrap_or_default(),
                                d.title.clone().unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default();

        let window_size = window.geometry().size;
        Self {
            id,
            window,
            app_id,
            title,
            floating: should_float,
            fullscreen: false,
            focused: false,
            tiled_geo: Rectangle::default(),
            float_geo: Rectangle::from_size(window_size),
            fullscreen_geo: Rectangle::default(),
            render: Vec::new(),
            radius: 0.0,
            rules: rules.to_vec(),
            pre_resize_buf: None,
        }
    }

    pub fn resolve_init(&mut self) -> (Option<String>, Option<Vec<usize>>) {
        let mut output = None;
        let mut tags = None;
        for rule in &self.rules {
            if rule
                .r#match
                .matches(&self.app_id, &self.title, self.floating)
            {
                if let Some(ref init) = rule.init {
                    if let Some(f) = init.floating {
                        self.floating = f;
                    }
                    if let Some((w, h)) = init.size {
                        self.float_geo.size = (w, h).into();
                    }
                    if let Some((x, y)) = init.position {
                        self.float_geo.loc = (x, y).into();
                    }
                    if let Some(ref o) = init.output {
                        output = Some(o.clone());
                    }
                    if let Some(ref t) = init.tags {
                        tags = Some(t.clone());
                    }
                }
            }
        }
        (output, tags)
    }

    pub fn resolve_render(&mut self) {
        self.render.clear();
        for rule in &self.rules {
            if rule
                .r#match
                .matches(&self.app_id, &self.title, self.floating)
            {
                if let Some(ref render) = rule.render {
                    self.render.clone_from(render);
                }
            }
        }
        self.radius = self
            .render
            .iter()
            .find_map(|s| match s {
                config::RenderStep::WindowSurface { radius, .. } => Some(*radius),
                _ => None,
            })
            .unwrap_or(0.0);
    }

    pub fn geo(&self) -> Rectangle<i32, Logical> {
        if self.fullscreen {
            self.fullscreen_geo
        } else if self.floating {
            self.float_geo
        } else {
            self.tiled_geo
        }
    }

    pub fn set_app_id(&mut self, app_id: String) {
        self.app_id = app_id;
        self.resolve_render();
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
        self.resolve_render();
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.window.set_activated(focused);
        if let Some(tl) = self.window.toplevel() {
            tl.send_pending_configure();
        }
    }

    pub fn set_fullscreen(&mut self, geo: Option<Rectangle<i32, Logical>>) {
        self.fullscreen = geo.is_some();
        if let Some(g) = geo {
            self.fullscreen_geo = g;
        }
        if let Some(tl) = self.window.toplevel() {
            tl.with_pending_state(|s| {
                if self.fullscreen {
                    s.states.set(xdg_toplevel::State::Fullscreen);
                } else {
                    s.states.unset(xdg_toplevel::State::Fullscreen);
                }
            });
        }
    }

    pub fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
        self.fullscreen = false;
        self.resolve_render();
        if let Some(tl) = self.window.toplevel() {
            tl.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Fullscreen);
            });
        }
    }

    pub fn configure(&mut self) {
        let Some(tl) = self.window.toplevel() else {
            return;
        };
        tl.with_pending_state(|s| {
            s.size = Some(self.geo().size);
        });
        if tl.send_pending_configure().is_some() {
            self.pre_resize_buf = Some((self.window.geometry().size, Instant::now()));
        }
    }

    pub fn on_commit(&mut self) {
        self.window.on_commit();
        if let Some((old, _)) = self.pre_resize_buf {
            let buf = self.window.geometry().size;
            if buf != old || buf == self.geo().size {
                self.pre_resize_buf = None;
            }
        }
    }

    pub fn has_pending_resize(&self) -> bool {
        self.pre_resize_buf
            .is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(300))
    }
}

pub fn should_float(tl: &ToplevelSurface) -> bool {
    if tl.parent().is_some() {
        return true;
    }

    let (min, max) = with_states(tl.wl_surface(), |states| {
        let mut data = states.cached_state.get::<SurfaceCachedState>();
        let cur = data.current();
        (cur.min_size, cur.max_size)
    });
    min.w > 0 && min.h > 0 && (min.w == max.w || min.h == max.h)
}

#[derive(Debug, Default)]
pub struct Windows(pub SlotMap<WindowId, WindowElement>);

impl Deref for Windows {
    type Target = SlotMap<WindowId, WindowElement>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Windows {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Windows {
    pub fn update_rules(&mut self, rules: &[config::WindowRule]) {
        for we in self.values_mut() {
            we.rules = rules.to_vec();
            we.resolve_render();
        }
    }

    pub fn find_by_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        for we in self.values() {
            if let Some(tl) = we.window.toplevel() {
                if tl.wl_surface() == surface {
                    return Some(we.id);
                }
            }
        }
        None
    }

    pub fn visible(&self, tag: &Tag) -> Vec<&WindowElement> {
        if let Some(id) = tag.fullscreen {
            return self.get(id).into_iter().collect();
        }
        tag.window_ids().filter_map(|id| self.get(id)).collect()
    }

    pub fn configure_visible(&mut self, tag: &Tag) {
        let ids: Vec<_> = self.visible(tag).iter().map(|we| we.id).collect();
        for id in ids {
            if let Some(we) = self.get_mut(id) {
                we.configure();
            }
        }
    }

    pub fn any_pending_resize(&self, tag: &Tag) -> bool {
        tag.window_ids()
            .any(|id| self.get(id).is_some_and(|we| we.has_pending_resize()))
    }

    pub fn window_under(&self, tag: &Tag, pos: Point<f64, Logical>) -> Option<&WindowElement> {
        if let Some(id) = tag.fullscreen {
            return self.get(id);
        }
        for id in tag.window_ids().rev() {
            if let Some(we) = self.get(id)
                && we.geo().to_f64().contains(pos)
            {
                return Some(we);
            }
        }
        None
    }
}

#[derive(Debug, Default)]
pub struct Tag {
    pub tiled: Vec<WindowId>,
    pub floating: Vec<WindowId>,
    pub focus_stack: Vec<WindowId>,
    pub layout: TilingLayout,
    pub fullscreen: Option<WindowId>,
}

impl Tag {
    fn contains(&self, id: WindowId) -> bool {
        self.focus_stack.contains(&id)
    }

    fn remove(&mut self, id: WindowId) {
        self.tiled.retain(|&wid| wid != id);
        self.floating.retain(|&wid| wid != id);
        self.focus_stack.retain(|&wid| wid != id);
        if self.fullscreen == Some(id) {
            self.fullscreen = None;
        }
    }

    fn add(&mut self, id: WindowId) {
        self.remove(id);
        self.focus_stack.insert(0, id);
    }

    pub fn window_ids(&self) -> impl DoubleEndedIterator<Item = WindowId> + '_ {
        self.tiled.iter().chain(self.floating.iter()).copied()
    }

    pub fn focus(&mut self, id: WindowId) {
        self.focus_stack.retain(|&x| x != id);
        self.focus_stack.insert(0, id);
    }

    pub fn focus_cycle(&self, delta: i32) -> Option<WindowId> {
        let pos = self.focused_tiled_pos()?;
        let next = (pos as i32 + delta).rem_euclid(self.tiled.len() as i32) as usize;
        Some(self.tiled[next])
    }

    pub fn focused_id(&self) -> Option<WindowId> {
        self.focus_stack.first().copied()
    }

    fn focused_tiled_pos(&self) -> Option<usize> {
        let current = self.focused_id()?;
        self.tiled.iter().position(|&id| id == current)
    }

    pub fn raise(&mut self, id: WindowId) {
        if let Some(pos) = self.floating.iter().position(|&wid| wid == id) {
            let id = self.floating.remove(pos);
            self.floating.push(id);
        }
    }

    pub fn move_in_stack(&mut self, delta: i32) {
        let Some(pos) = self.focused_tiled_pos() else {
            return;
        };
        let next = (pos as i32 + delta).rem_euclid(self.tiled.len() as i32) as usize;
        self.tiled.swap(pos, next);
    }

    pub fn zoom(&mut self) {
        let Some(pos) = self.focused_tiled_pos() else {
            return;
        };
        if pos > 0 {
            self.tiled.swap(0, pos);
        }
    }

    pub fn adjust_mfact(&mut self, delta: f32) {
        self.layout.master_factor = (self.layout.master_factor + delta).clamp(0.1, 0.9);
    }

    pub fn adjust_nmaster(&mut self, delta: i32) {
        self.layout.master_count = (self.layout.master_count as i32 + delta).max(1) as usize;
    }
}

#[derive(Debug)]
pub struct Monitor {
    pub output: Output,
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub prev_tag: usize,
    pub background: [f32; 4],
    pub exclusive_layer: Option<WlSurface>,
}

impl Monitor {
    pub fn new(output: Output, config: &Config) -> Self {
        let mut mon = Self {
            output,
            tags: (0..config.layout.tags).map(|_| Tag::default()).collect(),
            active_tag: 0,
            exclusive_layer: None,
            prev_tag: 0,
            background: [0.0, 0.0, 0.0, 1.0],
        };
        mon.resolve(config);
        mon
    }

    pub fn resolve(&mut self, config: &Config) {
        let name = self.output.name();
        let props = self.output.physical_properties();
        let mut bg = None;
        for rule in &config.outputs {
            if rule
                .r#match
                .matches(&name, &props.make, &props.model, &props.serial_number)
            {
                if let Some(c) = rule.background {
                    bg = Some(c);
                }
            }
        }
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
