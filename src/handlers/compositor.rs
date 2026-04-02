// SPDX-License-Identifier: GPL-3.0-only

use crate::{Monotile, shell::Placement, state::ClientState};
use smithay::{
    backend::renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state},
    delegate_compositor, delegate_shm,
    desktop::{PopupKind, WindowSurfaceType, find_popup_root_surface, layer_map_for_output},
    reexports::wayland_server::{
        Client, Resource,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface, with_states,
        },
        shell::wlr_layer::{LayerSurfaceConfigure, LayerSurfaceData, LayerSurfaceState},
        shm::{ShmHandler, ShmState},
    },
};

impl CompositorHandler for Monotile {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.state.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if is_sync_subsurface(surface) {
            return;
        }

        self.state.popups.commit(surface);

        if let Some(mon) = self
            .on_window_commit(surface)
            .or_else(|| self.on_popup_commit(surface))
            .or_else(|| self.on_unmapped_commit(surface))
            .or_else(|| self.on_layer_commit(surface))
        {
            self.recompute_layout(mon);
        }

        // TODO: use the output the surface is mapped on when
        // multi-monitor is implemented
        self.backend.schedule_render(&self.state.mon().output);
    }
}

impl Monotile {
    fn on_window_commit(&mut self, surface: &WlSurface) -> Option<usize> {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        let id = self.state.windows.find_by_surface(&root)?;
        self.state.windows[id].on_commit();
        None
    }

    fn on_popup_commit(&mut self, surface: &WlSurface) -> Option<usize> {
        let popup = self.state.popups.find_popup(surface)?;

        if let PopupKind::Xdg(ref xdg) = popup {
            if !xdg.is_initial_configure_sent() {
                xdg.send_configure().expect("initial configure");
            }
        }

        if let Ok(popup_root) = find_popup_root_surface(&popup)
            && let Some(id) = self.state.windows.find_by_surface(&popup_root)
        {
            self.state.windows[id].buffer_committed = true;
        }

        None
    }

    /// Unmapped toplevel: two-phase configure/map state machine.
    fn on_unmapped_commit(&mut self, surface: &WlSurface) -> Option<usize> {
        let unmapped = self.state.unmapped.get_mut(&surface.id())?;

        if unmapped.placement.is_none() {
            // phase 1: first commit - send configure with tiled size
            let floating = unmapped.should_float();
            let mon = &self.state.monitors[self.state.active_monitor];
            let configured_size = if floating {
                (0, 0).into()
            } else {
                let tag = mon.tag();
                let count = tag.tiled.len() + 1;
                let area = layer_map_for_output(&mon.output).non_exclusive_zone();
                tag.layout
                    .compute_rects(count, area, &self.state.config.layout)
                    .last()
                    .map(|r| r.size)
                    .unwrap_or(area.size)
            };
            let tl = unmapped.window.toplevel().unwrap();
            if !floating {
                tl.with_pending_state(|s| s.size = Some(configured_size));
            }
            tl.send_configure();
            unmapped.placement = Some(Placement {
                floating,
                monitor: self.state.active_monitor,
                configured_size,
            });
            return None;
        }
        // phase 2: configure acked, check for buffer
        let has_buffer =
            with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false);
        if !has_buffer {
            return None;
        }

        let mut unmapped = self.state.unmapped.remove(&surface.id()).unwrap();
        unmapped.window.on_commit();
        let floating = unmapped.should_float();
        if let Some(p) = &mut unmapped.placement {
            p.floating |= floating;
        }
        let id = self.state.map(unmapped);
        let mon = self.state.windows[id].monitor;
        // recompute_layout must set tiled_geo before on_commit
        self.recompute_layout(mon);
        self.state.windows[id].on_commit();
        Some(mon)
    }

    fn on_layer_commit(&mut self, surface: &WlSurface) -> Option<usize> {
        for (i, mon) in self.state.monitors.iter().enumerate() {
            let mut map = layer_map_for_output(&mon.output);
            let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) else {
                continue;
            };
            let initial = with_states(surface, |s| {
                !s.data_map
                    .get::<LayerSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if initial {
                let serial = layer.layer_surface().send_configure();
                // Workaround for clients that batch the initial (empty) commit
                // and a buffer commit in the same socket write. Pre-set
                // last_acked so the pre_commit_hook accepts the buffer.
                with_states(surface, |s| {
                    s.data_map
                        .get::<LayerSurfaceData>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .last_acked = Some(LayerSurfaceConfigure {
                        serial,
                        state: LayerSurfaceState::default(),
                    });
                });
            }
            let changed = map.arrange();
            drop(map);
            self.state.monitors[i].update_exclusive_layer();
            return if changed { Some(i) } else { None };
        }
        None
    }
}

impl BufferHandler for Monotile {
    // No-op: smithay handles buffer cleanup via BufferHandler
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Monotile {
    fn shm_state(&self) -> &ShmState {
        &self.state.shm_state
    }
}

delegate_compositor!(Monotile);
delegate_shm!(Monotile);
