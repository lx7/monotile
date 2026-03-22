// SPDX-License-Identifier: GPL-3.0-only

use crate::{Monotile, shell::should_float};
use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    delegate_kde_decoration, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        PopupKeyboardGrab, PopupKind, PopupPointerGrab, Window, WindowSurfaceType,
        find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
    },
    input::{Seat, pointer::Focus},
    reexports::{
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
        wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration::OrgKdeKwinServerDecoration,
        wayland_server::protocol::{wl_output, wl_seat, wl_surface::WlSurface},
    },
    utils::Serial,
    wayland::{
        compositor::with_states,
        shell::{
            kde::decoration::{KdeDecorationHandler, KdeDecorationState},
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData, decoration::XdgDecorationHandler,
            },
        },
    },
};

impl XdgShellHandler for Monotile {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.state.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.state.pending.push(Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let wl = surface.wl_surface();
        self.state
            .pending
            .retain(|w| w.toplevel().is_none_or(|tl| tl.wl_surface() != wl));
        if let Some(id) = self.state.windows.find_by_surface(wl) {
            self.state.unmap(id);
            self.recompute_layout();
            self.backend.schedule_render(&self.state.mon().output);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.state.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        // for mapped windows that get a parent set late
        let wl = surface.wl_surface();
        if let Some(id) = self.state.windows.find_by_surface(wl)
            && surface.parent().is_some()
        {
            self.state.windows[id].set_floating(true);
            self.recompute_layout();
        }
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // ignored, compositor controls window movement
    }

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _edges: smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge,
    ) {
        // ignored, compositor controls window resizing
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        if let Some(id) = self.state.windows.find_by_surface(surface.wl_surface()) {
            let geo = self.state.mon().output_geometry();
            self.state.windows[id].set_fullscreen(Some(geo));
            self.recompute_layout();
        } else {
            surface.send_pending_configure();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.state.windows.find_by_surface(surface.wl_surface()) {
            self.state.windows[id].set_fullscreen(None);
            self.recompute_layout();
            self.backend.schedule_render(&self.state.mon().output);
        } else {
            surface.send_pending_configure();
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        let wl = surface.wl_surface();
        let title = with_states(wl, |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok()?.title.clone())
                .unwrap_or_default()
        });
        if let Some(id) = self.state.windows.find_by_surface(wl) {
            self.state.windows[id].set_title(title);
            self.state.ipc.mark_dirty();
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        let wl = surface.wl_surface();
        let app_id = with_states(wl, |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok()?.app_id.clone())
                .unwrap_or_default()
        });
        if let Some(id) = self.state.windows.find_by_surface(wl) {
            self.state.windows[id].set_app_id(app_id);
            self.state.ipc.mark_dirty();
        }
    }

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let seat: Seat<Monotile> = Seat::from_resource(&seat).unwrap();
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Ok(grab) = self.state.popups.grab_popup(root, kind, &seat, serial) else {
            return;
        };
        if let Some(kb) = seat.get_keyboard() {
            kb.set_focus(self, grab.current_grab(), serial);
            kb.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(ptr) = seat.get_pointer() {
            ptr.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }
}

delegate_xdg_shell!(Monotile);

// force server-side decorations
impl XdgDecorationHandler for Monotile {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        Self::set_server_side_decoration(&toplevel, false);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: Mode) {
        Self::set_server_side_decoration(&toplevel, true);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        Self::set_server_side_decoration(&toplevel, true);
    }
}

delegate_xdg_decoration!(Monotile);

// force server-side decorations (for GTK/Qt apps)
impl KdeDecorationHandler for Monotile {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.state.kde_decoration_state
    }

    fn new_decoration(&mut self, _: &WlSurface, _: &OrgKdeKwinServerDecoration) {}
}

delegate_kde_decoration!(Monotile);

/// called on `WlSurface::commit`.
/// returns true if a pending window just mapped.
pub fn handle_commit(state: &mut crate::state::State, surface: &WlSurface) -> bool {
    let mut mapped = false;
    if let Some((idx, tl)) = state.find_pending(surface) {
        let sent = with_states(surface, |states| {
            let mutex = states.data_map.get::<XdgToplevelSurfaceData>().unwrap();
            mutex.lock().unwrap().initial_configure_sent
        });
        if !sent {
            let floating = should_float(&tl);
            let window = state.pending.remove(idx);
            state.map(window, floating);
            mapped = true;
        } else {
            let has_buffer =
                with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false);
            if has_buffer {
                if let Some(id) = state.windows.find_by_surface(surface) {
                    state.windows[id].on_commit();
                }
            }
        }
    }

    state.popups.commit(surface);
    if let Some(popup) = state.popups.find_popup(surface) {
        match popup {
            PopupKind::Xdg(ref xdg) => {
                if !xdg.is_initial_configure_sent() {
                    // crash when a popup has no parent. should not happen,
                    // but if it does we want to notice it (crash)
                    xdg.send_configure().expect("initial configure");
                }
            }
            PopupKind::InputMethod(_) => {}
        }
    }

    mapped
}

impl Monotile {
    fn set_server_side_decoration(toplevel: &ToplevelSurface, send_configure: bool) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ServerSide);
        });
        if send_configure && toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    // reposition popup if it extends beyond the screen edge
    pub(crate) fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };

        // constraint rect depends on whether parent is a window or layer surface
        let popup_offset = get_popup_toplevel_coords(&kind);
        let parent = self
            .state
            .windows
            .find_by_surface(&root)
            .and_then(|id| self.state.windows.get(id));
        let mon = self.state.mon();

        let parent_loc = if let Some(we) = parent {
            we.target_loc()
        } else {
            let map = layer_map_for_output(&mon.output);
            let Some(l) = map.layer_for_surface(&root, WindowSurfaceType::TOPLEVEL) else {
                return;
            };
            let Some(geo) = map.layer_geometry(l) else {
                return;
            };
            geo.loc
        };

        // convert output rect to popup-local coordinates
        let mut target = mon.output_geometry();
        target.loc -= parent_loc;
        target.loc -= popup_offset;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
