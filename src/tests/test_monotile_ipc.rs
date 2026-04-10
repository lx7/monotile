use super::Fixture;
use super::client::IpcEvent;
use super::ipc_client_protocol::monotile::zmonotile_seat_control_v1::Position;
use crate::config::Action;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn control_roundtrip(f: &mut Fixture, c: usize) {
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);
}

// ── Output Status ───────────────────────────────────

#[test]
fn output_status_initial_burst() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert_eq!(
        events[0],
        IpcEvent::TagCount(9),
        "first event should be tag_count"
    );

    let names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            IpcEvent::TagInfo { index, name } => Some((*index, name.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(names.len(), 9, "should get 9 tag names");
    assert_eq!(names[0], (0, "1".into()));
    assert_eq!(names[8], (8, "9".into()));

    assert!(
        events.contains(&IpcEvent::FocusedTags(1)),
        "tag 0 should be focused"
    );
    assert!(
        events.contains(&IpcEvent::OccupiedTags(0)),
        "no tags occupied"
    );
    assert!(events.contains(&IpcEvent::UrgentTags(0)), "no tags urgent");
    assert!(
        events.contains(&IpcEvent::Layout {
            name: "tile".into(),
            symbol: "[]=".into(),
        }),
        "should contain layout event, got {events:?}"
    );
}

#[test]
fn output_status_screencast_on_capture() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    open_window(&mut f, c);
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::Screencast(false)),
        "initial state is not screencasting, got {events:?}",
    );

    let handles = f.client_mut(c).take_foreign_toplevel_handles();
    let source = f
        .client(c)
        .create_toplevel_capture_source(&handles[0])
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::Screencast(true)),
        "screencast should be active, got {events:?}",
    );

    session.destroy();
    f.client(c).flush();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::Screencast(false)),
        "screencast should be cleared, got {events:?}",
    );
}

#[test]
fn output_status_screencast_on_output_capture() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    let source = f
        .client(c)
        .create_output_capture_source()
        .expect("create_source");
    let session = f
        .client(c)
        .create_capture_session(&source, false)
        .expect("create_session");
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::Screencast(true)),
        "output capture should set screencast, got {events:?}",
    );

    session.destroy();
    f.client(c).flush();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::Screencast(false)),
        "output capture stop should clear screencast, got {events:?}",
    );
}

#[test]
fn output_status_focused_tags_on_switch() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.mt.state.mon_mut().set_active_tag(2);
    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedTags(1 << 2)),
        "focused_tags should be tag 2, got {events:?}",
    );
}

#[test]
fn output_status_occupied_tags_on_map() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    open_window(&mut f, c);
    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::OccupiedTags(1)),
        "tag 0 should be occupied after mapping, got {events:?}",
    );
}

// ── Seat Status ─────────────────────────────────────

#[test]
fn seat_status_no_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedOutput),
        "should receive focused_output, got {events:?}"
    );
    let toplevel = events.iter().find_map(|e| match e {
        IpcEvent::FocusedToplevel { title, app_id, .. } => Some((title.clone(), app_id.clone())),
        _ => None,
    });
    assert_eq!(
        toplevel,
        Some((None, String::new())),
        "no focused toplevel: title should be None",
    );
}

#[test]
fn seat_status_focused_toplevel() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client(c).window(w).toplevel.set_title("hello".into());
    f.client(c).window(w).toplevel.set_app_id("test.app".into());
    f.client(c).flush();
    f.roundtrip(c);

    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedToplevel {
            title: Some("hello".into()),
            app_id: "test.app".into(),
            fullscreen: false,
            floating: false,
        }),
        "should reflect window metadata, got {events:?}"
    );
}

#[test]
fn seat_status_title_change() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client(c).window(w).toplevel.set_title("new title".into());
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    let title = events.iter().find_map(|e| match e {
        IpcEvent::FocusedToplevel { title, .. } => title.clone(),
        _ => None,
    });
    assert_eq!(
        title,
        Some("new title".into()),
        "title should update, got {events:?}"
    );
}

#[test]
fn seat_status_fullscreen_floating() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // toggle fullscreen
    f.mt.handle_action(Action::ToggleFullscreen);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            IpcEvent::FocusedToplevel {
                fullscreen: true,
                ..
            }
        )),
        "should be fullscreen, got {events:?}"
    );

    // unfullscreen
    f.mt.handle_action(Action::ToggleFullscreen);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            IpcEvent::FocusedToplevel {
                fullscreen: false,
                ..
            }
        )),
        "should not be fullscreen, got {events:?}"
    );

    // toggle floating
    f.mt.handle_action(Action::ToggleFloat);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, IpcEvent::FocusedToplevel { floating: true, .. })),
        "should be floating, got {events:?}"
    );
}

// ── Control: global ─────────────────────────────────

#[test]
fn control_exit() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client(c).control().exit();
    f.client(c).flush();
    f.roundtrip(c);
}

// ── Control: tag operations ─────────────────────────

#[test]
fn seat_control_focus_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client(c).seat_control().focus_tag(3);
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedTags(1 << 3)),
        "should switch to tag 3, got {events:?}",
    );
}

#[test]
fn seat_control_focus_previous_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // switch to tag 2
    f.client(c).seat_control().focus_tag(2);
    control_roundtrip(&mut f, c);
    f.client_mut(c).take_ipc_events();

    // toggle back
    f.client(c).seat_control().focus_previous_tag();
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedTags(1)),
        "should toggle back to tag 0, got {events:?}"
    );
}

