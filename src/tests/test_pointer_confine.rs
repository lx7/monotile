use smithay::utils::Point;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime;

use super::{
    Fixture,
    client::ConstraintEvent,
    test_pointer_lock::{
        client, open_two_windows, open_window, pointer_location, window_center, window_rect,
    },
};

#[test]
fn confinement_activates_with_the_pointer_on_the_surface() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.absolute_motion(window_center(&f, c, w));

    f.client_mut(c).confine_pointer(w, Lifetime::Persistent);
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Confined],
    );
}

#[test]
fn confined_pointer_stays_on_the_surface() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let (a, _) = open_two_windows(&mut f, c);
    f.absolute_motion(window_center(&f, c, a));
    f.client_mut(c).confine_pointer(a, Lifetime::Persistent);
    f.roundtrip(c);
    let rect = window_rect(&f, c, a).to_f64();

    for delta in [(5000.0, 5000.0), (-5000.0, -5000.0)] {
        f.relative_motion(delta.into());
        f.roundtrip(c);

        assert!(rect.contains(pointer_location(&f)));
        assert_eq!(
            f.client(c).pointer_focus(),
            Some(&f.client(c).window(a).surface),
        );
    }
    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Confined],
    );
}

#[test]
fn absolute_motion_ends_the_confinement() {
    let mut f = Fixture::new();
    let c = client(&mut f);
    let w = open_window(&mut f, c);
    f.absolute_motion(window_center(&f, c, w));
    f.client_mut(c).confine_pointer(w, Lifetime::Persistent);
    f.roundtrip(c);

    f.absolute_motion(window_center(&f, c, w) + Point::new(10.0, 10.0));
    f.roundtrip(c);

    assert_eq!(
        f.client_mut(c).take_constraint_events(),
        vec![ConstraintEvent::Confined, ConstraintEvent::Unconfined],
    );
}
