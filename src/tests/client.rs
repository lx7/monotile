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
        wl_buffer, wl_callback, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

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
    windows: Vec<WindowState>,
}

pub struct WindowState {
    pub surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub toplevel: xdg_toplevel::XdgToplevel,
    pub configures: Vec<Configure>,
    pub closed: bool,
    last_serial: u32,
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
            windows: Vec::new(),
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
