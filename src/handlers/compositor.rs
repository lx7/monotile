// SPDX-License-Identifier: GPL-3.0-only

use super::xdg_shell;
use crate::{Monotile, state::ClientState};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_shm,
    desktop::{WindowSurfaceType, layer_map_for_output},
    reexports::wayland_server::{
        Client,
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

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(id) = self.state.windows.find_by_surface(&root) {
                self.state.windows[id].on_commit();
            }
        };

        let window_mapped = xdg_shell::handle_commit(&mut self.state, surface);
        let layer_changed = self.handle_layer_commit(surface);
        if window_mapped || layer_changed {
            self.recompute_layout(self.state.active_monitor);
        }
        self.backend.schedule_render(&self.state.mon().output);
    }
}

impl Monotile {
    fn handle_layer_commit(&mut self, surface: &WlSurface) -> bool {
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
                // INFO: This is a workawound for clients that batch the
                // initial (empty) commit and a buffer commit in the same
                // socket write. The protocol says there must be an ack
                // before attaching a buffer, so this is a violation of the
                // protocol.
                // The server processes both commits before the configure
                // round-trips. To fix this, pre-set last_acked so the
                // pre_commit_hook accepts the buffer.
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
            return changed;
        }
        false
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
