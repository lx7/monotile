// SPDX-License-Identifier: GPL-3.0-only

use smithay::output::Output;
use wayland_protocols_wlr::output_power_management::v1::server::{
    zwlr_output_power_manager_v1::{self, ZwlrOutputPowerManagerV1},
    zwlr_output_power_v1::{self, ZwlrOutputPowerV1},
};
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use crate::Monotile;

pub fn register_global(dh: &DisplayHandle) {
    dh.create_global::<Monotile, ZwlrOutputPowerManagerV1, _>(1, ());
}

impl GlobalDispatch<ZwlrOutputPowerManagerV1, ()> for Monotile {
    fn bind(
        _monotile: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputPowerManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputPowerManagerV1,
        request: zwlr_output_power_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_power_manager_v1::Request::GetOutputPower { id, output } => {
                let Some(output) = Output::from_resource(&output) else {
                    return;
                };
                let surface = match &mut monotile.backend {
                    crate::backend::Backend::Drm(drm) => {
                        drm.surfaces.values_mut().find(|s| s.output == output)
                    }
                    _ => None,
                };
                let Some(surface) = surface else { return };
                let handle = data_init.init(id, output);
                let mode = if surface.powered {
                    zwlr_output_power_v1::Mode::On
                } else {
                    zwlr_output_power_v1::Mode::Off
                };
                handle.mode(mode);
                surface.power_clients.push(handle.downgrade());
            }
            zwlr_output_power_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputPowerV1, Output> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputPowerV1,
        request: zwlr_output_power_v1::Request,
        output: &Output,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_power_v1::Request::SetMode { mode } => {
                let on = mode == WEnum::Value(zwlr_output_power_v1::Mode::On);
                monotile.backend.set_output_power(output, on);
            }
            zwlr_output_power_v1::Request::Destroy => {}
            _ => {}
        }
    }
}
