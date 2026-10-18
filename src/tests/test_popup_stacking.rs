use smithay::utils::Rectangle;
use wayland_protocols::xdg::shell::client::xdg_positioner::Gravity;

use super::{
    Fixture,
    test_pointer_lock::{client, open_two_windows, window_center, window_rect},
};

#[test]
fn popup_is_above_a_raised_neighbour_window() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = open_two_windows(&mut f, c);
    let (left, right) = if window_rect(&f, c, a).loc.x < window_rect(&f, c, b).loc.x {
        (a, b)
    } else {
        (b, a)
    };
    let left_rect = window_rect(&f, c, left);

    // the right window is focused
    f.absolute_motion(window_center(&f, c, right));
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(right).surface),
    );

    // a popup of the left window that extends 100px into the right window
    let anchor = Rectangle::new(
        (left_rect.size.w - 1, left_rect.size.h / 2).into(),
        (1, 1).into(),
    );
    let p = f
        .client_mut(c)
        .create_popup_at(left, anchor, Gravity::Right);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);

    let over_right_window = (
        (left_rect.loc.x + left_rect.size.w + 50) as f64,
        (left_rect.loc.y + left_rect.size.h / 2) as f64,
    );
    f.absolute_motion(over_right_window.into());
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).popup(p).surface),
        "the popup must receive the pointer although the window under it is raised",
    );
}
