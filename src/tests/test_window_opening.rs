use super::Fixture;
use smithay::utils::Rectangle;
use wayland_protocols::xdg::shell::client::xdg_toplevel::State as ToplevelState;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    assert!(
        f.mt.state.mon().active_id().is_some(),
        "window {w} should be mapped after open_window",
    );
    w
}

#[test]
fn two_windows() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    f.client_mut(c).take_configures(w1); // drain

    let w2 = open_window(&mut f, c);
    assert_eq!(
        f.mt.state.mon().visible_windows().count(),
        2,
        "compositor should have 2 visible windows",
    );

    // w1 should be reconfigured (full -> master)
    let cfgs1 = f.client_mut(c).take_configures(w1);
    // w2 got its initial configure during open_window
    let cfgs2 = f.client_mut(c).take_configures(w2);

    assert!(!cfgs1.is_empty(), "master should be reconfigured",);
    assert!(!cfgs2.is_empty(), "stack window should get a configure",);

    // master and stack should have different widths
    let last1 = cfgs1.last().unwrap();
    let last2 = cfgs2.last().unwrap();
    assert_ne!(
        last1.width, last2.width,
        "master and stack should differ: \
         {}x{} vs {}x{}",
        last1.width, last1.height, last2.width, last2.height,
    );
}

#[test]
fn close_window() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let _w1 = open_window(&mut f, c);
    let w2 = open_window(&mut f, c);

    // close the active window (w2) via the server
    f.mt.state.mon().kill_active();
    f.roundtrip(c);

    let ws = f.client(c).window(w2);
    assert!(ws.closed, "expected close event on second window",);
}

#[test]
fn tag_switch() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).take_configures(w); // drain

    f.mt.state.mon_mut().set_active_tag(1);
    f.roundtrip(c);

    // window is on tag 0, not visible on tag 1
    let cfgs = f.client_mut(c).take_configures(w);
    assert!(
        cfgs.is_empty(),
        "no configures expected after switching \
         away from window's tag, got {}",
        cfgs.len(),
    );
    assert_eq!(
        f.mt.state.mon().visible_windows().count(),
        0,
        "tag 1 should have no visible windows",
    );

    // switch back — window should be visible again
    f.mt.state.mon_mut().set_active_tag(0);
    assert_eq!(
        f.mt.state.mon().visible_windows().count(),
        1,
        "tag 0 should have 1 visible window",
    );
}

/// Check that the last configure for a window has the Activated state.
fn is_activated(f: &mut Fixture, c: usize, w: usize) -> bool {
    let cfgs = f.client_mut(c).take_configures(w);
    assert!(
        !cfgs.is_empty(),
        "expected at least one configure for window {w}"
    );
    cfgs.last()
        .unwrap()
        .states
        .contains(&ToplevelState::Activated)
}

#[test]
fn first_window_activated() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).take_configures(w); // drain initial

    // trigger focus sync
    f.mt.update_focus();
    f.roundtrip(c);

    assert!(
        is_activated(&mut f, c, w),
        "sole window should be activated"
    );
}

#[test]
fn second_window_steals_focus() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    f.client_mut(c).take_configures(w1); // drain

    let w2 = open_window(&mut f, c);
    f.mt.update_focus();
    f.roundtrip(c);

    assert!(
        !is_activated(&mut f, c, w1),
        "first window should not be activated"
    );
    assert!(
        is_activated(&mut f, c, w2),
        "second window should be activated"
    );
}

#[test]
fn focus_cycle() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let w2 = open_window(&mut f, c);
    f.client_mut(c).take_configures(w1);
    f.client_mut(c).take_configures(w2);

    // cycle focus to w1
    if let Some(id) = f.mt.state.mon().tag().focus_cycle(1) {
        f.mt.set_focus(Some(id));
    }
    f.roundtrip(c);

    assert!(
        is_activated(&mut f, c, w1),
        "w1 should be activated after focus cycle"
    );
    assert!(
        !is_activated(&mut f, c, w2),
        "w2 should not be activated after focus cycle"
    );
}

#[test]
fn focus_after_remove() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w1 = open_window(&mut f, c);
    let _w2 = open_window(&mut f, c);
    f.client_mut(c).take_configures(w1);

    // remove the active window and re-sync focus
    let active = f.mt.state.mon().active_id().unwrap();
    f.mt.state.mon_mut().unmap(active);
    f.mt.update_focus();
    f.roundtrip(c);

    assert_eq!(
        f.mt.state.mon().visible_windows().count(),
        1,
        "should have 1 visible window after remove",
    );
    assert!(
        is_activated(&mut f, c, w1),
        "remaining window should be activated"
    );
}

#[test]
fn float_geo_preserved_across_toggle() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let w = open_window(&mut f, c);
    f.client_mut(c).take_configures(w);

    let id = f.mt.state.mon().active_id().unwrap();

    // toggle to floating, should get a centered float_geo
    f.mt.state.mon_mut().toggle_active_floating();
    let geo1 = f.mt.state.mon().get(id).unwrap().float_geo;
    assert!(
        geo1.size.w > 0 && geo1.size.h > 0,
        "float_geo should have nonzero size"
    );

    // modify float_geo to simulate a move
    let moved = Rectangle::new((100, 200).into(), geo1.size);
    f.mt.state.mon_mut().get_mut(id).unwrap().float_geo = moved;

    // toggle to tiled and back to floating
    f.mt.state.mon_mut().toggle_active_floating();
    f.mt.state.mon_mut().toggle_active_floating();

    let geo2 = f.mt.state.mon().get(id).unwrap().float_geo;
    assert_eq!(
        geo2, moved,
        "float_geo should be preserved across tiled round-trip"
    );
}
