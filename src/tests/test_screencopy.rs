use wayland_client::protocol::wl_shm;

use super::Fixture;
use super::client::{
    CaptureFrameEvent, CaptureSessionEvent, CursorSessionEvent, ForeignToplevelEvent,
};

// ── Helpers ────────────────────────────────────────

fn assert_valid_constraint_batch(events: &[CaptureSessionEvent]) -> (u32, u32) {
    assert!(!events.is_empty(), "constraint batch must not be empty");
    assert_eq!(
        events.last(),
        Some(&CaptureSessionEvent::Done),
        "constraint batch must end with done, got {events:?}",
    );

    let done_count = events
        .iter()
        .filter(|e| matches!(e, CaptureSessionEvent::Done))
        .count();
    assert_eq!(done_count, 1, "exactly one done per batch, got {events:?}");

    let sizes: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CaptureSessionEvent::BufferSize { width, height } => Some((*width, *height)),
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes.len(),
        1,
        "exactly one buffer_size per batch, got {events:?}",
    );
    let (width, height) = sizes[0];
    assert!(width > 0 && height > 0, "buffer_size must be non-zero");

    let has_format = events.iter().any(|e| {
        matches!(
            e,
            CaptureSessionEvent::ShmFormat(_) | CaptureSessionEvent::DmabufFormat { .. }
        )
    });
    assert!(has_format, "must have at least one format, got {events:?}");

    let device_count = events
        .iter()
        .filter(|e| matches!(e, CaptureSessionEvent::DmabufDevice))
        .count();
    assert!(
        device_count <= 1,
        "at most one dmabuf_device, got {events:?}",
    );

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, CaptureSessionEvent::Stopped)),
        "stopped must not appear in constraint batch, got {events:?}",
    );

    (width, height)
}

fn roundtrip_and_capture(f: &mut Fixture, c: usize) {
    f.client(c).flush();
    f.roundtrip(c);
    f.fail_pending_captures();
    f.mt.state.flush_clients();
    f.roundtrip(c);
}

// ── Session lock ───────────────────────────────────

#[test]
fn capture_blocked_when_locked() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);

    // Lock the session
    f.mt.state.locked = true;

    // Try to capture - should fail
    let buffer = f.client(c).create_shm_buffer(width as i32, height as i32);
    let frame = f.client(c).create_capture_frame(&session);
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();
    f.client(c).flush();
    f.roundtrip(c);

    let frame_events = f.client_mut(c).take_capture_frame_events();
    assert_eq!(frame_events.len(), 1);
    assert!(
        matches!(frame_events[0], CaptureFrameEvent::Failed(_)),
        "capture must fail when locked, got {frame_events:?}",
    );

    frame.destroy();
    f.client(c).flush();
}

fn open_window_and_get_handle(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client(c)
        .window(w)
        .toplevel
        .set_title("capture-test".into());
    f.client(c)
        .window(w)
        .toplevel
        .set_app_id("test.capture".into());
    f.client(c).flush();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

// ── Fullscreen capture ─────────────────────────────

#[test]
fn fullscreen_globals_advertised() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    assert!(
        f.client(c).has_output_capture_source_manager(),
        "ext_output_image_capture_source_manager_v1 must be advertised",
    );
    assert!(
        f.client(c).has_capture_manager(),
        "ext_image_copy_capture_manager_v1 must be advertised",
    );
}

#[test]
fn fullscreen_session_receives_constraints() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let _session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);

    assert_eq!((width, height), (1000, 800), "must match output mode");

    assert!(
        events
            .iter()
            .any(|e| matches!(e, CaptureSessionEvent::ShmFormat(wl_shm::Format::Argb8888))),
        "must support Argb8888, got {events:?}",
    );
}

#[test]
fn fullscreen_capture_frame_lifecycle() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);

    let buffer = f.client(c).create_shm_buffer(width as i32, height as i32);
    let frame = f.client(c).create_capture_frame(&session);
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();
    roundtrip_and_capture(&mut f, c);

    let frame_events = f.client_mut(c).take_capture_frame_events();
    assert_eq!(frame_events.len(), 1);
    assert!(matches!(frame_events[0], CaptureFrameEvent::Failed(_)));

    frame.destroy();
    f.client(c).flush();
}

