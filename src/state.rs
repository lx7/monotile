// SPDX-License-Identifier: GPL-3.0-only

use std::{ffi::OsString, os::unix::net::UnixStream, sync::Arc};

use tracing::{info, warn};

use smithay::{
    desktop::{PopupManager, Window, WindowSurfaceType, layer_map_for_output},
    input::{Seat, SeatState},
    output::Output,
    reexports::{
        calloop::{
            EventLoop, Interest, LoopHandle, LoopSignal, Mode as CalloopMode, PostAction,
            generic::Generic,
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
        idle_inhibit::IdleInhibitManagerState,
        idle_notify::IdleNotifierState,
        output::OutputManagerState,
        selection::{
            data_device::DataDeviceState,
            ext_data_control::DataControlState as ExtDataControlState,
            primary_selection::PrimarySelectionState,
            wlr_data_control::DataControlState as WlrDataControlState,
        },
        session_lock::SessionLockManagerState,
        shell::{
            kde::decoration::KdeDecorationState,
            wlr_layer::{Layer, WlrLayerShellState},
            xdg::{ToplevelSurface, XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        single_pixel_buffer::SinglePixelBufferState,
        socket::ListeningSocketSource,
        viewporter::ViewporterState,
    },
};

use crate::{
    backend::Backend,
    config::Config,
    handlers::screencopy::ScreencopyState,
    ipc::IpcState,
    render::cursor::CursorManager,
    shell::{Monitor, MonitorSettings, Monitors, Tag, WindowElement, WindowId, Windows},
    spawn::notify,
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

        let mut state = State::new(
            display_handle,
            loop_handle.clone(),
            event_loop.get_signal(),
            config,
        );

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

    // TODO: decide how to do recompute_layout for all monitors.
    // Do we need recompute_layout for the active monitors, or always recompute all?
    pub fn recompute_layout(&mut self) {
        let config = &self.state.config;
        let mon = &mut self.state.monitors[self.state.active_monitor];
        mon.recompute_layout(&mut self.state.windows, config);
        self.state.windows.configure_visible(mon.tag());
        self.update_focus();
    }

    pub fn reload_config(&mut self) {
        let path = self.state.config.path.clone();
        let config = match Config::load(Some(path)) {
            Ok(c) => {
                notify("normal", "config", &format!("reloaded"));
                c
            }
            Err(e) => {
                warn!("config reload failed: {e}");
                notify("critical", "config", &format!("reload failed: {e}"));
                return;
            }
        };

        let seat_conf = &config.seats["seat0"];
        let kb_changed = seat_conf.keyboard != self.state.config.seats["seat0"].keyboard;
        if kb_changed {
            let kb_conf = &seat_conf.keyboard;
            let kb = self.state.seat.get_keyboard().unwrap();
            let _ = kb.set_xkb_config(self, kb_conf.xkb_config());
            kb.change_repeat_info(kb_conf.repeat_rate, kb_conf.repeat_delay);
            info!(
                "keyboard: layout={} variant={}",
                kb_conf.layout, kb_conf.variant
            );
        }

        self.state.config = config;
        self.state.windows.update_rules(&self.state.config.windows);
        self.state.monitors.update_rules(&self.state.config.outputs);
        self.backend.apply_output_settings(&self.state.monitors);
        self.backend.reconfigure_devices(&self.state.config);
        for i in 0..self.state.monitors.len() {
            self.state.monitors[i].recompute_layout(&mut self.state.windows, &self.state.config);
            self.state.windows.configure_visible(self.state.monitors[i].tag());
        }
        self.update_focus();
        for mon in self.state.monitors.iter() {
            self.backend.schedule_render(&mon.output);
        }

        info!("config reloaded");
    }

    pub fn update_focus(&mut self) {
        self.set_focus(self.state.mon().tag().focused_id());
    }

    pub fn set_focus(&mut self, id: Option<WindowId>) {
        for we in self.state.windows.values_mut() {
            if we.focused {
                we.set_focused(false);
            }
        }

        if let Some(ls) = &self.state.mon().lock_surface {
            let surface = ls.wl_surface().clone();
            if let Some(kb) = self.state.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
            return;
        }

        if let Some(surface) = self.state.mon().exclusive_layer.clone() {
            if let Some(kb) = self.state.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
            return;
        }

        let target = id
            .and_then(|id| {
                self.state.mon_mut().tag_mut().promote(id);
                self.state.windows.get_mut(id)
            })
            .and_then(|we| {
                we.set_focused(true);
                we.window.toplevel().map(|tl| tl.wl_surface().clone())
            });

        if let Some(kb) = self.state.seat.get_keyboard() {
            kb.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
        self.state.ipc.mark_dirty();
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
    pub primary_selection_state: PrimarySelectionState,
    pub wlr_data_control_state: WlrDataControlState,
    pub ext_data_control_state: ExtDataControlState,
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub viewporter_state: ViewporterState,
    pub single_pixel_buffer_state: SinglePixelBufferState,
    pub idle_notifier_state: IdleNotifierState<Monotile>,
    pub idle_notifier_activity: bool,
    pub idle_inhibit_state: IdleInhibitManagerState,
    pub popups: PopupManager,
    pub seat: Seat<Monotile>,
    pub cursor_shape_state: CursorShapeManagerState,
    pub cursor: CursorManager,
    pub windows: Windows,
    pub monitors: Monitors,
    // TODO: active_monitor should be derived, not stored.
    // Every lookup (render, map, unmap, focus, layout) really needs
    // "monitor for this output/window/pointer location", not "active".
    // Remove this index when multi-monitor is implemented.
    pub active_monitor: usize,
    pub pending: Vec<Window>,
    pub locked: bool,
    pub session_lock_state: SessionLockManagerState,
    pub screencopy: ScreencopyState,
    pub ipc: IpcState,
}

impl State {
    pub fn new(
        dh: DisplayHandle,
        lh: LoopHandle<'static, Monotile>,
        signal: LoopSignal,
        config: Config,
    ) -> Self {
        let compositor_state = CompositorState::new::<Monotile>(&dh);
        let xdg_shell_state = XdgShellState::new::<Monotile>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Monotile>(&dh);
        let kde_decoration_state = KdeDecorationState::new::<Monotile>(&dh, KdeMode::Server);
        let layer_shell_state = WlrLayerShellState::new::<Monotile>(&dh);
        let session_lock_state = SessionLockManagerState::new::<Monotile, _>(&dh, |_| true);
        let viewporter_state = ViewporterState::new::<Monotile>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<Monotile>(&dh);
        let idle_notifier_state = IdleNotifierState::<Monotile>::new(&dh, lh);
        let idle_inhibit_state = IdleInhibitManagerState::new::<Monotile>(&dh);
        let shm_state = ShmState::new::<Monotile>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Monotile>(&dh);
        let data_device_state = DataDeviceState::new::<Monotile>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Monotile>(&dh);
        let wlr_data_control_state =
            WlrDataControlState::new::<Monotile, _>(&dh, Some(&primary_selection_state), |_| true);
        let ext_data_control_state =
            ExtDataControlState::new::<Monotile, _>(&dh, Some(&primary_selection_state), |_| true);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        let kb_conf = &config.seats["seat0"].keyboard;
        seat.add_keyboard(
            kb_conf.xkb_config(),
            kb_conf.repeat_delay,
            kb_conf.repeat_rate,
        )
        .unwrap();
        seat.add_pointer();
        info!(
            "keyboard: layout={} variant={}",
            kb_conf.layout, kb_conf.variant
        );

        let cursor_shape_state = CursorShapeManagerState::new::<Monotile>(&dh);
        let cursor = CursorManager::new(1.0);
        let screencopy = ScreencopyState::new(&dh);
        let ipc = IpcState::new(&dh);

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
            primary_selection_state,
            wlr_data_control_state,
            ext_data_control_state,
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            viewporter_state,
            single_pixel_buffer_state,
            idle_notifier_state,
            idle_notifier_activity: false,
            idle_inhibit_state,
            popups: PopupManager::default(),
            seat,
            cursor_shape_state,
            cursor,
            windows: Windows::default(),
            monitors: Monitors::default(),
            active_monitor: 0,
            pending: Vec::new(),
            locked: false,
            session_lock_state,
            screencopy,
            ipc,
        }
    }

    pub fn mon(&self) -> &Monitor {
        &self.monitors[self.active_monitor]
    }

    pub fn mon_mut(&mut self) -> &mut Monitor {
        &mut self.monitors[self.active_monitor]
    }

    pub fn add_monitor(&mut self, output: Output, settings: MonitorSettings) {
        let mut tags = Vec::new();
        tags.resize_with(settings.tags.len(), Tag::default);
        self.monitors.push(Monitor {
            output,
            settings,
            tags,
            active_tag: 0,
            prev_tag: 0,
            exclusive_layer: None,
            lock_surface: None,
        });
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
    ) -> (Option<(WlSurface, Point<f64, Logical>)>, Option<WindowId>) {
        if self.locked {
            let surface = self
                .mon()
                .lock_surface
                .as_ref()
                .map(|ls| (ls.wl_surface().clone(), pos));
            return (surface, None);
        }

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
        let hit = layer_hit(Layer::Overlay).or_else(|| layer_hit(Layer::Top));
        if let Some(hit) = hit {
            return (Some(hit), None);
        }

        // windows and popups
        for id in mon.tag().window_ids().rev() {
            let Some(we) = self.windows.get(id) else {
                continue;
            };
            let loc = we.surface_loc();
            let rel = pos - loc.to_f64();
            if let Some((s, point)) = we.window.surface_under(rel, WindowSurfaceType::ALL) {
                return (Some((s, (point + loc).to_f64())), Some(id));
            }
        }

        // bottom / background layers
        let hit = layer_hit(Layer::Bottom).or_else(|| layer_hit(Layer::Background));
        (hit, None)
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

    pub fn notify_activity(&mut self) {
        if !self.idle_notifier_activity {
            self.idle_notifier_activity = true;
            self.idle_notifier_state.notify_activity(&self.seat);
        }
    }

    pub fn flush_clients(&mut self) {
        self.idle_notifier_activity = false;
        self.screencopy.cleanup();
        self.ipc
            .flush(&self.monitors, &self.windows, self.active_monitor);
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
