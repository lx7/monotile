use smithay::utils::{Logical, Point};

use super::Fixture;
use crate::{
    config::{Action, Config},
    shell::{MonitorsExt, WindowId},
};

fn warp_enabled() -> Config {
    Config::parse(r#"(seats: {"seat0": (cursor_warp: true)})"#).expect("config")
}

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    w
}

fn pointer_location(f: &Fixture) -> Point<f64, Logical> {
    f.mt.state.seat.get_pointer().unwrap().current_location()
}

fn set_pointer(f: &Fixture, pos: Point<f64, Logical>) {
    f.mt.state.seat.get_pointer().unwrap().set_location(pos);
}

fn window_center(f: &Fixture, id: WindowId) -> Point<f64, Logical> {
    let rect =
        f.mt.state
            .monitors
            .window_rect(&f.mt.state.windows, id)
            .expect("mapped window");
    (
        rect.loc.x as f64 + rect.size.w as f64 / 2.0,
        rect.loc.y as f64 + rect.size.h as f64 / 2.0,
    )
        .into()
}

fn two_windows(f: &mut Fixture, c: usize) -> (WindowId, WindowId) {
    open_window(f, c);
    open_window(f, c);
    let focused = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let other =
        f.mt.state
            .seat_mon()
            .tag()
            .window_ids()
            .into_iter()
            .find(|&id| id != focused)
            .expect("second window");
    (focused, other)
}

#[test]
fn focus_change_warps_the_cursor() {
    let mut f = Fixture::with_config(warp_enabled());
    let c = f.add_client();
    let (focused, other) = two_windows(&mut f, c);

    set_pointer(&f, window_center(&f, focused));
    f.mt.set_keyboard_focus(Some(other));

    assert_eq!(
        pointer_location(&f),
        window_center(&f, other),
        "focusing another window must warp the cursor",
    );
}

#[test]
fn no_warp_when_the_cursor_is_already_on_the_window() {
    let mut f = Fixture::with_config(warp_enabled());
    let c = f.add_client();
    let (_, other) = two_windows(&mut f, c);

    let inside = window_center(&f, other) + Point::from((3.0, 3.0));
    set_pointer(&f, inside);
    f.mt.set_keyboard_focus(Some(other));

    assert_eq!(
        pointer_location(&f),
        inside,
        "cursor must not be moved when already on the window",
    );
}

#[test]
fn disabled_cursor_warp_leaves_the_cursor() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let (focused, other) = two_windows(&mut f, c);

    let start = window_center(&f, focused);
    set_pointer(&f, start);
    f.mt.set_keyboard_focus(Some(other));

    assert_eq!(
        pointer_location(&f),
        start,
        "cursor_warp defaults to off and must not move the cursor",
    );
}

#[test]
fn tag_switch_warps_the_cursor() {
    let mut f = Fixture::with_config(warp_enabled());
    let c = f.add_client();
    open_window(&mut f, c);
    let first = f.mt.state.seat_mon().tag().focused_id().expect("focus");

    f.mt.handle_action(Action::FocusTag(1));
    open_window(&mut f, c);
    let second = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    set_pointer(&f, window_center(&f, second));

    f.mt.handle_action(Action::FocusTag(0));

    assert_eq!(
        pointer_location(&f),
        window_center(&f, first),
        "switching tags must warp the cursor",
    );
}

#[test]
fn opening_a_window_warps_the_cursor() {
    let mut f = Fixture::with_config(warp_enabled());
    let c = f.add_client();
    open_window(&mut f, c);
    let first = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    set_pointer(&f, window_center(&f, first));

    open_window(&mut f, c);
    let second = f.mt.state.seat_mon().tag().focused_id().expect("focus");

    assert_eq!(
        pointer_location(&f),
        window_center(&f, second),
        "a newly opened window must take the cursor",
    );
}
