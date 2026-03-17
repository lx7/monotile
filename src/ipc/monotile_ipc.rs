// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use smithay::output::Output;
use wayland_server::backend::protocol::WEnumError;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
};

use super::TagSnapshot;
use super::monotile_ipc_protocol::{
    zmonotile_control_v1::ZmonotileControlV1,
    zmonotile_output_status_v1::ZmonotileOutputStatusV1,
    zmonotile_seat_control_v1::{self as proto, ZmonotileSeatControlV1},
    zmonotile_seat_status_v1::ZmonotileSeatStatusV1,
    zmonotile_status_manager_v1::ZmonotileStatusManagerV1,
};
use crate::Monotile;
use crate::config::{Action, Direction, Rel};
use crate::spawn::spawn_shell;

// -- State --

pub struct MonotileIpcState {
    outputs: HashMap<Output, Vec<Weak<ZmonotileOutputStatusV1>>>,
    seats: Vec<Weak<ZmonotileSeatStatusV1>>,
}

impl MonotileIpcState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<Monotile, ZmonotileStatusManagerV1, _>(1, ());
        dh.create_global::<Monotile, ZmonotileControlV1, _>(1, ());
        Self {
            outputs: HashMap::new(),
            seats: Vec::new(),
        }
    }

    pub fn add_output(&mut self, output: &Output, handle: &ZmonotileOutputStatusV1) {
        self.outputs
            .entry(output.clone())
            .or_default()
            .push(handle.downgrade());
    }

    pub fn notify_output(&mut self, output: &Output, snap: &TagSnapshot) {
        let handles = match self.outputs.get_mut(output) {
            Some(h) => h,
            None => return,
        };
        handles.retain(|w| {
            let Some(h) = w.upgrade().ok() else {
                return false;
            };
            send_output_status(&h, snap);
            true
        });
    }

    pub fn notify_seat(&mut self, snap: &TagSnapshot, output: &Output) {
        self.seats.retain(|w| {
            let Some(h) = w.upgrade().ok() else {
                return false;
            };
            send_seat_status(&h, snap, output);
            true
        });
    }
}

fn send_output_status(h: &ZmonotileOutputStatusV1, snap: &TagSnapshot) {
    h.focused_tags(snap.focused_tags);
    h.occupied_tags(snap.occupied_tags);
    h.urgent_tags(snap.urgent_tags);
    h.layout(snap.layout_name.clone(), snap.layout_symbol.clone());
}

fn send_seat_status(h: &ZmonotileSeatStatusV1, snap: &TagSnapshot, output: &Output) {
    if let Some(client) = h.client() {
        if let Some(wl_output) = output.client_outputs(&client).next() {
            h.focused_output(&wl_output);
        }
    }
    let title = if snap.title.is_empty() {
        None
    } else {
        Some(snap.title.clone())
    };
    h.focused_toplevel(
        title,
        snap.app_id.clone(),
        snap.fullscreen as u32,
        snap.floating as u32,
    );
}

// -- Status Manager (factory) --

impl GlobalDispatch<ZmonotileStatusManagerV1, ()> for Monotile {
    fn bind(
        _monotile: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZmonotileStatusManagerV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZmonotileStatusManagerV1, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZmonotileStatusManagerV1,
        request: <ZmonotileStatusManagerV1 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use super::monotile_ipc_protocol::zmonotile_status_manager_v1::Request;
        match request {
            Request::GetOutputStatus { id, output } => {
                let Some(output) = Output::from_resource(&output) else {
                    return;
                };
                let handle = data_init.init(id, ());
                monotile.state.ipc.monotile.add_output(&output, &handle);

                if let Some((_, mon)) = monotile.state.monitors.by_output(&output) {
                    // send tag metadata
                    handle.tag_count(mon.tag_names.len() as u32);
                    for (i, name) in mon.tag_names.iter().enumerate() {
                        handle.tag_info(i as u32, name.clone());
                    }
                    // send initial state
                    let snap = mon.snapshot(&monotile.state.windows);
                    send_output_status(&handle, &snap);
                }
            }
            Request::GetSeatStatus { id, seat: _ } => {
                let handle = data_init.init(id, ());
                monotile.state.ipc.monotile.seats.push(handle.downgrade());

                // TODO: get monitor from seat when multiseat is implemented
                // send initial state
                let mon = monotile.state.mon();
                let snap = mon.snapshot(&monotile.state.windows);
                send_seat_status(&handle, &snap, &mon.output);
            }
            Request::Destroy => {}
        }
    }
}

