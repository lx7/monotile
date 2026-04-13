// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, HashMap};

use derive_more::{Deref, DerefMut};

use slotmap::SlotMap;
use smithay::{
    desktop::Window,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, backend::ObjectId, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{SurfaceCachedState, ToplevelSurface, XdgToplevelSurfaceData},
    },
};

use crate::{config, render::RenderStep};

use super::{Tag, WindowId};

pub trait ToplevelSurfaceExt {
    fn info(&self) -> (String, String);
}

impl ToplevelSurfaceExt for ToplevelSurface {
    fn info(&self) -> (String, String) {
        with_states(self.wl_surface(), |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok())
                .map(|d| {
                    (
                        d.app_id.clone().unwrap_or_default(),
                        d.title.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or_default()
        })
    }
}

pub struct Unmapped {
    pub window: Window,
    pub placement: Option<Placement>,
    pub rules: Vec<config::WindowRule>,
}

pub struct Placement {
    pub floating: bool,
    pub monitor: usize,
    pub configured_size: Size<i32, Logical>,
}

impl Unmapped {
    pub fn new(window: Window, rules: Vec<config::WindowRule>) -> Self {
        Self {
            window,
            placement: None,
            rules,
        }
    }

    pub fn should_float(&self) -> bool {
        let Some(tl) = self.window.toplevel() else {
            return false;
        };
        if tl.parent().is_some() {
            return true;
        }
        let (min, max) = with_states(tl.wl_surface(), |states| {
            let mut data = states.cached_state.get::<SurfaceCachedState>();
            let cur = data.current();
            (cur.min_size, cur.max_size)
        });
        if min.h > 0 && min.h == max.h {
            return true;
        }
        let (app_id, title) = tl.info();
        self.rules.iter().any(|rule| {
            let m = &rule.r#match;
            rule.init.as_ref().is_some_and(|i| i.floating == Some(true))
                && m.app_id.as_ref().is_none_or(|p| p.is_match(&app_id))
                && m.title.as_ref().is_none_or(|p| p.is_match(&title))
        })
    }
}

#[derive(Debug)]
pub struct WindowElement {
    // identity
    pub id: WindowId,
    pub window: Window,

    // state
    pub monitor: usize,
    pub app_id: String,
    pub title: String,
    pub floating: bool,
    pub fullscreen: bool,
    pub focused: bool,
    pub screencasts: u32,

    // target geometry
    pub tiled_geo: Rectangle<i32, Logical>,
    pub float_geo: Rectangle<i32, Logical>,
    pub fullscreen_geo: Rectangle<i32, Logical>,

    // current on-screen geo
    pub render_geo: Rectangle<i32, Logical>,

    // last configure sent to client
    configured_geo: Rectangle<i32, Logical>,

    // rendering: steps keyed by (rule_index, slot)
    pub render_steps: BTreeMap<(usize, u32), RenderStep>,
    pub render_pipeline: Vec<(usize, u32)>,
    pub radius: f32,
    rules: Vec<config::WindowRule>,

    // true after client commits a buffer, cleared after send_frame
    pub buffer_committed: bool,
}

impl WindowElement {
    pub fn new(id: WindowId, unmapped: Unmapped) -> Self {
        let placement = unmapped.placement.unwrap();
        let rules = unmapped.rules;
        let window = unmapped.window;
        let (app_id, title) = window.toplevel().unwrap().info();

        let float_size = window.geometry().size;
        let configured_size =
            if placement.floating { float_size } else { placement.configured_size };
        Self {
            id,
            window,
            monitor: placement.monitor,
            app_id,
            title,
            floating: placement.floating,
            fullscreen: false,
            focused: false,
            screencasts: 0,
            tiled_geo: Rectangle::default(),
            float_geo: Rectangle::from_size(float_size),
            fullscreen_geo: Rectangle::default(),
            render_steps: BTreeMap::new(),
            render_pipeline: Vec::new(),
            radius: 0.0,
            rules,
            configured_geo: Rectangle::from_size(configured_size),
            render_geo: Rectangle::default(),
            buffer_committed: false,
        }
    }

    fn matches(&self, rule: &config::WindowRule) -> bool {
        let m = &rule.r#match;
        m.app_id.as_ref().is_none_or(|p| p.is_match(&self.app_id))
            && m.title.as_ref().is_none_or(|p| p.is_match(&self.title))
            && m.floating.is_none_or(|v| v == self.floating)
            && m.focused.is_none_or(|v| v == self.focused)
            && m.screencast.is_none_or(|v| v == (self.screencasts > 0))
    }

    pub fn resolve_init(&mut self) -> (Option<String>, Option<Vec<usize>>) {
        let mut output = None;
        let mut tags = None;
        for rule in &self.rules {
            if self.matches(rule) {
                let Some(init) = &rule.init else { continue };
                self.floating = init.floating.unwrap_or(self.floating);
                if let Some((w, h)) = init.size {
                    self.float_geo.size = (w, h).into();
                }
                if let Some((x, y)) = init.position {
                    self.float_geo.loc = (x, y).into();
                }
                output = init.output.clone().or(output);
                tags = init.tags.clone().or(tags);
            }
        }
        (output, tags)
    }