#[test]
fn fullscreen_capture_continuous() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);

    let buffer = f.client(c).create_shm_buffer(width as i32, height as i32);

    for _ in 0..3 {
        let frame = f.client(c).create_capture_frame(&session);
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, width as i32, height as i32);
        frame.capture();
        roundtrip_and_capture(&mut f, c);

        let ev = f.client_mut(c).take_capture_frame_events();
        assert!(matches!(ev.last(), Some(CaptureFrameEvent::Failed(_))));

        frame.destroy();
        f.client(c).flush();
    }
}

#[test]
fn fullscreen_capture_with_cursors() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let _session = f
        .client(c)
        .create_capture_session(&source, true)
        .expect("create_session with paint_cursors");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    assert_valid_constraint_batch(&events);
}

// ── Toplevel capture ───────────────────────────────

#[test]
fn toplevel_handle_event_ordering() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    open_window_and_get_handle(&mut f, c);

    let events = f.client_mut(c).take_foreign_toplevel_events();

    let new_pos = events
        .iter()
        .position(|e| matches!(e, ForeignToplevelEvent::New { .. }))
        .expect("must receive identifier");
    let title_pos = events
        .iter()
        .position(|e| matches!(e, ForeignToplevelEvent::Title { .. }))
        .expect("must receive title");
    let app_id_pos = events
        .iter()
        .position(|e| matches!(e, ForeignToplevelEvent::AppId { .. }))
        .expect("must receive app_id");
    let done_pos = events
        .iter()
        .position(|e| matches!(e, ForeignToplevelEvent::Done { .. }))
        .expect("must receive done");

    assert!(
        new_pos < title_pos && new_pos < app_id_pos,
        "identifier must precede title and app_id, got {events:?}",
    );
    assert!(
        title_pos < done_pos && app_id_pos < done_pos,
        "title and app_id must precede done, got {events:?}",
    );
}

#[test]
fn toplevel_session_receives_constraints() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    open_window_and_get_handle(&mut f, c);
    f.client_mut(c).take_foreign_toplevel_events();
    let handles = f.client_mut(c).take_foreign_toplevel_handles();
    assert!(!handles.is_empty(), "must have toplevel handles");

    let source = f
        .client(c)
        .create_toplevel_capture_source(&handles[0])
        .expect("create toplevel capture source");
    let _session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    assert_valid_constraint_batch(&events);
}

#[test]
fn toplevel_capture_continuous() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    open_window_and_get_handle(&mut f, c);
    f.client_mut(c).take_foreign_toplevel_events();
    let handles = f.client_mut(c).take_foreign_toplevel_handles();

    let source = f
        .client(c)
        .create_toplevel_capture_source(&handles[0])
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);

    let buffer = f.client(c).create_shm_buffer(width as i32, height as i32);

    for _ in 0..3 {
        let frame = f.client(c).create_capture_frame(&session);
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, width as i32, height as i32);
        frame.capture();
        roundtrip_and_capture(&mut f, c);

        let ev = f.client_mut(c).take_capture_frame_events();
        assert!(matches!(ev.last(), Some(CaptureFrameEvent::Failed(_))));

        frame.destroy();
        f.client(c).flush();
    }
}

#[test]
fn toplevel_capture_cross_client() {
    let mut f = Fixture::new();
    let c1 = f.add_client();
    let c2 = f.add_client();

    open_window_and_get_handle(&mut f, c1);

    f.roundtrip(c2);
    let events = f.client_mut(c2).take_foreign_toplevel_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ForeignToplevelEvent::New { .. })),
        "client 2 must see client 1's toplevel, got {events:?}",
    );

    let handles = f.client_mut(c2).take_foreign_toplevel_handles();
    assert!(!handles.is_empty(), "client 2 must have handles");

    let source = f
        .client(c2)
        .create_toplevel_capture_source(&handles[0])
        .expect("create_source");
    let _session = f
        .client(c2)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c2);

    let events = f.client_mut(c2).take_capture_session_events();
    assert_valid_constraint_batch(&events);
}

#[test]
fn toplevel_capture_stopped_on_close() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let w = open_window_and_get_handle(&mut f, c);
    f.client_mut(c).take_foreign_toplevel_events();
    let handles = f.client_mut(c).take_foreign_toplevel_handles();

    let source = f
        .client(c)
        .create_toplevel_capture_source(&handles[0])
        .expect("create_source");
    let _session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    assert_valid_constraint_batch(&events);

    f.client(c).window(w).toplevel.destroy();
    f.client(c).window(w).xdg_surface.destroy();
    f.client(c).window(w).surface.destroy();
    f.client(c).flush();
    f.dispatch();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CaptureSessionEvent::Stopped)),
        "session must receive stopped when toplevel is closed, got {events:?}",
    );
}

