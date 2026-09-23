use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    protocol::{wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_surface},
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
    ext_session_lock_v1::{self, ExtSessionLockV1},
};

use super::Fixture;

pub(super) struct LockClient {
    lock_manager: Option<ExtSessionLockManagerV1>,
    compositor: Option<wl_compositor::WlCompositor>,
    output: Option<wl_output::WlOutput>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_positions: Vec<(f64, f64)>,
    locked: bool,
    finished: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for LockClient {
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
                "ext_session_lock_manager_v1" => {
                    state.lock_manager = Some(registry.bind(name, version, qh, ()));
                }
                "wl_compositor" => state.compositor = Some(registry.bind(name, version, qh, ())),
                "wl_output" => state.output = Some(registry.bind(name, version, qh, ())),
                "wl_seat" => state.seat = Some(registry.bind(name, version, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for LockClient {
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

impl Dispatch<wl_surface::WlSurface, ()> for LockClient {
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

impl Dispatch<wl_output::WlOutput, ()> for LockClient {
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

impl Dispatch<wl_seat::WlSeat, ()> for LockClient {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
            && caps.contains(wl_seat::Capability::Pointer)
            && state.pointer.is_none()
        {
            state.pointer = Some(seat.get_pointer(qh, ()));
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for LockClient {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            }
            | wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => state.pointer_positions.push((surface_x, surface_y)),
            _ => {}
        }
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, ()> for LockClient {
    fn event(
        _: &mut Self,
        ls: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure { serial, .. } = event {
            ls.ack_configure(serial);
        }
    }
}

impl Dispatch<ExtSessionLockManagerV1, ()> for LockClient {
    fn event(
        _: &mut Self,
        _: &ExtSessionLockManagerV1,
        _: <ExtSessionLockManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtSessionLockV1, ()> for LockClient {
    fn event(
        state: &mut Self,
        _: &ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_session_lock_v1::Event::Locked => state.locked = true,
            ext_session_lock_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }
}

fn lock_roundtrip(
    f: &mut Fixture,
    conn: &Connection,
    client: &mut LockClient,
    queue: &mut wayland_client::EventQueue<LockClient>,
) {
    for _ in 0..10 {
        f.dispatch();
        f.mt.state.flush_clients();
        if let Some(guard) = conn.prepare_read() {
            guard.read().ok();
        }
        queue.dispatch_pending(client).unwrap();
        let _ = queue.flush();
    }
}

fn connect_lock_client(
    f: &mut Fixture,
) -> (
    Connection,
    wayland_client::EventQueue<LockClient>,
    LockClient,
) {
    let (server_socket, client_socket) = std::os::unix::net::UnixStream::pair().unwrap();
    f.mt.state.insert_client(server_socket);

    let backend = wayland_backend::client::Backend::connect(client_socket).unwrap();
    let conn = Connection::from_backend(backend);
    let mut queue = conn.new_event_queue();
    conn.display().get_registry(&queue.handle(), ());

    let mut client = LockClient {
        lock_manager: None,
        compositor: None,
        output: None,
        seat: None,
        pointer: None,
        pointer_positions: Vec::new(),
        locked: false,
        finished: false,
    };

    // initial roundtrip to bind globals
    lock_roundtrip(f, &conn, &mut client, &mut queue);
    (conn, queue, client)
}

fn request_lock(
    f: &mut Fixture,
    conn: &Connection,
    client: &mut LockClient,
    queue: &mut wayland_client::EventQueue<LockClient>,
) -> ExtSessionLockV1 {
    let mgr = client
        .lock_manager
        .as_ref()
        .expect("lock manager not bound");
    let lock = mgr.lock(&queue.handle(), ());
    let _ = queue.flush();
    lock_roundtrip(f, conn, client, queue);
    lock
}

fn create_lock_surface(
    f: &mut Fixture,
    conn: &Connection,
    client: &mut LockClient,
    queue: &mut wayland_client::EventQueue<LockClient>,
    lock: &ExtSessionLockV1,
) {
    let qh = queue.handle();
    let surface = client
        .compositor
        .as_ref()
        .expect("compositor not bound")
        .create_surface(&qh, ());
    let output = client.output.as_ref().expect("output not bound").clone();
    lock.get_lock_surface(&surface, &output, &qh, ());
    let _ = queue.flush();
    lock_roundtrip(f, conn, client, queue);
}

pub(super) fn lock_session(
    f: &mut Fixture,
) -> (
    Connection,
    wayland_client::EventQueue<LockClient>,
    LockClient,
) {
    let (conn, mut queue, mut client) = connect_lock_client(f);
    let _lock = request_lock(f, &conn, &mut client, &mut queue);
    let outputs: Vec<_> = (0..f.mt.state.monitors.len())
        .map(|i| f.mt.state.monitors[i].output.clone())
        .collect();
    for output in outputs {
        f.mt.state.confirm_lock(&output);
    }
    lock_roundtrip(f, &conn, &mut client, &mut queue);
    assert!(client.locked, "session lock should be confirmed");
    (conn, queue, client)
}

#[test]
fn lock_deferred_until_frame_presented() {
    let mut f = Fixture::new();
    let (conn, mut queue, mut client) = connect_lock_client(&mut f);

    let mgr = client
        .lock_manager
        .as_ref()
        .expect("lock manager not bound");
    let _lock = mgr.lock(&queue.handle(), ());
    let _ = queue.flush();

    // dispatch the lock request
    lock_roundtrip(&mut f, &conn, &mut client, &mut queue);

    assert!(f.mt.state.locked(), "state should be locked");
    assert!(f.mt.state.pending_lock.is_some(), "lock should be pending");
    assert!(
        !client.locked,
        "locked event should NOT be sent before frame is presented"
    );

    // simulate frame render path
    let output = f.mt.state.monitors[0].output.clone();
    f.mt.state.confirm_lock(&output);

    // now the locked event should arrive
    lock_roundtrip(&mut f, &conn, &mut client, &mut queue);

    assert!(
        client.locked,
        "locked event should be sent after confirm_lock"
    );
    assert!(
        f.mt.state.pending_lock.is_none(),
        "pending_lock should be consumed"
    );
}

#[test]
fn second_locker_refused_while_locked() {
    let mut f = Fixture::new();
    let output = f.mt.state.monitors[0].output.clone();

    let (conn_a, mut queue_a, mut a) = connect_lock_client(&mut f);
    let _lock_a = request_lock(&mut f, &conn_a, &mut a, &mut queue_a);
    f.mt.state.confirm_lock(&output);
    lock_roundtrip(&mut f, &conn_a, &mut a, &mut queue_a);
    assert!(a.locked, "first locker should hold the lock");

    let (conn_b, mut queue_b, mut b) = connect_lock_client(&mut f);
    let _lock_b = request_lock(&mut f, &conn_b, &mut b, &mut queue_b);

    assert!(b.finished, "second locker should be refused with finished");
    assert!(!b.locked, "second locker should not receive locked");
    assert!(f.mt.state.locked(), "session should remain locked");
}

#[test]
fn locker_takeover_after_disconnect() {
    let mut f = Fixture::new();
    let output = f.mt.state.monitors[0].output.clone();

    {
        let (conn_a, mut queue_a, mut a) = connect_lock_client(&mut f);
        let _lock_a = request_lock(&mut f, &conn_a, &mut a, &mut queue_a);
        f.mt.state.confirm_lock(&output);
        lock_roundtrip(&mut f, &conn_a, &mut a, &mut queue_a);
        assert!(a.locked, "first locker should hold the lock");
    }

    for _ in 0..10 {
        f.dispatch();
    }
    assert!(
        f.mt.state.locked(),
        "session must stay locked after the locker died"
    );

    let (conn_b, mut queue_b, mut b) = connect_lock_client(&mut f);
    let _lock_b = request_lock(&mut f, &conn_b, &mut b, &mut queue_b);
    f.mt.state.confirm_lock(&output);
    lock_roundtrip(&mut f, &conn_b, &mut b, &mut queue_b);

    assert!(
        b.locked,
        "new locker should take over after the locker died"
    );
    assert!(!b.finished, "takeover should not be refused");
}

#[test]
fn lock_surface_receives_the_pointer_position() {
    let mut f = Fixture::new();
    let output = f.mt.state.monitors[0].output.clone();

    let (conn, mut queue, mut client) = connect_lock_client(&mut f);
    let lock = request_lock(&mut f, &conn, &mut client, &mut queue);
    create_lock_surface(&mut f, &conn, &mut client, &mut queue, &lock);
    f.mt.state.confirm_lock(&output);
    lock_roundtrip(&mut f, &conn, &mut client, &mut queue);

    for pos in [(100.0, 50.0), (300.0, 200.0)] {
        f.pointer_motion(pos.into());
        lock_roundtrip(&mut f, &conn, &mut client, &mut queue);
        assert_eq!(
            client.pointer_positions.last().copied(),
            Some(pos),
            "the locker must see the pointer where it actually is",
        );
    }
}

#[test]
fn locked_session_does_not_focus_a_window() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_keyboard();
    f.roundtrip(c);

    let (conn, mut queue, mut client) = connect_lock_client(&mut f);
    let _lock = request_lock(&mut f, &conn, &mut client, &mut queue);
    assert!(f.mt.state.locked(), "session should be locked");
    assert!(
        f.mt.state.seat_mon().lock_surface.is_none(),
        "the locker has not created its surface yet",
    );

    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);

    assert!(
        f.client(c).keyboard_focus().is_none(),
        "a window must not take keyboard focus while the session is locked",
    );
}
