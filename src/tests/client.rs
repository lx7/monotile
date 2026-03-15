use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

use super::ipc_client_protocol::dwl::{
    zdwl_ipc_manager_v2::ZdwlIpcManagerV2, zdwl_ipc_output_v2::ZdwlIpcOutputV2,
};
use super::ipc_client_protocol::monotile::{
    zmonotile_control_v1::ZmonotileControlV1, zmonotile_output_status_v1::ZmonotileOutputStatusV1,
    zmonotile_seat_control_v1::ZmonotileSeatControlV1,
    zmonotile_seat_status_v1::ZmonotileSeatStatusV1,
    zmonotile_status_manager_v1::ZmonotileStatusManagerV1,
};

// ── Client state ────────────────────────────────────

pub struct Client {
    conn: Connection,
    queue: EventQueue<ClientData>,
    data: ClientData,
}

struct ClientData {
    compositor: Option<wl_compositor::WlCompositor>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    shm: Option<wl_shm::WlShm>,
    buffer: Option<wl_buffer::WlBuffer>,
    layer_shell: Option<ZwlrLayerShellV1>,
    layers: Vec<LayerState>,
    windows: Vec<WindowState>,

    ipc_output: Option<wl_output::WlOutput>,
    ipc_seat: Option<wl_seat::WlSeat>,
    ipc_status_manager: Option<ZmonotileStatusManagerV1>,
    ipc_control: Option<ZmonotileControlV1>,
    ipc_output_status: Option<ZmonotileOutputStatusV1>,
    ipc_seat_status: Option<ZmonotileSeatStatusV1>,
    ipc_seat_control: Option<ZmonotileSeatControlV1>,
    pub ipc_events: Vec<IpcEvent>,

    ipc_dwl_manager: Option<ZdwlIpcManagerV2>,
    ipc_dwl_output: Option<ZdwlIpcOutputV2>,
    pub ipc_dwl_events: Vec<DwlEvent>,
}

pub struct LayerState {
    pub surface: wl_surface::WlSurface,
    pub layer_surface: ZwlrLayerSurfaceV1,
    pub last_serial: u32,
}

