// SPDX-License-Identifier: GPL-3.0-only

pub mod drm;
pub mod winit;

use smithay::{backend::renderer::glow::GlowRenderer, output::Output};
use winit::WinitState;

use self::drm::DrmState;

/// Enum over all supported backends
#[derive(Debug)]
pub enum Backend {
    Winit(WinitState),
    Drm(DrmState),
    Unset,
}

impl Backend {
    pub fn schedule_render(&mut self, _output: &Output) {
        match self {
            Backend::Winit(_) => {
                // no-op: winit renders continuously via input/redraw events
            }
            Backend::Drm(drm) => drm.schedule_render(_output),
            Backend::Unset => {} // no-op (tests)
        }
    }

    pub fn winit(&mut self) -> &mut WinitState {
        match self {
            Backend::Winit(winit) => winit,
            _ => panic!("called winit() on non-winit backend"),
        }
    }

    pub fn drm(&mut self) -> &mut DrmState {
        match self {
            Backend::Drm(drm) => drm,
            _ => panic!("called drm() on non-drm backend"),
        }
    }

    pub fn renderer(&mut self) -> &mut GlowRenderer {
        match self {
            Backend::Winit(winit) => winit.backend.renderer(),
            Backend::Drm(drm) => &mut drm.renderer,
            Backend::Unset => panic!("called renderer() on unset backend"),
        }
    }
}
