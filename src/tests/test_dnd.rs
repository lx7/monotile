use super::Fixture;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

/// Open a window, bind the pointer + data device, and press the button on the
/// window to obtain a grab.
fn arm_drag(f: &mut Fixture, c: usize) -> (usize, u32) {
    f.client_mut(c).bind_pointer();
    f.client_mut(c).bind_data_device();
    f.roundtrip(c);

    let w = open_window(f, c);
    let id = f.mt.state.mon().tag().focused_id().unwrap();
    let surface = f.mt.state.windows[id]
        .window
        .toplevel()
        .unwrap()
        .wl_surface()
        .clone();

    f.pointer_press(&surface, (10.0, 10.0).into());
    f.roundtrip(c);
    let serial = f.client(c).pointer_serial();
    (w, serial)
}

#[test]
fn start_drag_with_icon_stores_it() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let (origin, serial) = arm_drag(&mut f, c);

    let source = f.client(c).create_data_source();
    let icon = f.client(c).create_surface();
    f.client(c)
        .start_drag(Some(&source), origin, Some(&icon), serial);
    f.roundtrip(c);

    assert!(
        f.mt.state.cursor.dnd_icon_surface().is_some(),
        "drag icon should be stored after start_drag with an icon",
    );
}

#[test]
fn start_drag_without_icon_stores_nothing() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let (origin, serial) = arm_drag(&mut f, c);

    let source = f.client(c).create_data_source();
    f.client(c).start_drag(Some(&source), origin, None, serial);
    f.roundtrip(c);

    assert!(
        f.mt.state.cursor.dnd_icon_surface().is_none(),
        "no icon should be stored for a drag started without one",
    );
}

#[test]
fn drop_clears_icon() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let (origin, serial) = arm_drag(&mut f, c);

    let source = f.client(c).create_data_source();
    let icon = f.client(c).create_surface();
    f.client(c)
        .start_drag(Some(&source), origin, Some(&icon), serial);
    f.roundtrip(c);
    assert!(
        f.mt.state.cursor.dnd_icon_surface().is_some(),
        "icon stored before drop",
    );

    f.pointer_release();
    assert!(
        f.mt.state.cursor.dnd_icon_surface().is_none(),
        "dropping (button release) should clear the drag icon",
    );
}

#[test]
fn start_drag_without_grab_is_denied() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.client_mut(c).bind_data_device();
    f.roundtrip(c);
    let origin = open_window(&mut f, c);

    // A stale serial should not start a drag.
    let source = f.client(c).create_data_source();
    let icon = f.client(c).create_surface();
    f.client(c)
        .start_drag(Some(&source), origin, Some(&icon), 999_999);
    f.roundtrip(c);

    assert!(
        f.mt.state.cursor.dnd_icon_surface().is_none(),
        "start_drag without a matching implicit grab must be denied",
    );
}
