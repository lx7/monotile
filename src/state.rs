// SPDX-License-Identifier: GPL-3.0-only

use std::{ffi::OsString, os::unix::net::UnixStream, sync::Arc};

use tracing::info;

use smithay::{
    desktop::{PopupManager, Window, WindowSurfaceType, layer_map_for_output},
    input::{Seat, SeatState, keyboard::XkbConfig},
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
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::{
            kde::decoration::KdeDecorationState,
            wlr_layer::{Layer, WlrLayerShellState},
            xdg::{ToplevelSurface, XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::{
    backend::Backend,
    config::Config,
    render::cursor::CursorManager,
    shell::{Monitor, WindowElement, WindowId, Windows},
};

pub struct Monotile {
    pub backend: Backend,
    pub state: State,
}

impl Monotile {
    pub fn new(config: Config) -> (EventLoop<'static, Monotile>, Self) {
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

        let mut state = State::new(display_handle, event_loop.get_signal(), config);

        // insert event source to accept new client connections on the Wayland socket
        let socket = ListeningSocketSource::new_auto().unwrap();
        state.socket = socket.socket_name().to_os_string();
        info!("listening on {}", state.socket.to_string_lossy());
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

    pub fn recompute_layout(&mut self) {
        let config = &self.state.config;
        let mon = &mut self.state.monitors[self.state.active_monitor];
        mon.recompute_layout(&mut self.state.windows, config);
        self.state.windows.configure_visible(mon.tag());
        self.update_focus();
    }

    pub fn update_focus(&mut self) {
        self.set_focus(self.state.mon().tag().focused_id());
    }

    pub fn set_focus(&mut self, id: Option<WindowId>) {
        if let Some(old) = self.state.mon().tag().focused_id() {
            if let Some(we) = self.state.windows.get_mut(old) {
                we.set_focused(false);
            }
        }

        if let Some(surface) = self.state.mon().exclusive_layer_surface() {
            if let Some(kb) = self.state.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
            return;
        }

        let target = id
            .and_then(|id| {
                self.state.mon_mut().tag_mut().focus(id);
                self.state.windows.get_mut(id)
            })
            .and_then(|we| {
                we.set_focused(true);
                we.window.toplevel().map(|tl| tl.wl_surface().clone())
            });

        if let Some(kb) = self.state.seat.get_keyboard() {
            kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
    }
}

pub struct State {
    pub config: Config,
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
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub popups: PopupManager,
    pub seat: Seat<Monotile>,
    pub cursor_shape_state: CursorShapeManagerState,
    pub cursor: CursorManager,
    pub windows: Windows,
    pub monitors: Vec<Monitor>,
    // TODO: active_monitor should be derived, not stored.
    // Every lookup (render, map, unmap, focus, layout) really needs
    // "monitor for this output/window/pointer location", not "active".
    // Remove this index when multi-monitor is implemented.
    pub active_monitor: usize,
    pub pending: Vec<Window>,
}

impl State {
    pub fn new(dh: DisplayHandle, signal: LoopSignal, config: Config) -> Self {
        let compositor_state = CompositorState::new::<Monotile>(&dh);
        let xdg_shell_state = XdgShellState::new::<Monotile>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Monotile>(&dh);
        let kde_decoration_state = KdeDecorationState::new::<Monotile>(&dh, KdeMode::Server);
        let layer_shell_state = WlrLayerShellState::new::<Monotile>(&dh);
        let shm_state = ShmState::new::<Monotile>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Monotile>(&dh);
        let data_device_state = DataDeviceState::new::<Monotile>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        let kb = &config.input.keyboard;
        seat.add_keyboard(
            XkbConfig {
                layout: &kb.layout,
                variant: &kb.variant,
                options: Some(kb.options.clone()).filter(|s| !s.is_empty()),
                ..Default::default()
            },
            kb.repeat_delay,
            kb.repeat_rate,
        )
        .unwrap();
        seat.add_pointer();
        info!("keyboard: layout={} variant={}", kb.layout, kb.variant);

        let cursor_shape_state = CursorShapeManagerState::new::<Monotile>(&dh);
        let cursor = CursorManager::new(1.0);

        Self {
            config,
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
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            popups: PopupManager::default(),
            seat,
            cursor_shape_state,
            cursor,
            windows: Windows::default(),
            monitors: Vec::new(),
            active_monitor: 0,
            pending: Vec::new(),
        }
    }

    pub fn mon(&self) -> &Monitor {
        &self.monitors[self.active_monitor]
    }

    pub fn mon_mut(&mut self) -> &mut Monitor {
        &mut self.monitors[self.active_monitor]
    }

    pub fn add_monitor(&mut self, output: Output) {
        self.monitors.push(Monitor::new(output, &self.config));
    }

    pub fn monitor_idx(&self, name: &str) -> usize {
        self.monitors
            .iter()
            .position(|m| m.output.name() == name)
            .unwrap_or(self.active_monitor)
    }

    pub fn map(&mut self, window: Window, should_float: bool) -> WindowId {
        let rules = &self.config.windows;
        let id = self
            .windows
            .insert_with_key(|id| WindowElement::new(id, window, should_float, rules));
        let (output, tags) = self.windows[id].resolve_init();
        self.windows[id].resolve_render();

        let idx = output.map_or(self.active_monitor, |n| self.monitor_idx(&n));
        self.monitors[idx].map(&mut self.windows, id, tags);
        id
    }

    pub fn unmap(&mut self, id: WindowId) {
        let mon = &mut self.monitors[self.active_monitor];
        mon.unmap(&mut self.windows, id)
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let mon = self.mon();
        let map = layer_map_for_output(&mon.output);
        let layer_hit = |layer| {
            let layer = map.layer_under(layer, pos)?;
            let geo = map.layer_geometry(layer).unwrap();
            let rel = pos - geo.loc.to_f64();
            let (s, point) = layer.surface_under(rel, WindowSurfaceType::ALL)?;
            Some((s, (point + geo.loc).to_f64()))
        };

        // overlay / top layers
        if let Some(hit) = layer_hit(Layer::Overlay).or_else(|| layer_hit(Layer::Top)) {
            return Some(hit);
        }

        // windows
        let we = self.windows.window_under(mon.tag(), pos);
        if let Some(we) = we {
            let loc = we.geo().loc - we.window.geometry().loc;
            let rel = pos - loc.to_f64();
            if let Some((s, point)) = we.window.surface_under(rel, WindowSurfaceType::ALL) {
                return Some((s, (point + loc).to_f64()));
            }
        }

        // bottom / background layers
        layer_hit(Layer::Bottom).or_else(|| layer_hit(Layer::Background))
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

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
