// SPDX-License-Identifier: GPL-3.0-only
// Based on smithay's smallvil example (MIT licensed)

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
        shell::wlr_layer::LayerSurfaceData,
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
            if let Some(we) = self.state.mon().find_window_by_surface(&root) {
                we.window.on_commit();
            }
        };

        let window_mapped = xdg_shell::handle_commit(&mut self.state, surface);
        let layer_changed = self.handle_layer_commit(surface);
        if window_mapped || layer_changed {
            self.update_focus();
        }

        self.backend.schedule_render(&self.state.mon().output);
    }
}

impl Monotile {
    fn handle_layer_commit(&mut self, surface: &WlSurface) -> bool {
        let output = self.state.mon().output.clone();
        let mut map = layer_map_for_output(&output);
        if let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
            let sent = with_states(surface, |s| {
                let mutex = s.data_map.get::<LayerSurfaceData>().unwrap();
                mutex.lock().unwrap().initial_configure_sent
            });
            if !sent {
                layer.layer_surface().send_configure();
            }
        }
        let changed = map.arrange();
        drop(map);
        if changed {
            self.state.mon_mut().recompute_layout();
        }
        changed
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

// TODO: implement dmabuf

delegate_compositor!(Monotile);
delegate_shm!(Monotile);
