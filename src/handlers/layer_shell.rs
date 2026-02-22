// SPDX-License-Identifier: GPL-3.0-only
// Based on smithay's smallvil example (MIT licensed)

use crate::Monotile;
use smithay::{
    delegate_layer_shell,
    desktop::{LayerSurface, PopupKind, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_output,
    wayland::shell::{
        wlr_layer::{
            Layer, LayerSurface as WlrLayerSurface, WlrLayerShellHandler, WlrLayerShellState,
        },
        xdg::PopupSurface,
    },
};

impl WlrLayerShellHandler for Monotile {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.state.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.state.mon().output.clone());
        let mut map = layer_map_for_output(&output);
        let layer = LayerSurface::new(surface, namespace);
        map.map_layer(&layer).unwrap();
        drop(map);
        self.update_focus();
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
        let _ = self.state.popups.track_popup(PopupKind::Xdg(popup));
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let output = self.state.mon().output.clone();
        let mut map = layer_map_for_output(&output);
        let layer = map
            .layers()
            .find(|l| l.layer_surface() == &surface)
            .cloned();
        if let Some(layer) = layer {
            map.unmap_layer(&layer);
        }
        drop(map);
        self.state.mon_mut().recompute_layout();
        self.update_focus();
    }
}

delegate_layer_shell!(Monotile);