pub struct WindowState {
    pub surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub toplevel: xdg_toplevel::XdgToplevel,
    pub configures: Vec<Configure>,
    pub closed: bool,
    last_serial: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DwlEvent {
    Tags(u32),
    ManagerLayout(String),
    ToggleVisibility,
    Active(u32),
    Tag {
        tag: u32,
        state: u32,
        clients: u32,
        focused: u32,
    },
    Layout(u32),
    Title(String),
    AppId(String),
    LayoutSymbol(String),
    Frame,
    Fullscreen(u32),
    Floating(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IpcEvent {
    TagCount(u32),
    TagName {
        index: u32,
        name: String,
    },
    FocusedTags(u32),
    OccupiedTags(u32),
    UrgentTags(u32),
    Layout {
        name: String,
        symbol: String,
    },
    FocusedOutput,
    FocusedToplevel {
        title: Option<String>,
        app_id: String,
        fullscreen: bool,
        floating: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Configure {
    pub width: i32,
    pub height: i32,
    pub states: Vec<xdg_toplevel::State>,
}

impl Client {
    pub fn new(stream: UnixStream) -> Self {
        let backend = wayland_backend::client::Backend::connect(stream).unwrap();
        let conn = Connection::from_backend(backend);
        let queue = conn.new_event_queue();
        let qh = queue.handle();

        conn.display().get_registry(&qh, ());

        let data = ClientData {
            compositor: None,
            wm_base: None,
            shm: None,
            buffer: None,
            layer_shell: None,
            layers: Vec::new(),
            windows: Vec::new(),
            ipc_output: None,
            ipc_seat: None,
            ipc_status_manager: None,
            ipc_control: None,
            ipc_output_status: None,
            ipc_seat_status: None,
            ipc_seat_control: None,
            ipc_events: Vec::new(),

            ipc_dwl_manager: None,
            ipc_dwl_output: None,
            ipc_dwl_events: Vec::new(),
        };

        let mut client = Client { conn, queue, data };
        client.dispatch();
        client
    }

    pub fn dispatch(&mut self) {
        if let Some(guard) = self.conn.prepare_read() {
            let _ = guard.read();
        }
        self.queue.dispatch_pending(&mut self.data).unwrap();
        let _ = self.queue.flush();
    }

    pub fn start_sync(&self) -> Arc<AtomicBool> {
        let done = Arc::new(AtomicBool::new(false));
        let qh = self.queue.handle();
        self.conn.display().sync(&qh, done.clone());
        let _ = self.queue.flush();
        done
    }

    pub fn create_window(&mut self) -> usize {
        let qh = self.queue.handle();
        let comp = self.data.compositor.as_ref().expect("compositor not bound");
        let wm = self.data.wm_base.as_ref().expect("xdg_wm_base not bound");

        let surface = comp.create_surface(&qh, ());
        let xdg = wm.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());

        let idx = self.data.windows.len();
        self.data.windows.push(WindowState {
            surface,
            xdg_surface: xdg,
            toplevel,
            configures: Vec::new(),
            closed: false,
            last_serial: 0,
        });
        let _ = self.queue.flush();
        idx
    }

    pub fn commit(&self, win: usize) {
        self.data.windows[win].surface.commit();
        let _ = self.queue.flush();
    }

    pub fn ack_and_commit(&mut self, win: usize) {
        let qh = self.queue.handle();
        let ws = &self.data.windows[win];
        if ws.last_serial != 0 {
            ws.xdg_surface.ack_configure(ws.last_serial);
        }
        // create a shared 1x1 shm buffer on first use
        if self.data.buffer.is_none() {
            let shm = self.data.shm.as_ref().expect("wl_shm not bound");
            let mut tmp = tempfile::tempfile().unwrap();
            tmp.write_all(&[0u8; 4]).unwrap();
            let pool = shm.create_pool(tmp.as_fd(), 4, &qh, ());
            self.data.buffer =
                Some(pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ()));
        }
        ws.surface.attach(self.data.buffer.as_ref(), 0, 0);
        ws.surface.commit();
        let _ = self.queue.flush();
    }

    pub fn window(&self, win: usize) -> &WindowState {
        &self.data.windows[win]
    }

    pub fn take_configures(&mut self, win: usize) -> Vec<Configure> {
        self.data.windows[win].configures.drain(..).collect()
    }

    // layer shell

    pub fn create_layer_surface(&mut self) -> usize {
        let qh = self.queue.handle();
        let comp = self.data.compositor.as_ref().unwrap();
        let shell = self.data.layer_shell.as_ref().unwrap();
        let output = self.data.ipc_output.as_ref().unwrap();

        let surface = comp.create_surface(&qh, ());
        let ls = shell.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Top,
            "test".to_string(),
            &qh,
            (),
        );
        ls.set_size(0, 30);
        ls.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );

        let idx = self.data.layers.len();
        self.data.layers.push(LayerState {
            surface,
            layer_surface: ls,
            last_serial: 0,
        });
        let _ = self.queue.flush();
        idx
    }

    pub fn layer_commit(&self, ls: usize) {
        self.data.layers[ls].surface.commit();
        let _ = self.queue.flush();
    }

    pub fn layer_attach_and_commit(&mut self, ls: usize) {
        let qh = self.queue.handle();
        if self.data.buffer.is_none() {
            let shm = self.data.shm.as_ref().unwrap();
            let mut tmp = tempfile::tempfile().unwrap();
            tmp.write_all(&[0u8; 4]).unwrap();
            let pool = shm.create_pool(tmp.as_fd(), 4, &qh, ());
            self.data.buffer =
                Some(pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ()));
        }
        let layer = &self.data.layers[ls];
        layer.surface.attach(self.data.buffer.as_ref(), 0, 0);
        layer.surface.commit();
        let _ = self.queue.flush();
    }

    // monotile-ipc

    pub fn bind_output_status(&mut self) {
        let qh = self.queue.handle();
        let mgr = self
            .data
            .ipc_status_manager
            .as_ref()
            .expect("ipc_status_manager");
        let output = self.data.ipc_output.as_ref().expect("ipc_output");
        self.data.ipc_output_status = Some(mgr.get_output_status(output, &qh, ()));
        let _ = self.queue.flush();
    }

    pub fn bind_seat_status(&mut self) {
        let qh = self.queue.handle();
        let mgr = self
            .data
            .ipc_status_manager
            .as_ref()
            .expect("ipc_status_manager");
        let seat = self.data.ipc_seat.as_ref().expect("ipc_seat");
        self.data.ipc_seat_status = Some(mgr.get_seat_status(seat, &qh, ()));
        let _ = self.queue.flush();
    }

