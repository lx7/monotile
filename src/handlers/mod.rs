// SPDX-License-Identifier: GPL-3.0-only
// Based on smithay's smallvil example (MIT licensed)

mod compositor;
mod layer_shell;
mod xdg_shell;

use crate::Monotile;
use smithay::{
    delegate_data_device, delegate_output, delegate_seat,
    input::{
        Seat, SeatHandler, SeatState,
        dnd::{DnDGrab, DndGrabHandler, GrabType, Source},
        pointer::Focus,
    },
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::Serial,
    wayland::{
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus,
            },
        },
    },
};

impl SeatHandler for Monotile {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Monotile> {
        &mut self.state.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
        // TODO: implement cursor_image()
    }

    // update data device (clipboard) access when the focus changes
    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.state.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
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

                // create a dnd grab to start the operation
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
