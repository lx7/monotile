// SPDX-License-Identifier: GPL-3.0-only

pub mod drm;
pub mod winit;

use smithay::{
    backend::{renderer::glow::GlowRenderer, session::Session},
    output::Output,
};
use tracing::warn;
use winit::WinitState;

use self::drm::DrmState;
use crate::{config::Config, input::configure_device, shell::Monitors};

/// Enum over all supported backends
#[derive(Debug)]
pub enum Backend {
    Winit(WinitState),
    Drm(DrmState),
    Unset,
}

impl Backend {
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

    pub fn schedule_render_all(&mut self) {
        if let Backend::Drm(drm) = self {
            drm.schedule_render_all();
        }
    }

    pub fn schedule_render(&mut self, output: &Output) {
        match self {
            Backend::Winit(_) => {
                // no-op: winit renders continuously via input/redraw events
            }
            Backend::Drm(drm) => {
                drm.schedule_render(output);
            }
            Backend::Unset => {} // no-op (tests)
        }
    }

    pub fn set_output_power(&mut self, output: &Output, on: bool) {
        if let Backend::Drm(drm) = self {
            drm.set_output_power(output, on);
        }
    }

    pub fn set_all_outputs_power(&mut self, on: bool) {
        if let Backend::Drm(drm) = self {
            drm.set_all_outputs_power(on);
        }
    }

    pub fn any_output_off(&self) -> bool {
        match self {
            Backend::Drm(drm) => drm.any_output_off(),
            _ => false,
        }
    }

    pub fn change_vt(&mut self, vt: i32) {
        if let Backend::Drm(drm) = self {
            if let Err(err) = drm.session.change_vt(vt) {
                warn!("failed to switch VT: {err}");
            }
        }
    }

    pub fn apply_output_settings(&mut self, monitors: &Monitors) {
        if let Backend::Drm(drm) = self {
            drm.apply_output_settings(monitors);
        }
    }

    pub fn reconfigure_devices(&mut self, config: &Config) {
        if let Backend::Drm(drm) = self {
            for dev in &mut drm.input_devices {
                configure_device(dev, config);
            }
        }
    }
}
