// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    backend::Backend,
    shell::{Monitor, WindowId},
};
use smithay::{
    desktop::{PopupManager, Window},
    input::{Seat, SeatState},
    output::Output,
    reexports::{
        calloop::{
            EventLoop, Interest, LoopSignal, Mode as CalloopMode, PostAction, generic::Generic,
        },
        wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as KdeMode,
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::SERIAL_COUNTER,
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::{
            kde::decoration::KdeDecorationState,
            wlr_layer::WlrLayerShellState,
            xdg::{ToplevelSurface, XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};
use std::{ffi::OsString, os::unix::net::UnixStream, sync::Arc};

pub struct Monotile {
    pub backend: Backend,
    pub state: State,
}

impl Monotile {
    pub fn new() -> (EventLoop<'static, Monotile>, Self) {
        let event_loop: EventLoop<Monotile> = EventLoop::try_new().expect("event loop");
        let loop_handle = event_loop.handle();

        // insert event source to dispatch protocol messages from clients
        let display: Display<Monotile> = Display::new().unwrap();
        let display_handle = display.handle();
        let display_source = Generic::new(display, Interest::READ, CalloopMode::Level);
        loop_handle
            .insert_source(display_source, |_, display, monotile| {
                unsafe {
                    display.get_mut().dispatch_clients(monotile).unwrap();
                }
                Ok(PostAction::Continue)
            })
            .unwrap();

        let mut state = State::new(display_handle, event_loop.get_signal());

        // insert event source to accept new client connections on the Wayland socket
        let socket = ListeningSocketSource::new_auto().unwrap();
        state.socket = socket.socket_name().to_os_string();
        loop_handle
            .insert_source(socket, |stream, _, mt| mt.state.insert_client(stream))
            .unwrap();

        (
            event_loop,
            Self {
                backend: Backend::Unset,
                state,
            },
        )
    }

    // TODO: move to shell?
    pub fn update_focus(&mut self) {
        self.set_focus(self.state.mon().active_id());
    }

    // TODO: move to shell?
    pub fn set_focus(&mut self, id: Option<WindowId>) {
        let target = if let Some(surface) = self.state.mon().exclusive_layer_surface() {
            self.state.mon_mut().set_focus(None);
            Some(surface)
        } else {
            self.state.mon_mut().set_focus(id)
        };
        if let Some(kb) = self.state.seat.get_keyboard() {
            kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
    }
}

/// Core compositor state (everything except backend)
pub struct State {
    pub start_time: std::time::Instant,
    pub socket: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub kde_decoration_state: KdeDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Monotile>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,
    pub seat: Seat<Monotile>,
    pub monitors: Vec<Monitor>,
    pub active_monitor: usize,
    pub pending: Vec<Window>,
    pub key_bindings: Vec<crate::config::Key>,
}

impl State {
    pub fn new(dh: DisplayHandle, signal: LoopSignal) -> Self {
        let compositor_state = CompositorState::new::<Monotile>(&dh);
        let xdg_shell_state = XdgShellState::new::<Monotile>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Monotile>(&dh);
        let kde_decoration_state = KdeDecorationState::new::<Monotile>(&dh, KdeMode::Server);
        let layer_shell_state = WlrLayerShellState::new::<Monotile>(&dh);
        let shm_state = ShmState::new::<Monotile>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Monotile>(&dh);
        let data_device_state = DataDeviceState::new::<Monotile>(&dh);

        let mut seat_state = SeatState::new();
        // TODO: get seat name from backend
        let mut seat = seat_state.new_wl_seat(&dh, "winit");
        seat.add_keyboard(
            Default::default(),
            crate::config::REPEAT_DELAY,
            crate::config::REPEAT_RATE,
        )
        .unwrap();
        seat.add_pointer();

        Self {
            start_time: std::time::Instant::now(),
            socket: OsString::new(),
            display_handle: dh,
            loop_signal: signal,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            kde_decoration_state,
            layer_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups: PopupManager::default(),
            seat,
            monitors: Vec::new(),
            active_monitor: 0,
            pending: Vec::new(),
            key_bindings: crate::config::key_bindings(),
        }
    }

    // TODO: move to shell?
    pub fn mon(&self) -> &Monitor {
        &self.monitors[self.active_monitor]
    }

    // TODO: move to shell?
    pub fn mon_mut(&mut self) -> &mut Monitor {
        &mut self.monitors[self.active_monitor]
    }

    // TODO: move to shell?
    pub fn add_monitor(&mut self, output: Output) {
        self.monitors.push(Monitor::new(output));
    }

    pub fn find_pending(&self, surface: &WlSurface) -> Option<(usize, ToplevelSurface)> {
        for (i, w) in self.pending.iter().enumerate() {
            if let Some(tl) = w.toplevel()
                && tl.wl_surface() == surface
            {
                return Some((i, tl.clone()));
            }
        }
        None
    }

    pub fn insert_client(&mut self, stream: UnixStream) {
        self.display_handle
            .insert_client(stream, Arc::new(ClientState::default()))
            .unwrap();
    }

    pub fn flush_clients(&mut self) {
        let _ = self.display_handle.flush_clients();
    }
}

/// Data associated with a wayland client.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
