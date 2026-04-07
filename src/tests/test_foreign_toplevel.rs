use super::Fixture;
use super::client::ForeignToplevelEvent;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn open_window_with(f: &mut Fixture, c: usize, title: &str, app_id: &str) -> usize {
    let w = f.client_mut(c).create_window();
    f.client(c).window(w).toplevel.set_title(title.into());
    f.client(c).window(w).toplevel.set_app_id(app_id.into());
    f.client(c).flush();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

#[test]
fn late_bind_receives_existing_windows() {
    let mut f = Fixture::new();
    let c1 = f.add_client();
    open_window_with(&mut f, c1, "first", "app.first");
    open_window_with(&mut f, c1, "second", "app.second");

    // second client connects after windows exist
    let c2 = f.add_client();
    f.roundtrip(c2);

    let events = f.client_mut(c2).take_foreign_toplevel_events();
    let titles: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ForeignToplevelEvent::Title { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        titles.contains(&"first"),
        "should see first window, got {events:?}"
    );
    assert!(
        titles.contains(&"second"),
        "should see second window, got {events:?}"
    );
}

#[test]
fn window_open_sends_handle() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window_with(&mut f, c, "hello", "test.app");

    let events = f.client_mut(c).take_foreign_toplevel_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ForeignToplevelEvent::New { .. })),
        "should receive identifier, got {events:?}",
    );
    assert!(
        events.iter().any(|e| matches!(
            e, ForeignToplevelEvent::Title { title, .. } if title == "hello"
        )),
        "should receive title, got {events:?}",
    );
    assert!(
        events.iter().any(|e| matches!(
            e, ForeignToplevelEvent::AppId { app_id, .. } if app_id == "test.app"
        )),
        "should receive app_id, got {events:?}",
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ForeignToplevelEvent::Done { .. })),
        "should receive done, got {events:?}",
    );
}

#[test]
fn title_change() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).take_foreign_toplevel_events();

    f.client(c).window(w).toplevel.set_title("new title".into());
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_foreign_toplevel_events();
    assert!(
        events.iter().any(|e| matches!(
            e, ForeignToplevelEvent::Title { title, .. } if title == "new title"
        )),
        "title should update via flush, got {events:?}",
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ForeignToplevelEvent::Done { .. })),
        "done should follow title change, got {events:?}",
    );
}

#[test]
fn app_id_change() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).take_foreign_toplevel_events();

    f.client(c).window(w).toplevel.set_app_id("new.app".into());
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_foreign_toplevel_events();
    assert!(
        events.iter().any(|e| matches!(
            e, ForeignToplevelEvent::AppId { app_id, .. } if app_id == "new.app"
        )),
        "app_id should update via flush, got {events:?}",
    );
}

#[test]
fn window_close() {
    let mut f = Fixture::new();
    let c1 = f.add_client();
    let c2 = f.add_client();

    open_window(&mut f, c1);
    f.roundtrip(c2);
    f.client_mut(c2).take_foreign_toplevel_events();

    f.client(c1).window(0).toplevel.destroy();
    f.client(c1).window(0).xdg_surface.destroy();
    f.client(c1).window(0).surface.destroy();
    f.client(c1).flush();
    f.dispatch();
    f.mt.state.flush_clients();
    f.roundtrip(c2);

    let events = f.client_mut(c2).take_foreign_toplevel_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ForeignToplevelEvent::Closed { .. })),
        "should receive closed event, got {events:?}",
    );
}

#[test]
fn client_disconnect() {
    let mut f = Fixture::new();
    let c = f.add_client();
    open_window(&mut f, c);
    f.drop_client(c);
    f.dispatch();
    f.mt.state.flush_clients();
    // no panic = pass
}

// ── Toplevel image capture source tests ─────────────

#[test]
fn capture_source_manager_advertised() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);
    assert!(
        f.client(c).has_toplevel_capture_manager(),
        "ext_foreign_toplevel_image_capture_source_manager_v1 should be advertised",
    );
}

#[test]
fn create_capture_source_from_toplevel() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window_with(&mut f, c, "test", "test.app");

    let handles = f.client_mut(c).take_foreign_toplevel_handles();
    assert!(!handles.is_empty(), "should have toplevel handles");

    let source = f.client(c).create_toplevel_capture_source(&handles[0]);
    assert!(source.is_some(), "should create capture source");

    f.roundtrip(c);
}
