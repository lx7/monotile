use super::Fixture;
use crate::config::Action;

fn open_fullscreen_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();
    w
}

#[test]
fn pointer_stays_on_fullscreen_window_at_far_edges() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.roundtrip(c);
    let w = open_fullscreen_window(&mut f, c);
    let pointer = f.mt.state.seat.get_pointer().unwrap();

    f.relative_motion((5000.0, 0.0).into());
    f.roundtrip(c);
    assert!(pointer.current_location().x < 1000.0);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
        "the pointer stays on the window at the right output edge",
    );

    f.relative_motion((0.0, 5000.0).into());
    f.roundtrip(c);
    assert!(pointer.current_location().y < 800.0);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w).surface),
        "the pointer stays on the window at the bottom output edge",
    );
}