    pub fn build_render_steps(&mut self) {
        self.render_steps.clear();
        for (ri, rule) in self.rules.iter().enumerate() {
            for (&slot, step) in rule.render.iter().flatten() {
                if let Some(rs) = RenderStep::from_config(step) {
                    self.render_steps.insert((ri, slot), rs);
                }
            }
        }
        self.resolve_render();
    }

    pub fn resolve_render(&mut self) {
        let mut active: BTreeMap<u32, (usize, u32)> = BTreeMap::new();
        self.radius = 0.0;
        for (ri, rule) in self.rules.iter().enumerate() {
            if self.matches(rule) {
                for &slot in rule.render.iter().flat_map(|r| r.keys()) {
                    let key = (ri, slot);
                    if self.render_steps.contains_key(&key) {
                        active.insert(slot, key);
                    } else {
                        active.remove(&slot);
                    }
                }
            }
        }
        self.render_pipeline = active.into_values().collect();
        for key in &self.render_pipeline {
            if let Some(RenderStep::WindowSurface { radius, .. }) = self.render_steps.get(key) {
                self.radius = *radius;
            }
        }
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

    pub fn min_max_size(&self) -> (Size<i32, Logical>, Size<i32, Logical>) {
        self.window
            .toplevel()
            .map(|tl| {
                with_states(tl.wl_surface(), |states| {
                    let mut data = states.cached_state.get::<SurfaceCachedState>();
                    let cur = data.current();
                    (cur.min_size, cur.max_size)
                })
            })
            .unwrap_or_default()
    }

    pub fn resize_float(&mut self, size: Size<i32, Logical>) {
        self.float_geo.size = size;
        if let Some(tl) = self.window.toplevel() {
            tl.with_pending_state(|s| s.states.set(xdg_toplevel::State::Resizing));
        }
        self.configure();
    }

    pub fn finish_resize_float(&mut self) {
        if let Some(tl) = self.window.toplevel() {
            tl.with_pending_state(|s| s.states.unset(xdg_toplevel::State::Resizing));
        }
        self.configure();
    }

    pub fn surface_loc(&self) -> Point<i32, Logical> {
        self.render_geo.loc - self.window.geometry().loc
    }

    pub fn target_loc(&self) -> Point<i32, Logical> {
        self.geo().loc - self.window.geometry().loc
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
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        self.window.set_activated(focused);
        if let Some(tl) = self.window.toplevel() {
            tl.send_pending_configure();
        }
        self.resolve_render();
    }

    pub fn mark_screencast(&mut self) {
        self.screencasts += 1;
        if self.screencasts == 1 {
            self.resolve_render();
        }
    }

    pub fn unmark_screencast(&mut self) {
        self.screencasts -= 1;
        if self.screencasts == 0 {
            self.resolve_render();
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

    fn clear_render_cache(&mut self) {
        for step in self.render_steps.values_mut() {
            step.clear();
        }
    }

    pub fn configure(&mut self) {
        let target = self.geo();
        // nothing changed
        if target == self.configured_geo {
            return;
        }
        // position-only - update render_geo, no client roundtrip
        if target.size == self.configured_geo.size {
            self.render_geo.loc = target.loc;
            self.configured_geo = target;
            self.clear_render_cache();
            return;
        }
        let Some(tl) = self.window.toplevel() else {
            return;
        };
        self.configured_geo = target;
        tl.with_pending_state(|s| s.size = Some(target.size));
        tl.send_pending_configure();
    }

    pub fn on_commit(&mut self) {
        self.window.on_commit();
        self.buffer_committed = true;

        if self.render_geo != self.geo() {
            self.render_geo = self.geo();
            self.clear_render_cache();
        }
    }
}

#[derive(Debug, Default, Deref, DerefMut)]
pub struct Windows {
    #[deref]
    #[deref_mut]
    inner: SlotMap<WindowId, WindowElement>,
    by_surface: HashMap<ObjectId, WindowId>,
    pub focused: Option<WindowId>,
}

impl Windows {
    pub fn insert_with_key(&mut self, f: impl FnOnce(WindowId) -> WindowElement) -> WindowId {
        let id = self.inner.insert_with_key(f);
        if let Some(tl) = self.inner[id].window.toplevel() {
            self.by_surface.insert(tl.wl_surface().id(), id);
        }
        id
    }

    pub fn remove(&mut self, surface: &ObjectId) -> Option<WindowElement> {
        let id = self.by_surface.remove(surface)?;
        if self.focused == Some(id) {
            self.focused = None;
        }
        self.inner.remove(id)
    }

    pub fn update_rules(&mut self, rules: &[config::WindowRule]) {
        for we in self.inner.values_mut() {
            we.rules = rules.to_vec();
            we.build_render_steps();
        }
    }

    pub fn find_by_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        self.by_surface.get(&surface.id()).copied()
    }

    pub fn focused_surface(&self) -> Option<WlSurface> {
        let we = self.get(self.focused?)?;
        we.window.toplevel().map(|tl| tl.wl_surface().clone())
    }

    pub fn visible(&self, tag: &Tag) -> Vec<&WindowElement> {
        tag.window_ids().filter_map(|id| self.get(id)).collect()
    }

    pub fn window_under(&self, tag: &Tag, pos: Point<f64, Logical>) -> Option<&WindowElement> {
        for id in tag.window_ids().rev() {
            if let Some(we) = self.get(id)
                && we.render_geo.to_f64().contains(pos)
            {
                return Some(we);
            }
        }
        None
    }
}
