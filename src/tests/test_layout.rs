use crate::config::Layout;
use crate::shell::TilingLayout;
use smithay::utils::{Logical, Rectangle};

const W: i32 = 1000;
const H: i32 = 800;

fn cfg() -> Layout {
    Layout::default()
}

fn area() -> Rectangle<i32, Logical> {
    Rectangle::from_size((W, H).into())
}

fn layout(mcount: usize, mfact: f32) -> TilingLayout {
    TilingLayout {
        master_count: mcount,
        master_factor: mfact,
    }
}

fn compute(layout: &TilingLayout, count: usize) -> Vec<Rectangle<i32, Logical>> {
    layout.compute_rects(count, area(), &cfg())
}

#[test]
fn zero_windows() {
    let rects = compute(&TilingLayout::default(), 0);
    assert!(rects.is_empty(), "zero windows should produce no rects");
}

#[test]
fn single_window_fills_area() {
    let c = cfg();
    let outer = c.outer_gap;
    let rects = compute(&TilingLayout::default(), 1);
    assert_eq!(rects.len(), 1, "single window should produce 1 rect");
    let expected = Rectangle::new(
        (outer, outer).into(),
        (W - 2 * outer, H - 2 * outer).into(),
    );
    assert_eq!(rects[0], expected, "single window should fill usable area");
}

#[test]
fn two_windows_even_split() {
    let c = cfg();
    let outer = c.outer_gap;
    let inner = c.inner_gap;
    let mfact = 0.5_f32;
    let half = inner / 2;
    let usable_w = W - 2 * outer;
    let usable_h = H - 2 * outer;
    let mw = (usable_w as f32 * mfact) as i32;

    let rects = compute(&layout(1, mfact), 2);
    assert_eq!(rects.len(), 2, "two windows should produce 2 rects");

    assert_eq!(rects[0].loc, (outer, outer).into(), "master loc");
    assert_eq!(rects[0].size, (mw - half, usable_h).into(), "master size");

    let stack_x = outer + mw + inner - half;
    let stack_w = usable_w - mw - inner + half;
    assert_eq!(rects[1].loc, (stack_x, outer).into(), "stack loc");
    assert_eq!(rects[1].size, (stack_w, usable_h).into(), "stack size");
}

#[test]
fn three_windows_stack_splits_vertically() {
    let c = cfg();
    let outer = c.outer_gap;
    let inner = c.inner_gap;
    let mfact = 0.5_f32;
    let half = inner / 2;
    let usable_w = W - 2 * outer;
    let usable_h = H - 2 * outer;
    let mw = (usable_w as f32 * mfact) as i32;
    let stack_x = outer + mw + inner - half;
    let stack_w = usable_w - mw - inner + half;
    let stack_h = (usable_h - inner) / 2;

    let rects = compute(&layout(1, mfact), 3);
    assert_eq!(rects.len(), 3, "three windows should produce 3 rects");

    assert_eq!(rects[0].loc, (outer, outer).into(), "master loc");
    assert_eq!(rects[0].size, (mw - half, usable_h).into(), "master size");

    assert_eq!(rects[1].loc, (stack_x, outer).into(), "stack[0] loc");
    assert_eq!(rects[1].size, (stack_w, stack_h).into(), "stack[0] size");

    let y2 = outer + stack_h + inner;
    assert_eq!(rects[2].loc, (stack_x, y2).into(), "stack[1] loc");
    assert_eq!(rects[2].size, (stack_w, stack_h).into(), "stack[1] size");
}

#[test]
fn master_count_two() {
    let rects = compute(&layout(2, 0.5), 3);
    assert_eq!(rects.len(), 3, "should produce 3 rects");

    assert_eq!(rects[0].loc.x, rects[1].loc.x, "masters should share x");
    assert!(
        rects[0].loc.y < rects[1].loc.y,
        "master[0] should be above master[1]"
    );

    assert!(
        rects[2].loc.x > rects[0].loc.x,
        "stack should be right of master"
    );
}

#[test]
fn master_count_exceeds_window_count() {
    let rects = compute(&layout(3, 0.5), 2);
    assert_eq!(rects.len(), 2, "should produce 2 rects");
    assert_eq!(rects[0].loc.x, rects[1].loc.x, "both should share x");
    assert!(
        rects[0].loc.y < rects[1].loc.y,
        "first should be above second"
    );
}

#[test]
fn mfact_extremes() {
    let rects = compute(&layout(1, 0.1), 2);
    let master_w = rects[0].size.w;
    let stack_w = rects[1].size.w;
    assert!(
        stack_w > master_w * 5,
        "stack should be much wider: m={master_w} s={stack_w}",
    );

    let rects = compute(&layout(1, 0.9), 2);
    let master_w = rects[0].size.w;
    let stack_w = rects[1].size.w;
    assert!(
        master_w > stack_w * 5,
        "master should be much wider: m={master_w} s={stack_w}",
    );
}

#[test]
fn windows_cover_area_without_overlap() {
    let rects = compute(&layout(1, 0.5), 4);
    assert_eq!(rects.len(), 4, "should produce 4 rects");

    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            let a = rects[i];
            let b = rects[j];
            let overlap = a.intersection(b);
            assert!(
                overlap.is_none() || overlap.unwrap().is_empty(),
                "rects {i} and {j} overlap: {a:?} ∩ {b:?}",
            );
        }
    }
}

#[test]
fn all_rects_fit_within_area() {
    for count in 1..=6 {
        let rects = compute(&layout(1, 0.5), count);
        for (i, r) in rects.iter().enumerate() {
            assert!(
                r.loc.x >= 0 && r.loc.y >= 0,
                "count={count} rect {i} has negative loc: {r:?}",
            );
            assert!(
                r.loc.x + r.size.w <= W && r.loc.y + r.size.h <= H,
                "count={count} rect {i} exceeds area: {r:?}",
            );
        }
    }
}
