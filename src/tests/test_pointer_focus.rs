use smithay::{
    backend::input::ButtonState,
    utils::{Logical, Point},
};

use super::{
    Fixture,
    test_pointer_lock::{client, open_two_windows, window_center, window_rect},
};
use crate::{
    config::{Action, Config, Rel},
    shell::MonitorsExt,
};

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    w
}

fn focused_window_point(f: &Fixture) -> Point<f64, Logical> {
    let id = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let rect =
        f.mt.state
            .monitors
            .window_rect(&f.mt.state.windows, id)
            .expect("mapped window");
    (rect.loc.x as f64 + 0.5, rect.loc.y as f64 + 0.5).into()
}

#[test]
fn pointer_focus_follows_the_displayed_layout() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = open_two_windows(&mut f, c);
    let (under, other) = if window_rect(&f, c, a).loc.x < window_rect(&f, c, b).loc.x {
        (a, b)
    } else {
        (b, a)
    };
    f.absolute_motion(window_center(&f, c, under));
    f.roundtrip(c);

    f.mt.handle_action(Action::Swap(Rel::Next));
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(under).surface),
        "while the new layout waits for the clients, the old one is visible",
    );

    f.client_mut(c).ack_and_commit_sized(a);
    f.client_mut(c).ack_and_commit_sized(b);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(other).surface),
        "once the new layout is displayed, the window under the pointer is focused",
    );
}

#[test]
fn tag_switch_refreshes_pointer_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.roundtrip(c);

    let w0 = open_window(&mut f, c);
    f.mt.handle_action(Action::FocusTag(1));
    let w1 = open_window(&mut f, c);

    f.absolute_motion(focused_window_point(&f));
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w1).surface),
        "the pointer starts on the window of the active tag",
    );

    f.mt.handle_action(Action::FocusTag(0));
    f.mt.advance_view_queues();
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w0).surface),
        "after a tag switch the pointer must focus the window now under it",
    );
}

#[test]
fn input_is_clipped_to_the_window_rect_like_the_render() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = open_two_windows(&mut f, c);
    let left = if window_rect(&f, c, a).loc.x < window_rect(&f, c, b).loc.x { a } else { b };
    let left_id = f.mt.state.surface_under(window_center(&f, c, left)).window;
    let rect = window_rect(&f, c, left);

    // a buffer wider than the tile
    f.client(c)
        .commit_buffer(left, rect.size.w + 100, rect.size.h);
    f.roundtrip(c);

    let in_gap = Point::new(
        (rect.loc.x + rect.size.w + 1) as f64,
        window_center(&f, c, left).y,
    );
    assert_ne!(
        f.mt.state.surface_under(in_gap).window,
        left_id,
        "the part of a buffer that is clipped takes no input",
    );
}

#[test]
fn a_dropped_window_has_the_pointer_focus() {
    let config = Config::parse(r#"(seats: {"seat0": ()}, binds: [([], Mouse(Left), Move)])"#)
        .expect("config");
    let mut f = Fixture::with_config(config);
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.mt.handle_action(Action::ToggleFloat);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    // a floating window can choose its size
    f.client(c).commit_buffer(w, 200, 150);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    let id = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let start = window_center(&f, c, w);
    let delta = Point::new(40.0, 30.0);

    f.absolute_motion(start);
    f.left_button(ButtonState::Pressed);
    f.relative_motion(delta);
    assert_eq!(
        f.mt.state.surface_under(start + delta).window,
        Some(id),
        "while dragged, the window is shown and hit where the drag put it",
    );
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        None,
        "while dragged, no client has the pointer",
    );

    f.left_button(ButtonState::Released);
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
        "the dropped window has the pointer at once, not the one behind it",
    );
    assert_eq!(
        f.mt.state.surface_under(start + delta).window,
        Some(id),
        "after the release, the window is hit at its new position",
    );
}
