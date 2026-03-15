// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, Instant};

use derive_more::{Deref, DerefMut};
use slotmap::SlotMap;
use smithay::{
    desktop::{Window, WindowSurfaceType},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{SurfaceCachedState, ToplevelSurface},
    },
};

use crate::config;

use super::{Tag, WindowId};

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

#[derive(Debug, Default, Deref, DerefMut)]
pub struct Windows(pub SlotMap<WindowId, WindowElement>);

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

    pub fn window_id_under(&self, tag: &Tag, pos: Point<f64, Logical>) -> Option<WindowId> {
        for id in tag.window_ids().rev() {
            let Some(we) = self.get(id) else { continue };
            let loc = we.geo().loc - we.window.geometry().loc;
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
                && we.geo().to_f64().contains(pos)
            {
                return Some(we);
            }
        }
        None
    }
}
