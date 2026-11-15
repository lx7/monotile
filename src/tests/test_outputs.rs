use smithay::{
    desktop::layer_map_for_output,
    output::{Output, Scale},
    utils::Transform,
};

use super::{
    Fixture,
    client::{Configure, CursorSessionEvent, OutputState},
};
use crate::{config::Action, shell::SeatExt};

fn summary(outputs: &[OutputState]) -> Vec<(&str, (i32, i32))> {
    outputs.iter().map(|o| (o.name.as_str(), o.mode)).collect()
}

pub(super) fn set_output_state(f: &mut Fixture, transform: Transform, scale: f64) -> Output {
    let output = f.mt.state.seat.active_output();
    output.change_current_state(None, Some(transform), Some(Scale::Fractional(scale)), None);
    layer_map_for_output(&output).arrange();
    f.mt.recompute_layout(&output);
    output
}

fn fullscreen_size(f: &mut Fixture) -> (i32, i32) {
    let c = f.add_client();
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit_sized(w);
    f.roundtrip(c);
    f.mt.advance_view_queues();

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    let configures = f.client_mut(c).take_configures(w);
    let Configure { width, height, .. } = configures.last().expect("fullscreen configure");
    (*width, *height)
}

#[test]
fn fullscreen_window_fills_a_rotated_output() {
    let mut f = Fixture::new();
    set_output_state(&mut f, Transform::_90, 1.0);
    assert_eq!(fullscreen_size(&mut f), (800, 1000));
}

#[test]
fn fullscreen_window_fills_a_scaled_output() {
    let mut f = Fixture::new();
    set_output_state(&mut f, Transform::Normal, 2.0);
    assert_eq!(fullscreen_size(&mut f), (500, 400));
}

#[test]
fn pointer_is_clamped_to_a_rotated_output() {
    let mut f = Fixture::new();
    set_output_state(&mut f, Transform::_90, 1.0);
    let pointer = f.mt.state.seat.get_pointer().unwrap();

    f.relative_motion((5000.0, 5000.0).into());

    let location = pointer.current_location();
    assert!((799.0..800.0).contains(&location.x), "x is {}", location.x);
    assert!((999.0..1000.0).contains(&location.y), "y is {}", location.y);
}

#[test]
fn cursor_capture_position_on_a_rotated_output() {
    let mut f = Fixture::new();
    set_output_state(&mut f, Transform::_90, 1.0);
    let c = f.add_client();
    f.roundtrip(c);
    let source = f.client(c).create_output_capture_source().expect("source");
    let cursor_session = f
        .client(c)
        .create_cursor_session(&source)
        .expect("cursor session");
    let _capture_session = f
        .client(c)
        .cursor_session_get_capture_session(&cursor_session);
    f.roundtrip(c);

    f.absolute_motion((100.0, 50.0).into());
    f.mt.state.flush_clients();
    f.roundtrip(c);

    // the 800x1000 logical area maps to the 1000x800 buffer: (x, y) -> (1000 - y, x)
    let events = f.client_mut(c).take_cursor_session_events();
    assert!(
        events.contains(&CursorSessionEvent::Position { x: 950, y: 100 }),
        "got {events:?}"
    );
}

#[test]
fn client_sees_each_output_once() {
    let mut f = Fixture::new();
    f.add_output("second", (1920, 1080));
    let c = f.add_client();
    f.roundtrip(c);

    assert_eq!(
        summary(f.client(c).outputs()),
        [("test", (1000, 800)), ("second", (1920, 1080))]
    );
}

#[test]
fn output_added_later_is_announced_once() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);
    assert_eq!(summary(f.client(c).outputs()), [("test", (1000, 800))]);

    f.add_output("second", (1920, 1080));
    f.roundtrip(c);
    f.roundtrip(c);

    assert_eq!(
        summary(f.client(c).outputs()),
        [("test", (1000, 800)), ("second", (1920, 1080))]
    );
}
