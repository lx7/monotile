use smithay::{
    backend::input::ButtonState,
    utils::{Point, Rectangle},
};
use wayland_protocols::xdg::shell::client::xdg_positioner::Gravity;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;

use super::{
    Fixture,
    test_pointer_lock::{client, open_two_windows, open_window, window_center, window_rect},
};
use crate::{config::Action, shell::WindowId};

fn open_layer_bar(f: &mut Fixture, c: usize, layer: Layer) -> usize {
    let ls = f.client_mut(c).create_layer_surface_on(layer);
    f.client_mut(c).layer_commit(ls);
    f.roundtrip(c);
    f.client(c).layer_ack_and_commit_sized(ls, 1000, 30);
    f.roundtrip(c);
    ls
}

#[test]
fn bottom_layer_popup_is_above_windows() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    let ls = open_layer_bar(&mut f, c, Layer::Bottom);

    // a popup below the bar, over the window
    let anchor = Rectangle::new((500, 29).into(), (1, 1).into());
    let p = f
        .client_mut(c)
        .create_layer_popup_at(ls, anchor, Gravity::Bottom);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);

    f.absolute_motion((500.0, 80.0).into());
    f.roundtrip(c);
    assert_ne!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
    );
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).popup(p).surface),
        "a bottom-layer popup is placed above windows",
    );
}

#[test]
fn overlay_layer_is_above_window_popups() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    open_layer_bar(&mut f, c, Layer::Overlay);

    // a popup of the window that reaches up into the bar
    let anchor = Rectangle::new((500, 50).into(), (1, 1).into());
    let p = f.client_mut(c).create_popup_at(w, anchor, Gravity::Top);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);

    f.absolute_motion((500.0, 10.0).into());
    f.roundtrip(c);
    assert_ne!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).popup(p).surface),
        "a launcher on the overlay layer stays above window menus",
    );
}

#[test]
fn window_popup_belongs_to_its_window() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    let id = f.mt.state.seat_mon().tag().focused_id().expect("focus");
    let size = window_rect(&f, c, w).size;

    // a grab-less popup, like a tooltip
    let anchor = Rectangle::new((size.w / 2, size.h / 2).into(), (1, 1).into());
    let p = f
        .client_mut(c)
        .create_popup_at(w, anchor, Gravity::BottomRight);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);

    let pos = window_center(&f, c, w) + Point::new(20.0, 20.0);
    f.absolute_motion(pos);
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).popup(p).surface),
    );
    assert_eq!(
        f.mt.state.surface_under(pos).window,
        Some(id),
        "a click on a window's popup raises and focuses the window",
    );
}

#[test]
fn popup_subsurface_receives_the_pointer() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    let rect = window_rect(&f, c, w);

    let anchor = Rectangle::new((10, 10).into(), (1, 1).into());
    let p = f
        .client_mut(c)
        .create_popup_at(w, anchor, Gravity::BottomRight);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);
    let sub = f.client(c).add_popup_subsurface(p, (50, 50));
    f.roundtrip(c);

    f.absolute_motion(((rect.loc.x + 10 + 60) as f64, (rect.loc.y + 10 + 60) as f64).into());
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&sub),
        "a subsurface of a popup is drawn and must receive the pointer",
    );
}

#[test]
fn top_layer_is_hidden_from_input_over_fullscreen() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    open_layer_bar(&mut f, c, Layer::Top);

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    f.absolute_motion((500.0, 10.0).into());
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
        "the bar is not drawn over a fullscreen window, so it must not take the pointer",
    );
}

#[test]
fn top_layer_popup_is_hidden_from_input_over_fullscreen() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    let ls = open_layer_bar(&mut f, c, Layer::Top);
    let anchor = Rectangle::new((500, 29).into(), (1, 1).into());
    let p = f
        .client_mut(c)
        .create_layer_popup_at(ls, anchor, Gravity::Bottom);
    f.client(c).popup_commit(p);
    f.roundtrip(c);
    f.client(c).popup_ack_and_commit(p);
    f.roundtrip(c);

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    f.absolute_motion((500.0, 80.0).into());
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
        "the popup of a hidden layer is hidden with it",
    );
}

#[test]
fn clicked_floating_window_is_raised_in_the_view() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = open_two_windows(&mut f, c);
    let ids: Vec<WindowId> = f.mt.state.seat_mon().tag().window_ids();
    for (i, &id) in ids.iter().enumerate() {
        let we = &mut f.mt.state.windows[id];
        we.set_floating(true);
        we.float_geo = Rectangle::new((100 + 200 * i as i32, 100).into(), (300, 300).into());
    }
    let output = f.mt.state.seat_mon().output.clone();
    f.mt.recompute_layout(&output);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(a);
    f.client_mut(c).ack_and_commit_sized(b);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    let floating = f.mt.state.seat_mon().views.front().floating.clone();
    let lower = floating[0];
    let lower_rect = f.mt.state.windows[lower].float_geo;
    let upper_rect = f.mt.state.windows[floating[1]].float_geo;
    let only_lower = if lower_rect.loc.x < upper_rect.loc.x {
        (lower_rect.loc.x + 50, 200)
    } else {
        (lower_rect.loc.x + lower_rect.size.w - 50, 200)
    };

    f.absolute_motion((only_lower.0 as f64, only_lower.1 as f64).into());
    f.left_button(ButtonState::Pressed);
    f.left_button(ButtonState::Released);

    let newest = f.mt.state.seat_mon().views.back().unwrap();
    assert_eq!(
        newest.floating.last().copied(),
        Some(lower),
        "the clicked window is on top in the view that will be drawn next",
    );
    assert!(
        newest.blocker.is_committed(),
        "a raise changes no size, so its view waits for no client",
    );
}

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
