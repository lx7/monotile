use super::Fixture;
use super::client::IpcEvent;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

#[test]
fn status_manager_binds() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(!events.is_empty(), "should receive initial burst");
}

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
            IpcEvent::TagName { index, name } => Some((*index, name.clone())),
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
}

#[test]
fn output_status_focused_tags_on_switch() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.mt.state.mon_mut().set_active_tag(2);
    f.mt.state.ipc.mark_dirty();
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
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::OccupiedTags(1)),
        "tag 0 should be occupied after mapping, got {events:?}",
    );
}

#[test]
fn seat_status_no_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_seat_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
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
fn control_exit() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client(c).control().exit();
    f.client(c).flush();
    f.roundtrip(c);
}

#[test]
fn seat_control_focus_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.client_mut(c).bind_seat_control();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    f.client(c).seat_control().focus_tag(3);
    f.client(c).flush();
    f.roundtrip(c);

    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(
        events.contains(&IpcEvent::FocusedTags(1 << 3)),
        "should switch to tag 3, got {events:?}",
    );
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
fn client_disconnect_no_crash() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);

    // drop the client; no panic expected
    f.drop_client(c);
    f.mt.state.ipc.mark_dirty();
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
    f.mt.state.ipc.mark_dirty();
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
    f.mt.state.ipc.mark_dirty();
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

    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    assert!(events.is_empty(), "no events after destroy, got {events:?}");
}
