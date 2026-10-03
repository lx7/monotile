use smithay::{
    backend::input::ButtonState,
    reexports::wayland_server::Resource,
    utils::{Logical, Point, Rectangle},
};
use wayland_client::Proxy;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime;

use super::{
    Fixture,
    client::{ConstraintEvent, RelativeMotionEvent},
};
use crate::{
    config::{Action, Config, Rel},
    shell::MonitorsExt,
};

fn client(f: &mut Fixture) -> usize {
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.client_mut(c).bind_relative_pointer();
    f.roundtrip(c);
    c
}

fn settle(f: &mut Fixture, c: usize, windows: &[usize]) {
    f.roundtrip(c);
    for &w in windows {
        f.client_mut(c).ack_and_commit_sized(w);
    }
    f.roundtrip(c);
    f.mt.advance_view_queues();
}

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    settle(f, c, &[w]);
    w
}

fn open_two_windows(f: &mut Fixture, c: usize) -> (usize, usize) {
    let a = open_window(f, c);
    let b = open_window(f, c);
    settle(f, c, &[a, b]);
    (a, b)
}

fn window_rect(f: &Fixture, c: usize, w: usize) -> Rectangle<i32, Logical> {
    let protocol_id = f.client(c).window(w).surface.id().protocol_id();
    let windows = &f.mt.state.windows;
    let id =
        f.mt.state
            .seat_mon()
            .tag()
            .window_ids()
            .into_iter()
            .find(|&id| {
                let toplevel = windows[id].window.toplevel().unwrap();
                toplevel.wl_surface().id().protocol_id() == protocol_id
            })
            .expect("window on the active tag");
    f.mt.state
        .monitors
        .window_rect(windows, id)
        .expect("mapped window")
}

fn window_center(f: &Fixture, c: usize, w: usize) -> Point<f64, Logical> {
    let rect = window_rect(f, c, w).to_f64();
    rect.loc + rect.size.to_point().downscale(2.0)
}

fn pointer_location(f: &Fixture) -> Point<f64, Logical> {
    f.mt.state.seat.get_pointer().unwrap().current_location()
}

fn relative_motion_to(f: &mut Fixture, target: Point<f64, Logical>) {
    f.relative_motion(target - pointer_location(f));
}

fn locked_window(f: &mut Fixture, c: usize, lifetime: Lifetime) -> usize {
    let w = open_window(f, c);
    f.absolute_motion(window_center(f, c, w));
    f.client_mut(c).lock_pointer(w, None, lifetime);
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
    f.client_mut(c).take_pointer_motions();
    w
}

fn locked_first_of_two_windows(f: &mut Fixture, c: usize, lifetime: Lifetime) -> (usize, usize) {
    let (a, b) = open_two_windows(f, c);
    f.absolute_motion(window_center(f, c, a));
    f.client_mut(c).lock_pointer(a, None, lifetime);
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
    f.client_mut(c).take_pointer_motions();
    (a, b)
}

#[test]
fn lock_activates_with_the_pointer_on_the_surface() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.absolute_motion(window_center(&f, c, w));

    f.client_mut(c).lock_pointer(w, None, Lifetime::Persistent);
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
}

#[test]
fn lock_waits_for_the_pointer() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = open_two_windows(&mut f, c);
    f.absolute_motion(window_center(&f, c, b));

    f.client_mut(c).lock_pointer(a, None, Lifetime::Persistent);
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);

    let center = window_center(&f, c, a);
    relative_motion_to(&mut f, center);
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
}

#[test]
fn lock_activates_only_inside_its_region() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.absolute_motion(window_center(&f, c, w));

    let region = Rectangle::new((0, 0).into(), (10, 10).into());
    f.client_mut(c)
        .lock_pointer(w, Some(region), Lifetime::Persistent);
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);

    f.relative_motion((1.0, 1.0).into());
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);

    let in_region = window_rect(&f, c, w).loc.to_f64() + Point::new(5.0, 5.0);
    relative_motion_to(&mut f, in_region);
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
}

