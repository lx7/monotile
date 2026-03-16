use super::Fixture;
use super::client::DwlEvent;
use crate::config::Action;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

/// Tag state constants matching zdwl_ipc_output_v2::TagState
const TAG_NONE: u32 = 0;
const TAG_ACTIVE: u32 = 1;
const TAG_URGENT: u32 = 2;

// ── A. Manager Bind ─────────────────────────────────

#[test]
fn manager_sends_tags_on_bind() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);
    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Tags(9)),
        "should receive tags(9), got {events:?}"
    );
    assert!(
        events.contains(&DwlEvent::ManagerLayout("tile".into())),
        "should receive layout on bind, got {events:?}"
    );
}

// ── B. Initial State on get_output ──────────────────

#[test]
fn output_initial_burst_structure() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();

    let tag_count = events
        .iter()
        .filter(|e| matches!(e, DwlEvent::Tag { .. }))
        .count();
    assert_eq!(tag_count, 9, "should have 9 tag events");

    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::Layout(_))),
        "missing layout index"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DwlEvent::LayoutSymbol(_))),
        "missing layout_symbol"
    );
    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::Title(_))),
        "missing title"
    );
    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::AppId(_))),
        "missing appid"
    );
    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::Active(_))),
        "missing active"
    );
    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::Fullscreen(_))),
        "missing fullscreen"
    );
    assert!(
        events.iter().any(|e| matches!(e, DwlEvent::Floating(_))),
        "missing floating"
    );

    // frame must be the last event in the burst
    assert_eq!(
        events.last(),
        Some(&DwlEvent::Frame),
        "frame must terminate the burst"
    );
}

#[test]
fn output_initial_tag_states() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DwlEvent::Tag {
                tag,
                state,
                clients,
                focused,
            } => Some((*tag, *state, *clients, *focused)),
            _ => None,
        })
        .collect();

    // tag 0 is active, rest are none. no windows.
    assert_eq!(tags[0], (0, TAG_ACTIVE, 0, 1), "tag 0: active, focused");
    for i in 1..9 {
        assert_eq!(
            tags[i],
            (i as u32, TAG_NONE, 0, 0),
            "tag {i}: none, not focused"
        );
    }
}

#[test]
fn output_initial_active_flag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Active(1)),
        "single output should be active"
    );
}

// ── C. State Change Events ──────────────────────────

#[test]
fn tag_switch_updates_state() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    f.mt.state.mon_mut().set_active_tag(3);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DwlEvent::Tag { tag, state, .. } => Some((*tag, *state)),
            _ => None,
        })
        .collect();

    assert!(
        tags.contains(&(3, TAG_ACTIVE)),
        "tag 3 should be active, got {tags:?}"
    );
    assert!(
        tags.contains(&(0, TAG_NONE)),
        "tag 0 should be none, got {tags:?}"
    );
    assert_eq!(events.last(), Some(&DwlEvent::Frame), "ends with frame");
}

#[test]
fn window_map_updates_clients_count() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    open_window(&mut f, c);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tag0 = events.iter().find_map(|e| match e {
        DwlEvent::Tag {
            tag: 0,
            clients,
            focused,
            ..
        } => Some((*clients, *focused)),
        _ => None,
    });
    assert_eq!(tag0, Some((1, 1)), "tag 0 should have 1 client, focused");
}

#[test]
fn title_appid_reflect_focused_window() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client(c).window(w).toplevel.set_title("hello".into());
    f.client(c).window(w).toplevel.set_app_id("test.app".into());
    f.client(c).flush();
    f.roundtrip(c);

    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Title("hello".into())),
        "title should be 'hello', got {events:?}"
    );
    assert!(
        events.contains(&DwlEvent::AppId("test.app".into())),
        "appid should be 'test.app', got {events:?}"
    );
}

#[test]
fn fullscreen_floating_flags() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    // toggle fullscreen
    f.mt.handle_action(Action::ToggleFullscreen);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Fullscreen(1)),
        "fullscreen should be 1, got {events:?}"
    );

    // unfullscreen
    f.mt.handle_action(Action::ToggleFullscreen);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Fullscreen(0)),
        "fullscreen should be 0 after untoggle, got {events:?}"
    );

    f.mt.handle_action(Action::ToggleFloat);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(
        events.contains(&DwlEvent::Floating(1)),
        "floating should be 1, got {events:?}"
    );
}

// ── D. Control Requests ─────────────────────────────