#[test]
fn seat_control_set_toplevel_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_output_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // move window to tag 2
    f.client(c).seat_control().set_toplevel_tag(2);
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::OccupiedTags(0b100)),
        "tag 2 should be occupied, got {events:?}"
    );
    // tag 0 no longer occupied
    assert!(
        !events.contains(&IpcEvent::OccupiedTags(1)),
        "tag 0 should not be occupied"
    );
}

#[test]
fn seat_control_toggle_toplevel_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_output_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // toggle tag 2 on the window (window now on tags 0 and 2)
    f.client(c).seat_control().toggle_toplevel_tag(2);
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::OccupiedTags(0b101)),
        "tags 0 and 2 should be occupied, got {events:?}"
    );
}

// ── Control: toplevel operations ────────────────────

#[test]
fn seat_control_focus_toplevel() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w0 = open_window(&mut f, c);
    f.client(c).window(w0).toplevel.set_title("first".into());
    f.client(c).flush();
    f.roundtrip(c);

    let w1 = open_window(&mut f, c);
    f.client(c).window(w1).toplevel.set_title("second".into());
    f.client(c).flush();
    f.roundtrip(c);

    // second window should be focused now
    f.client_mut(c).bind_seat_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // focus next (wraps to first)
    f.client(c).seat_control().focus_toplevel(Position::Next);
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    let title = events.iter().find_map(|e| match e {
        IpcEvent::FocusedToplevel { title, .. } => title.clone(),
        _ => None,
    });
    assert_eq!(
        title,
        Some("first".into()),
        "focus should cycle to first, got {events:?}"
    );
}

#[test]
fn seat_control_swap() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    open_window(&mut f, c);

    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);

    // swap focused window with next
    let before: Vec<_> = f.mt.state.mon().tag().tiled.clone();
    f.client(c).seat_control().swap(Position::Next);
    f.client(c).flush();
    f.roundtrip(c);

    let after: Vec<_> = f.mt.state.mon().tag().tiled.clone();
    assert_ne!(before, after, "tiled order should change after swap");
}

#[test]
fn seat_control_close() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    assert!(
        f.mt.state.mon().tag().focused_id().is_some(),
        "window should be focused"
    );

    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);

    f.client(c).seat_control().close();
    f.client(c).flush();
    f.roundtrip(c);

    let ws = f.client(c).window(w);
    assert!(ws.closed, "window should receive close event");
}

#[test]
fn seat_control_toggle_float() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_seat_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client(c).seat_control().toggle_float();
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, IpcEvent::FocusedToplevel { floating: true, .. })),
        "should be floating, got {events:?}"
    );
}

#[test]
fn seat_control_toggle_fullscreen() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_seat_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client(c).seat_control().toggle_fullscreen();
    control_roundtrip(&mut f, c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            IpcEvent::FocusedToplevel {
                fullscreen: true,
                ..
            }
        )),
        "should be fullscreen, got {events:?}"
    );
}

// ── Control: layout operations ──────────────────────

#[test]
fn seat_control_adjust_main_count() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);

    let before = f.mt.state.mon().tag().layout.main_count;
    f.client(c).seat_control().adjust_main_count(1);
    f.client(c).flush();
    f.roundtrip(c);

    let after = f.mt.state.mon().tag().layout.main_count;
    assert_eq!(after, before + 1, "main_count should increase by 1");
}

#[test]
fn seat_control_set_main_ratio() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);

    f.client(c).seat_control().set_main_ratio(0.7);
    f.client(c).flush();
    f.roundtrip(c);

    let ratio = f.mt.state.mon().tag().layout.main_factor;
    assert!(
        (ratio - 0.7).abs() < 0.01,
        "main_factor should be ~0.7, got {ratio}"
    );
}

// ── Lifecycle ───────────────────────────────────────

#[test]
fn client_disconnect_no_crash() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);

    // drop the client; no panic expected
    f.drop_client(c);
    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
}

#[test]
fn two_clients_both_get_events() {
    let mut f = Fixture::new();
    let c1 = f.add_client();
    let c2 = f.add_client();

    f.client_mut(c1).bind_output_status();
    f.roundtrip(c1);
    f.client_mut(c2).bind_output_status();
    f.roundtrip(c2);
    f.client_mut(c1).take_ipc_events();
    f.client_mut(c2).take_ipc_events();

    f.mt.state.mon_mut().set_active_tag(4);
    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
    f.roundtrip(c1);
    f.roundtrip(c2);

    let e1 = f.client_mut(c1).take_ipc_events();
    let e2 = f.client_mut(c2).take_ipc_events();
    assert!(
        e1.contains(&IpcEvent::FocusedTags(1 << 4)),
        "client 1 should get update"
    );
    assert!(
        e2.contains(&IpcEvent::FocusedTags(1 << 4)),
        "client 2 should get update"
    );
}

#[test]
fn output_status_destroy_stops_events() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client_mut(c).destroy_output_status();
    f.roundtrip(c);

    f.mt.state.mon_mut().set_active_tag(5);
    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(events.is_empty(), "no events after destroy, got {events:?}");
}

#[test]
fn seat_status_destroy_stops_events() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client_mut(c).destroy_seat_status();
    f.roundtrip(c);

    f.mt.state.ipc.dirty = true;
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(events.is_empty(), "no events after destroy, got {events:?}");
}
