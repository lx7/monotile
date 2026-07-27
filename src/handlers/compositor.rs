// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    Monotile,
    shell::{Placement, SeatExt},
    state::ClientState,
};
use smithay::{
    backend::renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state},
    delegate_compositor, delegate_shm,
    desktop::{PopupKind, WindowSurfaceType, find_popup_root_surface, layer_map_for_output},
    output::Output,
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

        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }

        match self
            .on_window_commit(&root)
            .or_else(|| self.on_popup_commit(surface, &root))
            .or_else(|| self.on_unmapped_commit(&root))
            .or_else(|| self.on_layer_commit(&root))
            .or_else(|| self.on_lock_commit(&root))
            .or_else(|| self.on_cursor_commit(&root))
        {
            Some((output, true)) => self.recompute_layout(&output),
            Some((output, false)) => self.backend.schedule_render(&output),
            None => {}
        }
    }
}

impl Monotile {
    fn on_window_commit(&mut self, root: &WlSurface) -> Option<(Output, bool)> {
        let id = self.state.windows.find_by_surface(root)?;
        self.state.windows[id].on_commit();
        Some((self.state.windows[id].output.clone(), false))
    }

    fn on_popup_commit(&mut self, surface: &WlSurface, root: &WlSurface) -> Option<(Output, bool)> {
        self.state.popups.commit(surface);
        let popup = self.state.popups.find_popup(root)?;

        if let PopupKind::Xdg(ref xdg) = popup
            && !xdg.is_initial_configure_sent()
        {
            xdg.send_configure().expect("initial configure");
        }

        // window popup
        if let Ok(popup_root) = find_popup_root_surface(&popup)
            && let Some(id) = self.state.windows.find_by_surface(&popup_root)
        {
            self.state.windows[id].buffer_committed = true;
            return Some((self.state.windows[id].output.clone(), false));
        }

        // layer-shell popup
        // TODO for multi-monitor: resolve the layer surface's monitor
        Some((self.state.seat.active_output(), false))
    }

    /// Unmapped toplevel: two-phase configure/map state machine.
    fn on_unmapped_commit(&mut self, root: &WlSurface) -> Option<(Output, bool)> {
        let unmapped = self.state.unmapped.get_mut(&root.id())?;

        if unmapped.placement.is_none() {
            // phase 1: first commit - send configure with tiled size
            let floating = unmapped.should_float();
            let output = self.state.seat.active_output();
            let configured_size = if floating {
                (0, 0).into()
            } else {
                self.state
                    .monitors
                    .by_output(&output)
                    .map(|(_, m)| m)
                    .expect("the seat's active output is attached to a monitor")
                    .next_tiled_size()
            };
            unmapped.configure_initial(configured_size, !floating);
            unmapped.placement = Some(Placement {
                floating,
                output,
                configured_size,
            });
            return None;
        }
        // phase 2: configure acked, check for buffer
        let has_buffer =
            with_renderer_surface_state(root, |s| s.buffer().is_some()).unwrap_or(false);
        if !has_buffer {
            return None;
        }

        let mut unmapped = self.state.unmapped.remove(&root.id()).unwrap();
        // process the buffer commit before mapping
        unmapped.window.on_commit();
        let floating = unmapped.should_float();
        if let Some(p) = &mut unmapped.placement {
            p.floating |= floating;
        }
        let id = self.state.map(unmapped);
        Some((self.state.windows[id].output.clone(), true))
    }

    fn on_layer_commit(&mut self, root: &WlSurface) -> Option<(Output, bool)> {
        for mon in self.state.monitors.iter() {
            let mut map = layer_map_for_output(&mon.output);
            let Some(layer) = map.layer_for_surface(root, WindowSurfaceType::TOPLEVEL) else {
                continue;
            };
            let initial = with_states(root, |s| {
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
                with_states(root, |s| {
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
            return Some((mon.output.clone(), changed));
        }
        None
    }

    fn on_lock_commit(&mut self, root: &WlSurface) -> Option<(Output, bool)> {
        let mon = self.state.monitors.iter().find(|m| {
            m.lock_surface
                .as_ref()
                .is_some_and(|ls| ls.wl_surface() == root)
        })?;
        Some((mon.output.clone(), false))
    }

    fn on_cursor_commit(&mut self, root: &WlSurface) -> Option<(Output, bool)> {
        if self.state.cursor.on_commit(root) {
            Some((self.state.seat.pointer_output(), false))
        } else {
            None
        }
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
