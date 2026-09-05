use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_registry};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_v1::{self, ExtSessionLockV1},
};

use super::Fixture;

pub(super) struct LockClient {
    lock_manager: Option<ExtSessionLockManagerV1>,
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
            if interface == "ext_session_lock_manager_v1" {
                state.lock_manager = Some(registry.bind(name, version, qh, ()));
            }
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