    pub fn bind_seat_control(&mut self) {
        let qh = self.queue.handle();
        let ctl = self.data.ipc_control.as_ref().expect("ipc_control");
        let seat = self.data.ipc_seat.as_ref().expect("ipc_seat");
        self.data.ipc_seat_control = Some(ctl.get_seat_control(seat, &qh, ()));
        let _ = self.queue.flush();
    }

    pub fn seat_control(&self) -> &ZmonotileSeatControlV1 {
        self.data
            .ipc_seat_control
            .as_ref()
            .expect("ipc_seat_control")
    }

    pub fn control(&self) -> &ZmonotileControlV1 {
        self.data.ipc_control.as_ref().expect("ipc_control")
    }

    pub fn destroy_output_status(&mut self) {
        if let Some(os) = self.data.ipc_output_status.take() {
            os.destroy();
            let _ = self.queue.flush();
        }
    }

    pub fn destroy_seat_status(&mut self) {
        if let Some(ss) = self.data.ipc_seat_status.take() {
            ss.destroy();
            let _ = self.queue.flush();
        }
    }

    pub fn take_ipc_events(&mut self) -> Vec<IpcEvent> {
        self.data.ipc_events.drain(..).collect()
    }

    pub fn flush(&self) {
        let _ = self.queue.flush();
    }

    // dwl-ipc

    pub fn bind_dwl_output(&mut self) {
        let qh = self.queue.handle();
        let mgr = self.data.ipc_dwl_manager.as_ref().expect("dwl_manager");
        let output = self.data.ipc_output.as_ref().expect("ipc_output");
        self.data.ipc_dwl_output = Some(mgr.get_output(output, &qh, ()));
        let _ = self.queue.flush();
    }

    pub fn dwl_output(&self) -> &ZdwlIpcOutputV2 {
        self.data.ipc_dwl_output.as_ref().expect("dwl_output")
    }

    pub fn destroy_dwl_output(&mut self) {
        if let Some(o) = self.data.ipc_dwl_output.take() {
            o.release();
            let _ = self.queue.flush();
        }
    }

    pub fn take_dwl_events(&mut self) -> Vec<DwlEvent> {
        self.data.ipc_dwl_events.drain(..).collect()
    }
}

