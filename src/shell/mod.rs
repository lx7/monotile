// SPDX-License-Identifier: GPL-3.0-only

//! This module is the core window management abstraction that handles:
//! - Window lifecycle (map, unmap, get)
//! - Per-monitor tags (similar to workspaces)
//! - Tiling layout computation
//! - Window queries (visible, under cursor, etc.)
mod layout;
pub use layout::TilingLayout;

use slotmap::{SlotMap, new_key_type};
use smithay::{
    desktop::{Window, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::{
        compositor::with_states,
        shell::{
            wlr_layer::{KeyboardInteractivity, Layer},
            xdg::{SurfaceCachedState, ToplevelSurface},
        },
    },
};

use crate::config::TAGCOUNT;

new_key_type! {
    pub struct WindowId;
}

#[derive(Debug)]
pub struct WindowElement {
    pub id: WindowId,
    pub window: Window,
    pub tiled_geo: Rectangle<i32, Logical>,
    pub float_geo: Rectangle<i32, Logical>,
    pub floating: bool,
    pub focused: bool,
}

impl WindowElement {
    pub fn geo(&self) -> Rectangle<i32, Logical> {
        if self.floating {
            self.float_geo
        } else {
            self.tiled_geo
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.window.set_activated(focused);
        if let Some(tl) = self.window.toplevel() {
            tl.send_pending_configure();
        }
    }
}

/// Check if a new window should open as floating.
///
/// Heuristic, in this order:
/// - Does the window match a rule that explicitly sets it to floating? (not implemented yet)
/// - Does it have a parent window? (usually dialogs)
/// - Does it have a fixed width/height?
pub fn should_float(tl: &ToplevelSurface) -> bool {
    // TODO: check window rules here (override heuristics)

    // windows with a parent
    if tl.parent().is_some() {
        return true;
    }

    // fixed-size windows
    let (min, max) = with_states(tl.wl_surface(), |states| {
        let mut data = states.cached_state.get::<SurfaceCachedState>();
        let cur = data.current();
        (cur.min_size, cur.max_size)
    });
    min.w > 0 && min.h > 0 && (min.w == max.w || min.h == max.h)
}

#[derive(Debug, Default)]
pub struct Tag {
    pub tiled: Vec<WindowId>,
    pub floating: Vec<WindowId>,
    pub focus_stack: Vec<WindowId>,
    pub layout: TilingLayout,
}

impl Tag {
    fn contains(&self, id: WindowId) -> bool {
        self.tiled.contains(&id) || self.floating.contains(&id)
    }

    fn remove(&mut self, id: WindowId) {
        self.tiled.retain(|&wid| wid != id);
        self.floating.retain(|&wid| wid != id);
        self.focus_stack.retain(|&wid| wid != id);
    }

    fn add(&mut self, id: WindowId, floating: bool) {
        self.remove(id);
        if floating {
            self.floating.push(id);
        } else {
            self.tiled.push(id);
        }
        self.focus_stack.insert(0, id);
    }

    /// Get all window IDs in render order (tiled first, then floating)
    pub fn window_ids(&self) -> impl DoubleEndedIterator<Item = WindowId> + '_ {
        self.tiled.iter().chain(self.floating.iter()).copied()
    }

    /// Raise floating window to top of z-order
    pub fn raise(&mut self, id: WindowId) {
        if let Some(pos) = self.floating.iter().position(|&wid| wid == id) {
            let id = self.floating.remove(pos);
            self.floating.push(id);
        }
    }

    pub fn move_in_stack(&mut self, current: WindowId, delta: i32) {
        let Some(pos) = self.tiled.iter().position(|&id| id == current) else {
            return;
        };
        let next = (pos as i32 + delta).rem_euclid(self.tiled.len() as i32) as usize;
        self.tiled.swap(pos, next);
    }

    pub fn zoom(&mut self, current: WindowId) {
        let Some(pos) = self.tiled.iter().position(|&id| id == current) else {
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

    pub fn focus_cycle(&self, delta: i32) -> Option<WindowId> {
        let current = *self.focus_stack.first()?;
        let pos = self.tiled.iter().position(|&id| id == current)?;
        let next = (pos as i32 + delta).rem_euclid(self.tiled.len() as i32) as usize;
        Some(self.tiled[next])
    }
}

/// Per-monitor state with independent tag and window storage
#[derive(Debug)]
pub struct Monitor {
    windows: SlotMap<WindowId, WindowElement>,
    pub output: Output,
    pub tags: [Tag; TAGCOUNT],
    pub active_tag: usize,
    pub prev_tag: usize,
}

// TODO: review methods. Do window queries and layout delegates belong here?
impl Monitor {
    pub fn new(output: Output) -> Self {
        Self {
            windows: SlotMap::with_key(),
            output,
            tags: [(); TAGCOUNT].map(|_| Tag::default()),
            active_tag: 0,
            prev_tag: 0,
        }
    }

    pub fn tag(&self) -> &Tag {
        &self.tags[self.active_tag]
    }

    pub fn tag_mut(&mut self) -> &mut Tag {
        &mut self.tags[self.active_tag]
    }

    // === Window lifecycle ===

    pub fn map(&mut self, window: Window, floating: bool) -> WindowId {
        let area = layer_map_for_output(&self.output).non_exclusive_zone();
        let size = window.geometry().size;

        let fw = if size.w > 0 {
            size.w
        } else {
            area.size.w * 3 / 4
        };
        let fh = if size.h > 0 {
            size.h
        } else {
            area.size.h * 3 / 4
        };

        let x = area.loc.x + (area.size.w - fw) / 2;
        let y = area.loc.y + (area.size.h - fh) / 2;
        let float_geo = Rectangle::new((x, y).into(), (fw, fh).into());

        let id = self.windows.insert_with_key(|id| WindowElement {
            id,
            window,
            tiled_geo: Rectangle::default(),
            float_geo,
            floating,
            focused: false,
        });

        self.tag_mut().add(id, floating);
        self.recompute_layout();
        id
    }

    pub fn unmap(&mut self, id: WindowId) {
        for tag in &mut self.tags {
            tag.remove(id);
        }
        self.windows.remove(id);
        self.recompute_layout();
    }

    pub fn get(&self, id: WindowId) -> Option<&WindowElement> {
        self.windows.get(id)
    }

    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut WindowElement> {
        self.windows.get_mut(id)
    }

    // === Activation / Focus stack ===

    /// Get currently active window (front of focus stack)
    pub fn focused_window(&self) -> Option<&WindowElement> {
        let id = *self.tag().focus_stack.first()?;
        self.windows.get(id)
    }

    /// Set focus to a window (or clear focus). Handles unfocus/focus/activate/focus stack.
    pub fn set_focus(&mut self, id: Option<WindowId>) -> Option<WlSurface> {
        for we in self.windows.values_mut() {
            if we.focused {
                we.set_focused(false);
            }
        }
        if let Some(id) = id
            && let Some(we) = self.windows.get_mut(id)
        {
            we.set_focused(true);
            let stack = &mut self.tags[self.active_tag].focus_stack;
            stack.retain(|&x| x != id);
            stack.insert(0, id);
            return we.window.toplevel().map(|tl| tl.wl_surface().clone());
        }
        None
    }

    // === Window operations (active) ===

    pub fn active_id(&self) -> Option<WindowId> {
        self.tag().focus_stack.first().copied()
    }

    pub fn move_active_to_tag(&mut self, tag: usize) {
        if tag >= TAGCOUNT {
            return;
        }
        let Some(id) = self.active_id() else { return };
        let floating = self.windows.get(id).is_some_and(|we| we.floating);
        for t in &mut self.tags {
            t.remove(id);
        }
        self.tags[tag].add(id, floating);
        self.recompute_layout();
    }

    pub fn toggle_active_tag(&mut self, tag: usize) {
        if tag >= TAGCOUNT {
            return;
        }
        let Some(id) = self.active_id() else { return };
        let floating = self.windows.get(id).is_some_and(|we| we.floating);

        if self.tags[tag].contains(id) {
            let count = self.tags.iter().filter(|t| t.contains(id)).count();
            if count > 1 {
                self.tags[tag].remove(id);
            }
        } else {
            self.tags[tag].add(id, floating);
        }
        self.recompute_layout();
    }

    pub fn set_floating(&mut self, id: WindowId, floating: bool) {
        let Some(we) = self.windows.get_mut(id) else {
            return;
        };
        if we.floating == floating {
            return;
        }
        we.floating = floating;
        if floating && let Some(tl) = we.window.toplevel() {
            tl.with_pending_state(|s| s.size = Some(we.float_geo.size));
            tl.send_pending_configure();
        }
        for tag in &mut self.tags {
            if tag.contains(id) {
                tag.add(id, floating);
            }
        }
        self.recompute_layout();
    }

    pub fn toggle_active_floating(&mut self) {
        let Some(id) = self.active_id() else { return };
        let Some(we) = self.windows.get(id) else {
            return;
        };
        let floating = !we.floating;
        self.set_floating(id, floating);
    }

    pub fn kill_active(&self) {
        if let Some(tl) = self.focused_window().and_then(|we| we.window.toplevel()) {
            tl.send_close();
        }
    }

    // === Tag management ===

    pub fn set_active_tag(&mut self, tag: usize) {
        if tag >= TAGCOUNT {
            return;
        }
        self.prev_tag = self.active_tag;
        self.active_tag = tag;
        self.recompute_layout();
    }

    pub fn toggle_prev_tag(&mut self) {
        std::mem::swap(&mut self.active_tag, &mut self.prev_tag);
        self.recompute_layout();
    }

    pub fn output_geometry(&self) -> Rectangle<i32, Logical> {
        let size = self.output.current_mode().unwrap().size;
        Rectangle::new((0, 0).into(), size.to_logical(1))
    }

    // === Queries ===

    pub fn visible_windows(&self) -> impl Iterator<Item = &WindowElement> {
        self.tag()
            .window_ids()
            .filter_map(move |id| self.windows.get(id))
    }

    pub fn find_window_by_surface(&self, surface: &WlSurface) -> Option<&WindowElement> {
        self.windows.values().find(|we| {
            we.window
                .toplevel()
                .is_some_and(|tl| tl.wl_surface() == surface)
        })
    }

    pub fn find_by_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        self.find_window_by_surface(surface).map(|we| we.id)
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<&WindowElement> {
        for id in self.tag().window_ids().rev() {
            if let Some(we) = self.windows.get(id)
                && we.geo().to_f64().contains(pos)
            {
                return Some(we);
            }
        }
        None
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let map = layer_map_for_output(&self.output);
        let layer_hit = |layer| {
            let layer = map.layer_under(layer, pos)?;
            let geo = map.layer_geometry(layer).unwrap();
            let rel = pos - geo.loc.to_f64();
            let (s, point) = layer.surface_under(rel, WindowSurfaceType::ALL)?;
            Some((s, (point + geo.loc).to_f64()))
        };

        // overlay / top layers
        if let Some(hit) = layer_hit(Layer::Overlay).or_else(|| layer_hit(Layer::Top)) {
            return Some(hit);
        }

        // windows
        if let Some(we) = self.window_under(pos) {
            let loc = we.geo().loc;
            let rel = pos - loc.to_f64();
            if let Some((s, point)) = we.window.surface_under(rel, WindowSurfaceType::ALL) {
                return Some((s, (point + loc).to_f64()));
            }
        }

        // bottom / background layers
        layer_hit(Layer::Bottom).or_else(|| layer_hit(Layer::Background))
    }

    // === Layout ===

    /// Recompute layout for active tag
    pub fn recompute_layout(&mut self) {
        let geo = layer_map_for_output(&self.output).non_exclusive_zone();
        let tag = self.tag();
        let rects = tag.layout.compute_rects(tag.tiled.len(), geo);
        let tiled = tag.tiled.clone();
        for (id, rect) in tiled.iter().zip(rects) {
            let Some(we) = self.windows.get_mut(*id) else {
                continue;
            };
            we.tiled_geo = rect;
            if let Some(tl) = we.window.toplevel() {
                tl.with_pending_state(|s| {
                    s.size = Some(rect.size);
                });
                tl.send_pending_configure();
            }
        }
    }

    /// Find exclusive-keyboard layer surface (lock screens, launchers)
    pub fn exclusive_layer_surface(&self) -> Option<WlSurface> {
        let map = layer_map_for_output(&self.output);
        for l in [Layer::Overlay, Layer::Top] {
            for s in map.layers_on(l).rev() {
                if s.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive {
                    return Some(s.wl_surface().clone());
                }
            }
        }
        None
    }

    /// Move focused window up/down in the tiled stack
    pub fn move_in_stack(&mut self, delta: i32) {
        let Some(&current) = self.tag().focus_stack.first() else {
            return;
        };
        self.tag_mut().move_in_stack(current, delta);
        self.recompute_layout();
    }

    /// Swap focused window with master (first tiled window)
    pub fn zoom(&mut self) {
        let Some(&current) = self.tag().focus_stack.first() else {
            return;
        };
        self.tag_mut().zoom(current);
        self.recompute_layout();
    }

    /// Adjust master factor for current tag
    pub fn adjust_mfact(&mut self, delta: f32) {
        self.tag_mut().adjust_mfact(delta);
        self.recompute_layout();
    }

    /// Adjust master count for current tag
    pub fn adjust_nmaster(&mut self, delta: i32) {
        self.tag_mut().adjust_nmaster(delta);
        self.recompute_layout();
    }
}
