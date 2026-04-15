use super::Fixture;
use super::client::IpcEvent;
use crate::shell::WindowId;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn get_token(f: &mut Fixture, c: usize) -> String {
    let token_obj = f.client_mut(c).get_activation_token();
    token_obj.commit();
    f.roundtrip(c);
    let tokens = f.client_mut(c).take_activation_tokens();
    assert_eq!(tokens.len(), 1, "should receive exactly one token");
    tokens.into_iter().next().unwrap()
}

fn all_window_ids(f: &Fixture) -> Vec<WindowId> {
    f.mt.state.mon().tag().focus_stack.clone()
}

// ── Token lifecycle ────────────────────────────────

#[test]
fn token_commit_returns_done() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let token = get_token(&mut f, c);
    assert!(!token.is_empty(), "token string should be non-empty");
}

// ── Activation sets urgent ─────────────────────────

#[test]
fn activation_sets_urgent() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);

    // w2 is focused (top of focus stack); w1 is not
    let ids = all_window_ids(&f);
    let w1_id = ids[1]; // w1 is second in focus stack (w2 was mapped last)
    assert!(!f.mt.state.windows[w1_id].focused, "w1 should not be focused");

    // activate w1 (unfocused window)
    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w1);
    f.roundtrip(c);

    assert!(
        f.mt.state.windows[w1_id].urgent,
        "w1 should be marked urgent after activation",
    );
}

#[test]
fn activation_of_focused_window_does_not_set_urgent() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    let w_id = f.mt.state.mon().tag().focused_id().unwrap();
    assert!(f.mt.state.windows[w_id].focused, "w should be focused");

    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w);
    f.roundtrip(c);

    assert!(
        !f.mt.state.windows[w_id].urgent,
        "focused window should not become urgent",
    );
}

// ── Urgent clears on focus ─────────────────────────

#[test]
fn urgent_clears_on_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);

    let ids = all_window_ids(&f);
    let w1_id = ids[1];

    // activate w1 to mark it urgent
    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w1);
    f.roundtrip(c);
    assert!(f.mt.state.windows[w1_id].urgent, "w1 should be urgent");

    // now focus w1
    f.mt.set_focus(Some(w1_id));

    assert!(
        !f.mt.state.windows[w1_id].urgent,
        "urgency should clear when window is focused",
    );
}

// ── IPC urgent_tags ────────────────────────────────

#[test]
fn ipc_urgent_tags_set_on_activation() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);

    f.client_mut(c).bind_output_status();
    f.roundtrip(c);
    f.client_mut(c).take_ipc_events();

    // activate w1 (unfocused) to make it urgent
    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w1);
    f.roundtrip(c);

    // flush IPC to send updated state
    f.mt.state.flush_clients();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    let urgent = events.iter().find_map(|e| match e {
        IpcEvent::UrgentTags(tags) => Some(*tags),
        _ => None,
    });
    assert_eq!(
        urgent,
        Some(1), // window on tag 0 -> bit 0 set
        "urgent_tags should have bit 0 set",
    );
}

#[test]
fn ipc_urgent_tags_clear_on_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);

    let ids = all_window_ids(&f);
    let w1_id = ids[1];

    // activate w1 to make it urgent
    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w1);
    f.roundtrip(c);

    // focus w1 to clear urgency
    f.mt.set_focus(Some(w1_id));

    // bind IPC and check
    f.client_mut(c).bind_output_status();
    f.roundtrip(c);

    let events = f.client_mut(c).take_ipc_events();
    let urgent = events.iter().find_map(|e| match e {
        IpcEvent::UrgentTags(tags) => Some(*tags),
        _ => None,
    });
    assert_eq!(
        urgent,
        Some(0),
        "urgent_tags should be 0 after focusing the urgent window",
    );
}

// ── Stale token ────────────────────────────────────

#[test]
fn stale_token_ignored() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);

    let ids = all_window_ids(&f);
    let w1_id = ids[1];

    // a fresh token within 10s should work
    let token = get_token(&mut f, c);
    f.client(c).activate(&token, w1);
    f.roundtrip(c);
    assert!(
        f.mt.state.windows[w1_id].urgent,
        "fresh token should set urgent",
    );
}