// ── Dispatch impls ──────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, ()> for ClientData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version, qh, ()));
                }
                "xdg_wm_base" => {
                    state.wm_base = Some(registry.bind(name, version, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version, qh, ()));
                }
                "wl_output" => {
                    state.ipc_output = Some(registry.bind(name, version, qh, ()));
                }
                "wl_seat" => {
                    state.ipc_seat = Some(registry.bind(name, version, qh, ()));
                }
                "zmonotile_status_manager_v1" => {
                    state.ipc_status_manager = Some(registry.bind(name, version, qh, ()));
                }
                "zmonotile_control_v1" => {
                    state.ipc_control = Some(registry.bind(name, version, qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version, qh, ()));
                }
                "zdwl_ipc_manager_v2" => {
                    state.ipc_dwl_manager = Some(registry.bind(name, version, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ClientData {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientData {
    fn event(
        state: &mut Self,
        xdg: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            for ws in &mut state.windows {
                if ws.xdg_surface == *xdg {
                    ws.last_serial = serial;
                    break;
                }
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientData {
    fn event(
        state: &mut Self,
        tl: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let parsed: Vec<xdg_toplevel::State> = states
                    .chunks(4)
                    .filter_map(|c| {
                        let v = u32::from_ne_bytes(c.try_into().ok()?);
                        xdg_toplevel::State::try_from(v).ok()
                    })
                    .collect();

                for ws in &mut state.windows {
                    if ws.toplevel == *tl {
                        ws.configures.push(Configure {
                            width,
                            height,
                            states: parsed,
                        });
                        break;
                    }
                }
            }
            xdg_toplevel::Event::Close => {
                for ws in &mut state.windows {
                    if ws.toplevel == *tl {
                        ws.closed = true;
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, Arc<AtomicBool>> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        flag: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ── Layer shell Dispatch impls ──────────────────────

impl Dispatch<ZwlrLayerShellV1, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for ClientData {
    fn event(
        state: &mut Self,
        ls: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            for l in &mut state.layers {
                if l.layer_surface == *ls {
                    l.last_serial = serial;
                    break;
                }
            }
        }
    }
}

// ── IPC Dispatch impls ──────────────────────────────

impl Dispatch<ZmonotileStatusManagerV1, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &ZmonotileStatusManagerV1,
        _: <ZmonotileStatusManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZmonotileOutputStatusV1, ()> for ClientData {
    fn event(
        state: &mut Self,
        _: &ZmonotileOutputStatusV1,
        event: <ZmonotileOutputStatusV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use super::ipc_client_protocol::monotile::zmonotile_output_status_v1::Event;
        match event {
            Event::TagCount { count } => state.ipc_events.push(IpcEvent::TagCount(count)),
            Event::TagName { index, name } => {
                state.ipc_events.push(IpcEvent::TagName { index, name });
            }
            Event::FocusedTags { tags } => state.ipc_events.push(IpcEvent::FocusedTags(tags)),
            Event::OccupiedTags { tags } => state.ipc_events.push(IpcEvent::OccupiedTags(tags)),
            Event::UrgentTags { tags } => state.ipc_events.push(IpcEvent::UrgentTags(tags)),
            Event::Layout { name, symbol } => {
                state.ipc_events.push(IpcEvent::Layout { name, symbol });
            }
        }
    }
}

impl Dispatch<ZmonotileSeatStatusV1, ()> for ClientData {
    fn event(
        state: &mut Self,
        _: &ZmonotileSeatStatusV1,
        event: <ZmonotileSeatStatusV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use super::ipc_client_protocol::monotile::zmonotile_seat_status_v1::Event;
        match event {
            Event::FocusedOutput { .. } => {
                state.ipc_events.push(IpcEvent::FocusedOutput);
            }
            Event::FocusedToplevel {
                title,
                app_id,
                fullscreen,
                floating,
            } => {
                state.ipc_events.push(IpcEvent::FocusedToplevel {
                    title,
                    app_id,
                    fullscreen: fullscreen != 0,
                    floating: floating != 0,
                });
            }
        }
    }
}

impl Dispatch<ZmonotileControlV1, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &ZmonotileControlV1,
        _: <ZmonotileControlV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZmonotileSeatControlV1, ()> for ClientData {
    fn event(
        _: &mut Self,
        _: &ZmonotileSeatControlV1,
        _: <ZmonotileSeatControlV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ── DWL IPC Dispatch impls ─────────────────────────

impl Dispatch<ZdwlIpcManagerV2, ()> for ClientData {
    fn event(
        state: &mut Self,
        _: &ZdwlIpcManagerV2,
        event: <ZdwlIpcManagerV2 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use super::ipc_client_protocol::dwl::zdwl_ipc_manager_v2::Event;
        match event {
            Event::Tags { amount } => state.ipc_dwl_events.push(DwlEvent::Tags(amount)),
            Event::Layout { name } => state.ipc_dwl_events.push(DwlEvent::ManagerLayout(name)),
        }
    }
}

impl Dispatch<ZdwlIpcOutputV2, ()> for ClientData {
    fn event(
        state: &mut Self,
        _: &ZdwlIpcOutputV2,
        event: <ZdwlIpcOutputV2 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use super::ipc_client_protocol::dwl::zdwl_ipc_output_v2::Event;
        match event {
            Event::ToggleVisibility => state.ipc_dwl_events.push(DwlEvent::ToggleVisibility),
            Event::Active { active } => state.ipc_dwl_events.push(DwlEvent::Active(active)),
            Event::Tag {
                tag,
                state: tag_state,
                clients,
                focused,
            } => state.ipc_dwl_events.push(DwlEvent::Tag {
                tag,
                state: tag_state.into(),
                clients,
                focused,
            }),
            Event::Layout { layout } => state.ipc_dwl_events.push(DwlEvent::Layout(layout)),
            Event::Title { title } => state.ipc_dwl_events.push(DwlEvent::Title(title)),
            Event::Appid { appid } => state.ipc_dwl_events.push(DwlEvent::AppId(appid)),
            Event::LayoutSymbol { layout } => {
                state.ipc_dwl_events.push(DwlEvent::LayoutSymbol(layout))
            }
            Event::Frame => state.ipc_dwl_events.push(DwlEvent::Frame),
            Event::Fullscreen { is_fullscreen } => state
                .ipc_dwl_events
                .push(DwlEvent::Fullscreen(is_fullscreen)),
            Event::Floating { is_floating } => {
                state.ipc_dwl_events.push(DwlEvent::Floating(is_floating))
            }
        }
    }
}