#[test]
fn set_tags_switches_tag() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    // FocusTag(3) via click (toggle_tagset=0)
    f.client(c).dwl_output().set_tags(1 << 3, 0);
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DwlEvent::Tag { tag, state, .. } => Some((*tag, *state)),
            _ => None,
        })
        .collect();
    assert!(
        tags.contains(&(3, TAG_ACTIVE)),
        "should switch to tag 3, got {tags:?}"
    );

    // FocusTag(5) via click (toggle_tagset=1, different tag)
    // should switch to that tag - ignore toggle
    f.client(c).dwl_output().set_tags(1 << 5, 1);
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DwlEvent::Tag { tag, state, .. } => Some((*tag, *state)),
            _ => None,
        })
        .collect();
    assert!(
        tags.contains(&(5, TAG_ACTIVE)),
        "should switch to tag 5, got {tags:?}"
    );
}

#[test]
fn set_tags_same_tag_is_noop() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    // switch to tag 2
    f.mt.state.mon_mut().set_active_tag(2);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    // Click current tag (2) with toggle_tagset=1 - should stay on tag 2
    f.client(c).dwl_output().set_tags(1 << 2, 1);
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DwlEvent::Tag { tag, state, .. } => Some((*tag, *state)),
            _ => None,
        })
        .collect();
    // No tag change - either empty or tag 2 still active
    assert!(
        tags.is_empty() || tags.contains(&(2, TAG_ACTIVE)),
        "should stay on tag 2, got {tags:?}"
    );
}

#[test]
fn set_client_tags_move() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    // move window to tag 2
    f.client(c).dwl_output().set_client_tags(0, 1 << 2);
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    // tag 0 should have 0 clients
    let tag0 = events.iter().find_map(|e| match e {
        DwlEvent::Tag {
            tag: 0, clients, ..
        } => Some(*clients),
        _ => None,
    });
    assert_eq!(tag0, Some(0), "tag 0 should have 0 clients after move");
}

#[test]
fn set_client_tags_toggle() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    // ToggleTag(2)
    f.client(c).dwl_output().set_client_tags(0xFFFFFFFF, 1 << 2);
    f.client(c).flush();
    f.roundtrip(c);
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    // window should now be visible on tag 2 as well
    let tag2 = events.iter().find_map(|e| match e {
        DwlEvent::Tag {
            tag: 2, clients, ..
        } => Some(*clients),
        _ => None,
    });
    assert_eq!(
        tag2,
        Some(1),
        "tag 2 should have 1 client after toggle, got {events:?}"
    );
}

// ── E. Lifecycle ────────────────────────────────────

#[test]
fn release_stops_events() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);
    f.client_mut(c).take_dwl_events();

    f.client_mut(c).destroy_dwl_output();
    f.roundtrip(c);

    f.mt.state.mon_mut().set_active_tag(5);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    assert!(events.is_empty(), "no events after release, got {events:?}");
}

#[test]
fn client_disconnect_no_crash() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    f.drop_client(c);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
}

#[test]
fn two_clients_both_receive() {
    let mut f = Fixture::new();
    let c1 = f.add_client();
    let c2 = f.add_client();

    f.client_mut(c1).bind_dwl_output();
    f.roundtrip(c1);
    f.client_mut(c2).bind_dwl_output();
    f.roundtrip(c2);
    f.client_mut(c1).take_dwl_events();
    f.client_mut(c2).take_dwl_events();

    f.mt.state.mon_mut().set_active_tag(4);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c1);
    f.roundtrip(c2);

    let e1 = f.client_mut(c1).take_dwl_events();
    let e2 = f.client_mut(c2).take_dwl_events();

    let has_tag4 = |events: &[DwlEvent]| {
        events
            .iter()
            .any(|e| matches!(e, DwlEvent::Tag { tag: 4, state, .. } if *state == TAG_ACTIVE))
    };
    assert!(has_tag4(&e1), "client 1 should see tag 4 active");
    assert!(has_tag4(&e2), "client 2 should see tag 4 active");
}

// ── F. Frame Batching ───────────────────────────────

#[test]
fn frame_terminates_every_batch() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_dwl_output();
    f.roundtrip(c);

    // initial burst should end with frame
    let events = f.client_mut(c).take_dwl_events();
    let frame_count = events
        .iter()
        .filter(|e| matches!(e, DwlEvent::Frame))
        .count();
    assert_eq!(frame_count, 1, "initial burst: exactly 1 frame");
    assert_eq!(events.last(), Some(&DwlEvent::Frame));

    // state update should also end with frame
    f.mt.state.mon_mut().set_active_tag(1);
    f.mt.state.ipc.mark_dirty();
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_dwl_events();
    let frame_count = events
        .iter()
        .filter(|e| matches!(e, DwlEvent::Frame))
        .count();
    assert_eq!(frame_count, 1, "update: exactly 1 frame");
    assert_eq!(events.last(), Some(&DwlEvent::Frame));
}
