use smithay::utils::Rectangle;

use super::Fixture;
use crate::{
    config::{Action, Config},
    shell::{MonitorsExt, SeatExt, View},
};

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

fn settle(f: &mut Fixture, c: usize, win: usize) {
    f.client_mut(c).ack_and_commit(win);
    f.roundtrip(c);
    f.mt.advance_view_queues();
}

fn newest_view(f: &Fixture) -> &View {
    f.mt.state.seat_mon().views.back().unwrap()
}

fn recompute_seat(f: &mut Fixture) {
    let output = f.mt.state.seat.active_output();
    f.mt.recompute_layout(&output);
}

#[test]
fn single_tiled_window_projects_one_tile() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);

    let v = newest_view(&f);
    assert_eq!(v.tiled.len(), 1, "one tiled window");
    assert!(v.floating.is_empty(), "nothing floating");
    assert!(v.fullscreen.is_none(), "not fullscreen");
}

#[test]
fn two_tiled_windows_project_in_layout_order() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    let _b = open_window(&mut f, c);
    settle(&mut f, c, a);

    let order = f.mt.state.seat_mon().tag().tiled.clone();
    let v = newest_view(&f);
    assert_eq!(v.tiled.len(), 2);
    assert_eq!(
        v.tiled.iter().map(|t| t.id).collect::<Vec<_>>(),
        order,
        "view tiled order mirrors the layout",
    );
}

#[test]
fn floating_window_lands_in_floating_group() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    let id = f.mt.state.seat_mon().tag().focused_id().unwrap();
    f.mt.state.windows[id].set_floating(true);
    recompute_seat(&mut f);
    settle(&mut f, c, a);

    let v = newest_view(&f);
    assert!(v.tiled.is_empty(), "floating window not tiled");
    assert_eq!(v.floating, vec![id]);
}

#[test]
fn fullscreen_window_projects_to_fullscreen() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    let id = f.mt.state.seat_mon().tag().focused_id().unwrap();
    f.mt.state.windows[id].set_fullscreen(true);
    recompute_seat(&mut f);
    settle(&mut f, c, a);

    let v = newest_view(&f);
    assert!(v.tiled.is_empty() && v.floating.is_empty());
    assert_eq!(v.fullscreen.map(|p| p.id), Some(id));
}

#[test]
fn smart_gaps_single_window_fills_area() {
    let mut config = Config::new();
    config.layout.smart_gaps = true;
    config.layout.outer_gap = 20;
    let mut f = Fixture::with_config(config);
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);

    let tile = f.mt.state.seat_mon().views.front().tiled[0].rect;
    assert_eq!(
        tile,
        Rectangle::from_size((1000, 800).into()),
        "a lone tiled window fills the whole area with smart_gaps",
    );
}

#[test]
fn lone_tiled_is_derivable_from_tiled_len() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);
    assert_eq!(newest_view(&f).tiled.len(), 1);

    let _b = open_window(&mut f, c);
    settle(&mut f, c, a);
    assert_eq!(newest_view(&f).tiled.len(), 2);
}

#[test]
fn layout_change_configures_windows_to_their_view_rects() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    let b = open_window(&mut f, c);
    f.roundtrip(c);

    let mut configured: Vec<(i32, i32)> = [a, b]
        .into_iter()
        .map(|w| {
            let last = f.client(c).window(w).configures.last().expect("configure");
            (last.width, last.height)
        })
        .collect();
    let mut shown: Vec<(i32, i32)> = newest_view(&f)
        .tiled
        .iter()
        .map(|p| (p.rect.size.w, p.rect.size.h))
        .collect();
    configured.sort();
    shown.sort();
    assert_eq!(
        configured, shown,
        "each window is configured to the size of its rect in the view",
    );
}

#[test]
fn window_rect_follows_the_newest_view() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);
    let id = f.mt.state.seat_mon().tag().focused_id().unwrap();

    // a second window shrinks the first; the first has not committed its new size
    let _b = open_window(&mut f, c);
    let mon = f.mt.state.seat_mon();
    let rect_in = |view: &View| view.tiled.iter().find(|p| p.id == id).map(|p| p.rect);
    let shown = rect_in(mon.views.front());
    let newest = rect_in(newest_view(&f));
    assert_ne!(shown, newest, "the transaction is still pending");
    assert_eq!(
        f.mt.state.monitors.window_rect(&f.mt.state.windows, id),
        newest,
        "window_rect answers from the newest layout",
    );
}

#[test]
fn window_hidden_by_fullscreen_is_configured_when_shown_again() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let main = open_window(&mut f, c);
    let stacked = open_window(&mut f, c);
    let newest = open_window(&mut f, c);
    settle(&mut f, c, main);
    settle(&mut f, c, stacked);

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    settle(&mut f, c, newest);
    f.client_mut(c).take_configures(stacked);

    let _hidden = open_window(&mut f, c);
    f.roundtrip(c);
    assert!(
        f.client_mut(c).take_configures(stacked).is_empty(),
        "a window hidden by a fullscreen window is not configured",
    );

    f.mt.handle_action(Action::ToggleFullscreen);
    f.roundtrip(c);
    assert!(
        !f.client_mut(c).take_configures(stacked).is_empty(),
        "it is configured to its new size when it is shown again",
    );
}
