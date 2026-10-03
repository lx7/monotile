use super::{Fixture, client::RelativeMotionEvent};

fn client_with_pointer_on_window(f: &mut Fixture) -> usize {
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.client_mut(c).bind_relative_pointer();
    f.roundtrip(c);

    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    f.absolute_motion((500.0, 400.0).into());
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
    );
    f.client_mut(c).take_pointer_motions();
    c
}

#[test]
fn relative_motion_sends_relative_and_pointer_motion() {
    let mut f = Fixture::new();
    let c = client_with_pointer_on_window(&mut f);

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
    assert_eq!(f.client_mut(c).take_pointer_motions(), 1);
}

#[test]
fn absolute_motion_sends_pointer_motion_only() {
    let mut f = Fixture::new();
    let c = client_with_pointer_on_window(&mut f);

    f.absolute_motion((510.0, 405.0).into());
    f.roundtrip(c);

    assert_eq!(f.client_mut(c).take_relative_motions(), vec![]);
    assert_eq!(f.client_mut(c).take_pointer_motions(), 1);
}

#[test]
fn relative_motion_at_the_output_edge_is_unclipped() {
    let mut f = Fixture::new();
    let c = client_with_pointer_on_window(&mut f);

    f.relative_motion((5000.0, 0.0).into());
    f.roundtrip(c);

    let pointer = f.mt.state.seat.get_pointer().unwrap();
    assert!(pointer.current_location().x < 1000.0);
    assert_eq!(
        f.client_mut(c).take_relative_motions(),
        vec![RelativeMotionEvent {
            dx: 5000.0,
            dy: 0.0,
            dx_unaccel: 5000.0,
            dy_unaccel: 0.0,
        }],
    );
}