#[test]
fn toplevel_capture_sets_screencast_flag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    open_window_and_get_handle(&mut f, c);
    let id = f.mt.state.mon().tag().focused_id().expect("focused window");
    assert!(
        !(f.mt.state.windows[id].screencasts > 0),
        "screencast off initially"
    );

    f.client_mut(c).take_foreign_toplevel_events();
    let handles = f.client_mut(c).take_foreign_toplevel_handles();

    let source = f
        .client(c)
        .create_toplevel_capture_source(&handles[0])
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    assert!(
        (f.mt.state.windows[id].screencasts > 0),
        "screencast must be set after session starts",
    );

    session.destroy();
    f.client(c).flush();
    f.roundtrip(c);

    assert!(
        !(f.mt.state.windows[id].screencasts > 0),
        "screencast must be cleared after session destroyed",
    );
}

#[test]
fn output_capture_does_not_set_screencast_flag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    open_window_and_get_handle(&mut f, c);
    let id = f.mt.state.mon().tag().focused_id().expect("focused window");

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let _session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.roundtrip(c);

    assert!(
        !(f.mt.state.windows[id].screencasts > 0),
        "output capture must not set screencast on windows",
    );
}

// ── Cursor session ─────────────────────────────────

#[test]
fn cursor_session_receives_constraints() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f.client(c).create_output_capture_source().expect("source");
    let cursor_session = f
        .client(c)
        .create_cursor_session(&source)
        .expect("cursor session");
    let _capture_session = f
        .client(c)
        .cursor_session_get_capture_session(&cursor_session);
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    let (width, height) = assert_valid_constraint_batch(&events);
    assert_eq!(width, height, "cursor buffer should be square");
}

#[test]
fn cursor_session_enter_and_position() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f.client(c).create_output_capture_source().expect("source");
    let cursor_session = f
        .client(c)
        .create_cursor_session(&source)
        .expect("cursor session");
    let _capture_session = f
        .client(c)
        .cursor_session_get_capture_session(&cursor_session);
    f.roundtrip(c);
    f.client_mut(c).take_capture_session_events();

    // Simulate pointer motion to trigger cursor enter + position
    let ptr = f.mt.state.seat.get_pointer().unwrap();
    let pos = ptr.current_location();
    let output = &f.mt.state.mon().output;
    let hotspot = f.mt.state.cursor.hotspot;
    f.mt.state
        .screencopy
        .update_cursor(Some(pos), hotspot, output);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_cursor_session_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CursorSessionEvent::Enter)),
        "should receive enter event, got {events:?}",
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CursorSessionEvent::Position { .. })),
        "should receive position event, got {events:?}",
    );
}

#[test]
fn cursor_session_hotspot() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f.client(c).create_output_capture_source().expect("source");
    let cursor_session = f
        .client(c)
        .create_cursor_session(&source)
        .expect("cursor session");
    let _capture_session = f
        .client(c)
        .cursor_session_get_capture_session(&cursor_session);
    f.roundtrip(c);
    f.client_mut(c).take_capture_session_events();

    let ptr = f.mt.state.seat.get_pointer().unwrap();
    let pos = ptr.current_location();
    let output = &f.mt.state.mon().output;
    let hotspot = f.mt.state.cursor.hotspot;
    f.mt.state
        .screencopy
        .update_cursor(Some(pos), hotspot, output);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_cursor_session_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CursorSessionEvent::Hotspot { .. })),
        "should receive hotspot event with enter, got {events:?}",
    );
}

#[test]
fn cursor_session_stopped_on_destroy() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);

    let source = f.client(c).create_output_capture_source().expect("source");
    let cursor_session = f
        .client(c)
        .create_cursor_session(&source)
        .expect("cursor session");
    let _capture_session = f
        .client(c)
        .cursor_session_get_capture_session(&cursor_session);
    f.roundtrip(c);
    f.client_mut(c).take_capture_session_events();

    cursor_session.destroy();
    f.client(c).flush();
    f.dispatch();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_capture_session_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CaptureSessionEvent::Stopped)),
        "inner capture session should receive stopped, got {events:?}",
    );
}
