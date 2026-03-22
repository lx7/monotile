// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use derive_more::{Deref, DerefMut};

use slotmap::SlotMap;
use smithay::{
    desktop::{Window, WindowSurfaceType},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, backend::ObjectId, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{SurfaceCachedState, ToplevelSurface},
    },
};

use crate::{config, render::RenderStep};

use super::{Tag, WindowId};

#[derive(Debug)]
pub struct WindowElement {
    pub id: WindowId,
    pub window: Window,

    pub monitor: usize,
    pub app_id: String,
    pub title: String,
    pub floating: bool,
    pub fullscreen: bool,
    pub focused: bool,

    pub tiled_geo: Rectangle<i32, Logical>,
    pub float_geo: Rectangle<i32, Logical>,
    fullscreen_geo: Rectangle<i32, Logical>,

    pub render: Vec<RenderStep>,
    pub radius: f32,
    rules: Vec<config::WindowRule>,

    // client protocol and rendering
    configured_geo: Rectangle<i32, Logical>,
    pub render_geo: Rectangle<i32, Logical>,
    pub committed: bool,
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
            monitor: 0,
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
            configured_geo: Rectangle::default(),
            render_geo: Rectangle::default(),
            committed: false,
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

    pub fn resolve_render(&mut self) {
        let mut matched = None;
        for rule in &self.rules {
            if rule
                .r#match
                .matches(&self.app_id, &self.title, self.floating)
            {
                if rule.render.is_some() {
                    matched = rule.render.as_ref();
                }
            }
        }
        self.render = matched
            .map(|steps| steps.iter().map(RenderStep::from_config).collect())
            .unwrap_or_default();
        self.radius = self
            .render
            .iter()
            .find_map(|s| match s {
                RenderStep::WindowSurface { radius, .. } => Some(*radius),
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
            tl.with_pending_state(|s| {
                s.states.set(xdg_toplevel::State::Resizing);
                s.size = Some(size);
            });
            tl.send_pending_configure();
        }
    }

    pub fn finish_resize_float(&mut self) {
        if let Some(tl) = self.window.toplevel() {
            tl.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Resizing);
                s.size = Some(self.float_geo.size);
            });
            tl.send_pending_configure();
        }
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
        let target = self.geo();
        // nothing changed
        if target == self.configured_geo {
            return;
        }
        // update rendering position + invalidate decoration caches
        self.render_geo = target;
        for step in &mut self.render {
            step.clear();
        }
        // position-only change - no configure needed
        if target.size == self.configured_geo.size {
            self.configured_geo = target;
            return;
        }
        // size changed - send configure to client
        self.configured_geo = target;
        let Some(tl) = self.window.toplevel() else {
            return;
        };
        tl.with_pending_state(|s| {
            s.size = Some(target.size);
        });
        if tl.is_initial_configure_sent() {
            tl.send_pending_configure();
        } else {
            tl.send_configure();
        }
    }

    pub fn on_commit(&mut self) {
        self.window.on_commit();
        self.committed = true;
        if self.render_geo != self.geo() {
            self.render_geo = self.geo();
            for step in &mut self.render {
                step.clear();
            }
        }
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

    pub fn remove(&mut self, id: WindowId) -> Option<WindowElement> {
        if let Some(we) = self.inner.get(id) {
            if let Some(tl) = we.window.toplevel() {
                self.by_surface.remove(&tl.wl_surface().id());
            }
            if self.focused == Some(id) {
                self.focused = None;
            }
        }
        self.inner.remove(id)
    }

    pub fn update_rules(&mut self, rules: &[config::WindowRule]) {
        for we in self.inner.values_mut() {
            we.rules = rules.to_vec();
            we.resolve_render();
        }
    }

    pub fn find_by_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        self.by_surface.get(&surface.id()).copied()
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

    pub fn window_id_under(&self, tag: &Tag, pos: Point<f64, Logical>) -> Option<WindowId> {
        for id in tag.window_ids().rev() {
            let Some(we) = self.get(id) else { continue };
            let loc = we.surface_loc();
            let rel = pos - loc.to_f64();
            if we
                .window
                .surface_under(rel, WindowSurfaceType::ALL)
                .is_some()
            {
                return Some(id);
            }
        }
        None
    }

    pub fn window_under(&self, tag: &Tag, pos: Point<f64, Logical>) -> Option<&WindowElement> {
        if let Some(id) = tag.fullscreen {
            return self.get(id);
        }
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
