use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_registry};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_v1::{self, ExtSessionLockV1},
};

use super::Fixture;

struct LockClient {
    lock_manager: Option<ExtSessionLockManagerV1>,
    locked: bool,
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
        if let ext_session_lock_v1::Event::Locked = event {
            state.locked = true;
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

#[test]
fn lock_deferred_until_frame_presented() {
    let mut f = Fixture::new();

    // set up a raw session lock client
    let (server_socket, client_socket) = std::os::unix::net::UnixStream::pair().unwrap();
    f.mt.state.insert_client(server_socket);

    let backend = wayland_backend::client::Backend::connect(client_socket).unwrap();
    let conn = Connection::from_backend(backend);
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut client = LockClient {
        lock_manager: None,
        locked: false,
    };

    // initial roundtrip to bind globals
    lock_roundtrip(&mut f, &conn, &mut client, &mut queue);

    let mgr = client
        .lock_manager
        .as_ref()
        .expect("lock manager not bound");
    let _lock = mgr.lock(&qh, ());
    let _ = queue.flush();

    // dispatch the lock request
    lock_roundtrip(&mut f, &conn, &mut client, &mut queue);

    assert!(f.mt.state.locked, "state.locked should be true");
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