// -- Output Status (events only) --

impl Dispatch<ZmonotileOutputStatusV1, ()> for Monotile {
    fn request(
        _monotile: &mut Self,
        _client: &Client,
        _resource: &ZmonotileOutputStatusV1,
        request: <ZmonotileOutputStatusV1 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use super::monotile_ipc_protocol::zmonotile_output_status_v1::Request;
        match request {
            Request::Destroy => {}
        }
    }
}

// -- Seat Status (events only) --

impl Dispatch<ZmonotileSeatStatusV1, ()> for Monotile {
    fn request(
        _monotile: &mut Self,
        _client: &Client,
        _resource: &ZmonotileSeatStatusV1,
        request: <ZmonotileSeatStatusV1 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use super::monotile_ipc_protocol::zmonotile_seat_status_v1::Request;
        match request {
            Request::Destroy => {}
        }
    }
}

// -- Control (factory) --

impl GlobalDispatch<ZmonotileControlV1, ()> for Monotile {
    fn bind(
        _monotile: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZmonotileControlV1>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZmonotileControlV1, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZmonotileControlV1,
        request: <ZmonotileControlV1 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use super::monotile_ipc_protocol::zmonotile_control_v1::Request;
        match request {
            Request::GetSeatControl { id, seat: _ } => {
                data_init.init(id, ());
            }
            Request::Spawn { command } => {
                spawn_shell(&command);
            }
            Request::ReloadConfig => {
                monotile.handle_action(Action::ReloadConfig);
            }
            Request::Exit => {
                monotile.handle_action(Action::Exit);
            }
            Request::Destroy => {}
        }
    }
}

// -- Seat Control --

impl Dispatch<ZmonotileSeatControlV1, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZmonotileSeatControlV1,
        request: <ZmonotileSeatControlV1 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use super::monotile_ipc_protocol::zmonotile_seat_control_v1::Request;

        let action = match request {
            Request::FocusTag { index } => Action::FocusTag(index as usize),
            Request::FocusPreviousTag => Action::FocusPrevTag,
            Request::SetToplevelTag { index } => Action::SetTag(index as usize),
            Request::ToggleToplevelTag { index } => Action::ToggleTag(index as usize),
            Request::FocusToplevel { position } => {
                let Ok(pos) = Rel::try_from(position) else {
                    return;
                };
                Action::Focus(pos)
            }
            Request::Swap { position } => {
                let Ok(pos) = Rel::try_from(position) else {
                    return;
                };
                Action::Swap(pos)
            }
            Request::Close => Action::Close,
            Request::ToggleFloat => Action::ToggleFloat,
            Request::ToggleFullscreen => Action::ToggleFullscreen,
            Request::FocusOutput { direction } => {
                let Ok(dir) = Direction::try_from(direction) else {
                    return;
                };
                Action::FocusOutput(dir)
            }
            Request::SendToOutput { direction } => {
                let Ok(dir) = Direction::try_from(direction) else {
                    return;
                };
                Action::SendToOutput(dir)
            }
            Request::AdjustMainCount { delta } => Action::AdjustMainCount(delta),
            Request::SetMainCount { count } => Action::SetMainCount(count as usize),
            Request::AdjustMainRatio { delta } => Action::AdjustMainRatio(f64::from(delta) as f32),
            Request::SetMainRatio { ratio } => Action::SetMainRatio(f64::from(ratio) as f32),
            Request::Destroy => return,
        };
        monotile.handle_action(action);
    }
}

impl TryFrom<WEnum<proto::Position>> for Rel {
    type Error = WEnumError;
    fn try_from(w: WEnum<proto::Position>) -> Result<Self, WEnumError> {
        match w.into_result()? {
            proto::Position::Next => Ok(Self::Next),
            proto::Position::Previous => Ok(Self::Prev),
            proto::Position::First => Ok(Self::First),
            proto::Position::Last => Ok(Self::Last),
        }
    }
}

impl TryFrom<WEnum<proto::Direction>> for Direction {
    type Error = WEnumError;
    fn try_from(w: WEnum<proto::Direction>) -> Result<Self, WEnumError> {
        match w.into_result()? {
            proto::Direction::Up => Ok(Self::Up),
            proto::Direction::Down => Ok(Self::Down),
            proto::Direction::Left => Ok(Self::Left),
            proto::Direction::Right => Ok(Self::Right),
        }
    }
}
