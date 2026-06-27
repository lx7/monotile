use smithay::utils::{Logical, Rectangle};

use super::Fixture;
use crate::shell::WindowId;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn settle(f: &mut Fixture, c: usize, win: usize) {
    f.client_mut(c).ack_and_commit(win);
    f.roundtrip(c);
    f.mt.advance_view_queues();
}

fn views_len(f: &Fixture) -> usize {
    f.mt.state.mon().views.len()
}

fn front_tiled_rect(f: &Fixture, id: WindowId) -> Option<Rectangle<i32, Logical>> {
    f.mt.state
        .mon()
        .views
        .front()?
        .tiled
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.rect)
}

fn front_shows(f: &Fixture, id: WindowId) -> bool {
    f.mt.state
        .mon()
        .views
        .front()
        .is_some_and(|v| v.contains(id))
}

#[test]
fn closing_stack_window_holds_view_until_main_commits() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c); // main (client window 0)
    open_window(&mut f, c); // stack (client window 1)
    settle(&mut f, c, 0); // main acks the split

    assert_eq!(views_len(&f), 1, "layout settled");
    let main_id = f.mt.state.mon().views.front().unwrap().tiled[0].id;
    let main_before = front_tiled_rect(&f, main_id).unwrap();

    // destroy the stack window
    f.client_mut(c).destroy_window(1);
    f.roundtrip(c);

    assert_eq!(
        views_len(&f),
        2,
        "closing the stack window queues a held view"
    );
    assert_eq!(
        f.mt.state.mon().views.front().unwrap().tiled.len(),
        2,
        "held view still renders both tiles (destroyed one from its texture)",
    );
    assert_eq!(
        front_tiled_rect(&f, main_id),
        Some(main_before),
        "main must not resize until it commits",
    );

    // main acks + commits its resized buffer
    settle(&mut f, c, 0);

    assert_eq!(views_len(&f), 1, "queue advanced after main committed");
    let main_after = front_tiled_rect(&f, main_id).unwrap();
    assert!(
        main_after.size.w > main_before.size.w,
        "main grew to fill the freed space",
    );
}

#[test]
fn closing_lone_window_settles_immediately() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    settle(&mut f, c, 0);

    f.client_mut(c).destroy_window(0);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    assert_eq!(views_len(&f), 1, "no survivors to wait on");
    assert!(
        f.mt.state.mon().views.front().unwrap().tiled.is_empty(),
        "nothing left to draw",
    );
}

#[test]
fn tag_switch_holds_outgoing_until_incoming_commits() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let idx = f.mt.state.active_monitor;

    open_window(&mut f, c); // a (client 0)
    open_window(&mut f, c); // b (client 1), focused
    settle(&mut f, c, 0);

    // move b to tag 1, leaving a alone on tag 0; a grows
    f.mt.state.monitors[idx].move_to_tag(&mut f.mt.state.windows, 1);
    f.mt.recompute_layout(idx);
    f.roundtrip(c);
    settle(&mut f, c, 0);

    let a_id = f.mt.state.monitors[idx].tags[0].focus_stack[0];
    let b_id = f.mt.state.monitors[idx].tags[1].focus_stack[0];

    // switch to tag 1: b must grow split->full, so its view is held
    f.mt.state.monitors[idx].set_active_tag(1);
    f.mt.recompute_layout(idx);
    f.roundtrip(c);

    assert_eq!(views_len(&f), 2, "the incoming resize is held");
    assert!(front_shows(&f, a_id), "outgoing tag still displayed");
    assert!(!front_shows(&f, b_id), "incoming tag not shown yet");
    assert_eq!(
        f.mt.state.monitors[idx].active_tag, 1,
        "the model flips immediately",
    );

    // b commits its new size -> view flips
    settle(&mut f, c, 1);

    assert_eq!(views_len(&f), 1, "queue advanced once b committed");
    assert!(front_shows(&f, b_id), "now showing the incoming tag");
    assert!(!front_shows(&f, a_id), "outgoing tag gone");
}

#[test]
fn tag_switch_without_resize_settles_immediately() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let idx = f.mt.state.active_monitor;

    open_window(&mut f, c);
    settle(&mut f, c, 0);

    // switch to an empty tag - nothing to resize, nothing to hold
    f.mt.state.monitors[idx].set_active_tag(1);
    f.mt.recompute_layout(idx);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    assert_eq!(views_len(&f), 1, "empty incoming tag needs no hold");
    assert!(
        f.mt.state.mon().views.front().unwrap().tiled.is_empty(),
        "the new tag is presented immediately",
    );
}

#[test]
fn idle_inhibit_follows_visibility() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let idx = f.mt.state.active_monitor;

    open_window(&mut f, c);
    settle(&mut f, c, 0);
    let id = f.mt.state.monitors[idx].tags[0].focus_stack[0];

    // simulate an idle inhibitor on the window's surface
    let surface = f.mt.state.windows[id]
        .window
        .toplevel()
        .unwrap()
        .wl_surface()
        .clone();
    f.mt.state.idle_inhibitors.push(surface);

    f.mt.state.refresh_idle_inhibit();
    assert!(
        f.mt.state.idle_notifier_state.is_inhibited(),
        "a visible inhibitor blocks idle",
    );

    // hide it on another tag
    f.mt.state.monitors[idx].set_active_tag(1);
    f.mt.recompute_layout(idx);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    f.mt.state.refresh_idle_inhibit();
    assert!(
        !f.mt.state.idle_notifier_state.is_inhibited(),
        "a hidden inhibitor must not block idle",
    );

    // bring it back
    f.mt.state.monitors[idx].set_active_tag(0);
    f.mt.recompute_layout(idx);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    f.mt.state.refresh_idle_inhibit();
    assert!(
        f.mt.state.idle_notifier_state.is_inhibited(),
        "visible again blocks idle",
    );

    // locked session is never inhibited, even with a visible inhibitor
    f.mt.state.locked = true;
    f.mt.state.refresh_idle_inhibit();
    assert!(
        !f.mt.state.idle_notifier_state.is_inhibited(),
        "a locked session is not inhibited",
    );
}
