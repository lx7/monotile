use smithay::utils::{Logical, Point};

use super::Fixture;
use crate::{config::Action, shell::MonitorsExt};

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
fn tag_switch_refreshes_pointer_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.client_mut(c).bind_pointer();
    f.roundtrip(c);

    let w0 = open_window(&mut f, c);
    f.mt.handle_action(Action::FocusTag(1));
    let w1 = open_window(&mut f, c);

    f.pointer_motion(focused_window_point(&f));
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w1).surface),
        "the pointer starts on the window of the active tag",
    );

    f.mt.handle_action(Action::FocusTag(0));
    f.roundtrip(c);
    assert_eq!(
        f.client(c).pointer_focus(),
        Some(&f.client(c).window(w0).surface),
        "after a tag switch the pointer must focus the window now under it",
    );
}
