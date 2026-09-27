use super::Fixture;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

/// Regression test for batched layer-shell commits (dwlb toggle-visibility).
///
/// When a layer-shell client re-creates its surface, it may batch the
/// initial empty commit and a buffer commit in one socket write. The server
/// processes both before the configure round-trips.
#[test]
fn batched_initial_commits() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let _w = open_window(&mut f, c);

    let ls = f.client_mut(c).create_layer_surface();
    f.client_mut(c).layer_commit(ls);
    f.client_mut(c).layer_attach_and_commit(ls);
    f.dispatch();

    f.assert_client_alive(c);
}

#[test]
fn exclusive_layer_keeps_window_focus_state() {
    let mut f = Fixture::new();
    let c = f.add_client();
    open_window(&mut f, c);
    open_window(&mut f, c);
    let focused = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let other =
        f.mt.state
            .seat_mon()
            .tag()
            .window_ids()
            .into_iter()
            .find(|&id| id != focused)
            .expect("second window");

    let ls = f.client_mut(c).create_layer_surface();
    f.client_mut(c).layer_exclusive_keyboard(ls);
    f.client_mut(c).layer_commit(ls);
    f.client_mut(c).layer_attach_and_commit(ls);
    f.dispatch();
    f.roundtrip(c);

    f.mt.set_keyboard_focus(Some(other));

    assert!(
        f.mt.state.windows[focused].focused,
        "the focused window keeps its border while a launcher holds the keyboard",
    );
    assert_eq!(
        f.mt.state.seat_mon().tag().focused_id(),
        Some(focused),
        "the request is ignored, closing the layer restores the same window",
    );
}
