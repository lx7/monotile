use super::Fixture;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

#[test]
fn closing_stack_window_holds_layout_until_main_acks() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c); // main (window 0)
    open_window(&mut f, c); // stack (window 1)

    // settle the layout
    f.client_mut(c).ack_and_commit(0);
    f.roundtrip(c);
    f.mt.unblock_ready_transitions();

    let tiles = f.mt.state.mon().tag().layout.tiles();
    assert_eq!(tiles.len(), 2, "expected two tiled windows");
    let main_id = tiles[0].id;
    let main_geo_before = f.mt.state.windows[main_id].render_geo;

    // destroy stack window
    f.client_mut(c).destroy_window(1);
    f.roundtrip(c);

    // old layout should still be consistent
    let mon = f.mt.state.mon();
    assert!(
        mon.transition.is_some(),
        "closing the stack window should start a transition",
    );
    let t = mon.transition.as_ref().unwrap();
    assert_eq!(
        t.closing.len(),
        1,
        "destroyed window texture kept for rendering"
    );
    assert_eq!(
        f.mt.state.windows[main_id].render_geo, main_geo_before,
        "main window must not change geo until it commits the resize",
    );

    // main acks + commits its resized buffer
    f.client_mut(c).ack_and_commit(0);
    f.roundtrip(c);
    f.mt.unblock_ready_transitions();

    // transition released, stack window gone, main fills the area
    assert!(
        f.mt.state.mon().transition.is_none(),
        "transition clears after main acked",
    );
    let main_after = f.mt.state.windows[main_id].render_geo;
    assert!(
        main_after.size.w > main_geo_before.size.w,
        "main should grow to fill the space",
    );
    assert_eq!(
        main_after,
        f.mt.state.mon().tag().layout.position_of(main_id).unwrap(),
        "main render_geo should match its new tiled position",
    );
}

#[test]
fn closing_single_window_starts_no_transition() {
    let mut f = Fixture::new();
    let c = f.add_client();

    open_window(&mut f, c);
    assert_eq!(f.mt.state.mon().tag().layout.tiles().len(), 1);

    f.client_mut(c).destroy_window(0);
    f.roundtrip(c);

    assert!(
        f.mt.state.mon().transition.is_none(),
        "closing the single window needs no transition",
    );
    assert_eq!(
        f.mt.state.mon().tag().layout.tiles().len(),
        0,
        "no tiled windows remain",
    );
}
