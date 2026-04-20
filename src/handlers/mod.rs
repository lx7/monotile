// SPDX-License-Identifier: GPL-3.0-only

mod compositor;
mod dmabuf;
pub mod foreign_toplevel;
mod idle_notifier;
mod layer_shell;
pub mod output_power;
pub mod screencopy;
mod session_lock;
mod xdg_shell;

use std::cell::RefCell;

use crate::Monotile;
use smithay::{
    backend::input::DeviceCapability,
    delegate_cursor_shape, delegate_data_control, delegate_data_device, delegate_ext_data_control,
    delegate_output, delegate_primary_selection, delegate_seat, delegate_single_pixel_buffer,
    delegate_viewporter, delegate_xdg_activation,
    input::{
        Seat, SeatHandler, SeatState,
        dnd::{DnDGrab, DndGrabHandler, GrabType, Source},
        keyboard::LedState,
        pointer::{CursorImageStatus, Focus},
    },
    reexports::input::Device,
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::Serial,
    wayland::{
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus,
            },
            ext_data_control::{
                DataControlHandler as ExtDataControlHandler,
                DataControlState as ExtDataControlState,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
            },
            wlr_data_control::{
                DataControlHandler as WlrDataControlHandler,
                DataControlState as WlrDataControlState,
            },
        },
        tablet_manager::TabletSeatHandler,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};

#[derive(Default)]
pub struct Devices(pub RefCell<Vec<Device>>);

impl SeatHandler for Monotile {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Monotile> {
        &mut self.state.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        if let CursorImageStatus::Named(icon) = &image
            && icon.name().contains("resize")
        {
            return;
        }
        self.state.cursor.status = image;
        self.backend.schedule_render(&self.state.mon().output);
    }

    fn led_state_changed(&mut self, seat: &Seat<Self>, led_state: LedState) {
        let devices = seat.user_data().get::<Devices>().unwrap();
        for dev in devices.0.borrow_mut().iter_mut() {
            if dev.has_capability(DeviceCapability::Keyboard.into()) {
                dev.led_update(led_state.into());
            }
        }
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.state.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }
}
delegate_seat!(Monotile);

impl SelectionHandler for Monotile {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Monotile {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.state.data_device_state
    }
}
delegate_data_device!(Monotile);

impl PrimarySelectionHandler for Monotile {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.state.primary_selection_state
    }
}
delegate_primary_selection!(Monotile);

impl WlrDataControlHandler for Monotile {
    fn data_control_state(&mut self) -> &mut WlrDataControlState {
        &mut self.state.wlr_data_control_state
    }
}
delegate_data_control!(Monotile);

impl ExtDataControlHandler for Monotile {
    fn data_control_state(&mut self) -> &mut ExtDataControlState {
        &mut self.state.ext_data_control_state
    }
}
delegate_ext_data_control!(Monotile);

impl DndGrabHandler for Monotile {}
impl WaylandDndGrabHandler for Monotile {
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        _icon: Option<WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        match type_ {
            GrabType::Pointer => {
                let ptr = seat.get_pointer().unwrap();
                let start_data = ptr.grab_start_data().unwrap();

                let grab =
                    DnDGrab::new_pointer(&self.state.display_handle, start_data, source, seat);
                ptr.set_grab(self, grab, serial, Focus::Keep);
            }
            GrabType::Touch => {
                // monotile doesn't support touch
                source.cancel();
            }
        }
    }
}

impl OutputHandler for Monotile {}
delegate_output!(Monotile);

delegate_viewporter!(Monotile);
delegate_single_pixel_buffer!(Monotile);

impl XdgActivationHandler for Monotile {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.state.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let Some(id) = self.state.windows.find_by_surface(&surface) else {
            return;
        };
        if self.state.windows[id].focused {
            return;
        }
        self.state.windows[id].urgent = true;
        self.state.windows[id].resolve_render();
        self.state.ipc.dirty = true;
        self.backend
            .schedule_render(&self.state.monitors[self.state.windows[id].monitor].output);
    }
}
delegate_xdg_activation!(Monotile);

impl TabletSeatHandler for Monotile {}
delegate_cursor_shape!(Monotile);
smithay::delegate_pointer_gestures!(Monotile);
