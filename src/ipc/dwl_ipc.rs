// SPDX-License-Identifier: GPL-3.0-only

// This is the compatibility layer for dwl-ipc-unstable-v2
// monotile-ipc-unstable-v1 is the primary protocol, conversion logic lives here.

use smithay::output::Output;
use std::collections::HashMap;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
};

use super::TagSnapshot;
use super::dwl_ipc_protocol::{
    zdwl_ipc_manager_v2::ZdwlIpcManagerV2,
    zdwl_ipc_output_v2::{self, ZdwlIpcOutputV2},
};
use crate::{
    Monotile,
    config::{self, Action},
    shell::Monitor,
};

#[derive(Default)]
pub struct DwlIpcState {
    outputs: HashMap<Output, Vec<Weak<ZdwlIpcOutputV2>>>,
}

fn tag_state(active: bool, urgent: bool) -> zdwl_ipc_output_v2::TagState {
    match (active, urgent) {
        (_, true) => zdwl_ipc_output_v2::TagState::Urgent,
        (true, _) => zdwl_ipc_output_v2::TagState::Active,
        _ => zdwl_ipc_output_v2::TagState::None,
    }
}

fn send_snapshot(handle: &ZdwlIpcOutputV2, snap: &TagSnapshot, mon: &Monitor, active_mon: bool) {
    for (i, t) in mon.tags.iter().enumerate() {
        let active = i == snap.active_tag;
        let urgent = snap.urgent_tags & (1 << i) != 0;
        let state = tag_state(active, urgent);
        let clients = t.focus_stack.len() as u32;
        let focused = (snap.focused_tags >> i) & 1;
        handle.tag(i as u32, state, clients, focused);
    }
    handle.layout(0);
    handle.layout_symbol(snap.layout_symbol.clone());
    handle.title(snap.title.clone());
    handle.appid(snap.app_id.clone());
    handle.active(u32::from(active_mon));
    if handle.version() >= 2 {
        handle.fullscreen(u32::from(snap.fullscreen));
        handle.floating(u32::from(snap.floating));
    }
    handle.frame();
}

impl DwlIpcState {
    pub fn new(dh: &DisplayHandle) -> Self {
        let tag_count = config::default_tags().len() as u32;
        dh.create_global::<Monotile, ZdwlIpcManagerV2, _>(2, DwlIpcManagerData { tag_count });
        Self {
            outputs: HashMap::new(),
        }
    }

    pub fn add_output(&mut self, output: &Output, handle: &ZdwlIpcOutputV2) {
        self.outputs
            .entry(output.clone())
            .or_default()
            .push(handle.downgrade());
    }

    pub fn notify(&mut self, mon: &Monitor, snap: &TagSnapshot, active_mon: bool) {
        let handles = match self.outputs.get_mut(&mon.output) {
            Some(h) => h,
            None => return,
        };

        handles.retain(|weak| {
            let Some(handle) = weak.upgrade().ok() else {
                return false;
            };
            send_snapshot(&handle, snap, mon, active_mon);
            true
        });
    }
}

pub struct DwlIpcManagerData {
    tag_count: u32,
}

impl GlobalDispatch<ZdwlIpcManagerV2, DwlIpcManagerData> for Monotile {
    fn bind(
        _monotile: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZdwlIpcManagerV2>,
        data: &DwlIpcManagerData,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let mgr = data_init.init(resource, ());
        mgr.tags(data.tag_count);
        mgr.layout("tile".to_string());
    }
}

impl Dispatch<ZdwlIpcManagerV2, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZdwlIpcManagerV2,
        request: <ZdwlIpcManagerV2 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use super::dwl_ipc_protocol::zdwl_ipc_manager_v2::Request;
        match request {
            Request::GetOutput { id, output } => {
                let Some(output) = Output::from_resource(&output) else {
                    return;
                };
                let handle = data_init.init(id, ());
                monotile.state.ipc.dwl.add_output(&output, &handle);

                // send initial state
                if let Some((i, mon)) = monotile.state.monitors.by_output(&output) {
                    let active = i == monotile.state.active_monitor;
                    let snap = mon.snapshot(&monotile.state.windows);
                    send_snapshot(&handle, &snap, mon, active);
                }
            }
            Request::Release => {}
        }
    }
}

impl Dispatch<ZdwlIpcOutputV2, ()> for Monotile {
    fn request(
        monotile: &mut Self,
        _client: &Client,
        _resource: &ZdwlIpcOutputV2,
        request: <ZdwlIpcOutputV2 as Resource>::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use super::dwl_ipc_protocol::zdwl_ipc_output_v2::Request;
        match request {
            Request::SetTags {
                tagmask,
                toggle_tagset,
            } => {
                if toggle_tagset != 0 {
                    monotile.handle_action(Action::FocusPrevTag);
                } else {
                    let tag = tagmask.trailing_zeros() as usize;
                    monotile.handle_action(Action::FocusTag(tag));
                }
            }
            Request::SetClientTags { and_tags, xor_tags } => {
                let tag = xor_tags.trailing_zeros() as usize;
                if and_tags == 0 {
                    monotile.handle_action(Action::SetTag(tag));
                } else {
                    monotile.handle_action(Action::ToggleTag(tag));
                }
            }
            // TODO: implement layout switching
            Request::SetLayout { .. } => {}
            Request::Release => {}
        }
    }
}
