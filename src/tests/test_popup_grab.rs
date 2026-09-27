use super::Fixture;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn open_window_with_popup(f: &mut Fixture, c: usize) -> (usize, usize) {
    let w = open_window(f, c);
    f.client_mut(c).bind_pointer();
    f.client_mut(c).bind_keyboard();
    f.roundtrip(c);

    let id = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let surface = f.mt.state.windows[id]
        .window
        .toplevel()
        .expect("toplevel")
        .wl_surface()
        .clone();
    f.pointer_press(&surface, (10.0, 10.0).into());
    f.roundtrip(c);
    let serial = f.client(c).pointer_serial();

    let p = f.client_mut(c).create_popup(w);
    f.client(c).popup_grab(p, serial);
    f.client(c).popup_commit(p);
    f.roundtrip(c);

    assert!(
        f.mt.state
            .seat
            .get_keyboard()
            .expect("keyboard")
            .is_grabbed(),
        "the popup should hold a keyboard grab",
    );
    (w, p)
}

#[test]
fn locking_takes_the_keyboard_from_a_popup_grab() {
    let mut f = Fixture::new();
    let c = f.add_client();
    open_window_with_popup(&mut f, c);

    let _lock = super::test_session_lock::lock_session(&mut f);
    f.roundtrip(c);

    assert!(
        f.client(c).keyboard_focus().is_none(),
        "the lock must take the keyboard back from the popup grab",
    );
}

#[test]
fn focus_change_closes_the_popup() {
    let mut f = Fixture::new();
    let c = f.add_client();
    open_window(&mut f, c);
    let (_w, p) = open_window_with_popup(&mut f, c);

    let focused = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let other =
        f.mt.state
            .seat_mon()
            .tag()
            .window_ids()
            .into_iter()
            .find(|&id| id != focused)
            .expect("second window");

    f.mt.set_keyboard_focus(Some(other));
    f.roundtrip(c);

    assert!(
        f.client(c).popup_done(p),
        "the client must be told to close the popup",
    );
    assert_eq!(
        f.mt.state.windows.focused,
        Some(other),
        "the keyboard moves to the requested window",
    );
}