#[test]
fn locked_pointer_gets_relative_motion_only() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    locked_window(&mut f, c, Lifetime::Persistent);
    let location = pointer_location(&f);

    f.relative_motion((10.0, 5.0).into());
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_relative_motions(),
        vec![RelativeMotionEvent {
            dx: 10.0,
            dy: 5.0,
            dx_unaccel: 10.0,
            dy_unaccel: 5.0,
        }],
    );
    assert_eq!(f.client_mut(c).take_pointer_motions(), 0);
    assert_eq!(pointer_location(&f), location);
}

#[test]
fn layout_change_keeping_the_window_under_the_pointer_keeps_the_lock() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    locked_window(&mut f, c, Lifetime::Persistent);

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);

    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);
    assert_eq!(f.client_mut(c).take_pointer_motions(), 0);
}

#[test]
fn layout_change_moving_the_window_away_ends_the_lock() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    locked_first_of_two_windows(&mut f, c, Lifetime::Persistent);

    f.mt.handle_action(Action::Swap(Rel::Next));
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );
}

#[test]
fn tag_switch_ends_the_lock() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    locked_window(&mut f, c, Lifetime::Persistent);

    f.mt.handle_action(Action::FocusTag(1));
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );
}

#[test]
fn cursor_warp_on_a_focus_change_ends_the_lock() {
    let config = Config::parse(r#"(seats: {"seat0": (cursor_warp: true)})"#).expect("config");
    let mut f = Fixture::with_config(config);
    let c = client(&mut f);
    locked_first_of_two_windows(&mut f, c, Lifetime::Persistent);

    f.mt.handle_action(Action::Focus(Rel::Next));
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );
}

#[test]
fn persistent_lock_activates_again_when_the_pointer_returns() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = locked_first_of_two_windows(&mut f, c, Lifetime::Persistent);

    f.absolute_motion(window_center(&f, c, b));
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );

    let center = window_center(&f, c, a);
    relative_motion_to(&mut f, center);
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
}

#[test]
fn oneshot_lock_stays_inactive_when_the_pointer_returns() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, b) = locked_first_of_two_windows(&mut f, c, Lifetime::Oneshot);

    f.absolute_motion(window_center(&f, c, b));
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );

    let center = window_center(&f, c, a);
    relative_motion_to(&mut f, center);
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);
}

#[test]
fn lock_does_not_activate_during_a_move_grab() {
    let config = Config::parse(r#"(seats: {"seat0": ()}, binds: [([], Mouse(Left), Move)])"#)
        .expect("config");
    let mut f = Fixture::with_config(config);
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.mt.handle_action(Action::ToggleFloat);
    settle(&mut f, c, &[w]);
    f.absolute_motion(window_center(&f, c, w));

    f.left_button(ButtonState::Pressed);
    f.client_mut(c).lock_pointer(w, None, Lifetime::Persistent);
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);

    f.relative_motion((5.0, 5.0).into());
    f.roundtrip(c);
    assert_eq!(f.client_mut(c).take_constraint_events(), vec![]);

    f.left_button(ButtonState::Released);
    f.relative_motion((5.0, 5.0).into());
    f.roundtrip(c);
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Locked],
    );
}

#[test]
fn absolute_motion_ends_the_lock() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = locked_window(&mut f, c, Lifetime::Persistent);

    f.absolute_motion(window_center(&f, c, w) + Point::new(10.0, 10.0));
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Unlocked],
    );
    assert_eq!(f.client_mut(c).take_pointer_motions(), 1);
}

#[test]
fn destroying_the_lock_frees_the_pointer() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    locked_window(&mut f, c, Lifetime::Persistent);
    let location = pointer_location(&f);

    f.client_mut(c).destroy_lock();
    f.roundtrip(c);
    f.relative_motion((10.0, 5.0).into());
    f.roundtrip(c);

    assert_eq!(f.client_mut(c).take_pointer_motions(), 1);
    assert_eq!(pointer_location(&f), location + Point::new(10.0, 5.0));
}
