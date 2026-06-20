use super::Fixture;
use crate::shell::View;

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
    f.mt.unblock_ready_transitions();
}

fn project(f: &Fixture) -> View {
    let st = &f.mt.state;
    let mon = &st.monitors[st.active_monitor];
    View::project(mon.tag(), &st.windows, mon.output_geometry())
}

#[test]
fn single_tiled_window_projects_one_tile() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);

    let v = project(&f);
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

    let st = &f.mt.state;
    let mon = &st.monitors[st.active_monitor];
    let order: Vec<_> = mon.tag().layout.ids().collect();
    let v = View::project(mon.tag(), &st.windows, mon.output_geometry());

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
    let id = f.mt.state.mon().tag().focused_id().unwrap();
    f.mt.state.windows[id].set_floating(true);
    f.mt.recompute_layout(f.mt.state.active_monitor);
    settle(&mut f, c, a);

    let v = project(&f);
    assert!(v.tiled.is_empty(), "floating window not tiled");
    assert_eq!(v.floating.len(), 1);
    assert_eq!(v.floating[0].id, id);
    assert_eq!(v.floating[0].rect, f.mt.state.windows[id].float_geo);
}

#[test]
fn fullscreen_window_projects_to_fullscreen_at_output_rect() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    let id = f.mt.state.mon().tag().focused_id().unwrap();
    f.mt.state.windows[id].set_fullscreen(true);
    f.mt.recompute_layout(f.mt.state.active_monitor);
    settle(&mut f, c, a);

    let st = &f.mt.state;
    let mon = &st.monitors[st.active_monitor];
    let v = View::project(mon.tag(), &st.windows, mon.output_geometry());

    assert!(v.tiled.is_empty() && v.floating.is_empty());
    assert_eq!(v.fullscreen.map(|t| t.id), Some(id));
    assert_eq!(v.fullscreen.map(|t| t.rect), Some(mon.output_geometry()));
}

#[test]
fn lone_tiled_is_derivable_from_tiled_len() {
    let mut f = Fixture::new();
    let c = f.add_client();
    let a = open_window(&mut f, c);
    settle(&mut f, c, a);
    assert_eq!(project(&f).tiled.len(), 1);

    let _b = open_window(&mut f, c);
    settle(&mut f, c, a);
    assert_eq!(project(&f).tiled.len(), 2);
}
