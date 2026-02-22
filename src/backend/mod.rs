// SPDX-License-Identifier: GPL-3.0-only

pub mod winit;

use smithay::output::Output;
use winit::WinitState;

/// Enum over all supported backends
#[derive(Debug)]
pub enum Backend {
    Winit(WinitState),
    // Drm(DrmState),  // TODO: implement DRM backend
    Unset,
}

impl Backend {
    pub fn schedule_render(&mut self, _output: &Output) {
        match self {
            Backend::Winit(_) => {
                // no-op: winit renders continuously via input/redraw events
            }
            // Backend::Drm(drm) => drm.schedule_render(output),
            Backend::Unset => {} // no-op (tests)
        }
    }

    pub fn winit(&mut self) -> &mut WinitState {
        match self {
            Backend::Winit(winit) => winit,
            _ => panic!("walled winit() on non-winit backend"),
        }
    }
}
