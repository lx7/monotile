use crate::config::{BORDER_WIDTH, GAP};
use crate::shell::TilingLayout;
use smithay::utils::{Logical, Rectangle};

const W: i32 = 1000;
const H: i32 = 800;

/// edge margin
const EDGE: i32 = GAP + BORDER_WIDTH;
/// inner gap
const INNER: i32 = GAP + 2 * BORDER_WIDTH;

fn area() -> Rectangle<i32, Logical> {
    Rectangle::from_size((W, H).into())
}

fn layout(mcount: usize, mfact: f32) -> TilingLayout {
    TilingLayout {
        master_count: mcount,
        master_factor: mfact,
    }
}

#[test]
fn zero_windows() {
    let rects = TilingLayout::default().compute_rects(0, area());
    assert!(rects.is_empty(), "zero windows should produce no rects");
}

#[test]
fn single_window_no_border() {
    // SINGLE_BORDER=false
    let rects = TilingLayout::default().compute_rects(1, area());
    assert_eq!(rects.len(), 1, "single window should produce 1 rect");
    assert_eq!(rects[0], area(), "single window should fill area");
}

#[test]
fn two_windows_even_split() {
    let mfact = 0.5_f32;
    let half = INNER / 2;
    let usable_w = W - 2 * EDGE;
    let usable_h = H - 2 * EDGE;
    let mw = (usable_w as f32 * mfact) as i32;

    let rects = layout(1, mfact).compute_rects(2, area());
    assert_eq!(rects.len(), 2, "two windows should produce 2 rects");

    assert_eq!(rects[0].loc, (EDGE, EDGE).into(), "master loc");
    assert_eq!(rects[0].size, (mw - half, usable_h).into(), "master size");

    let stack_x = EDGE + mw + INNER - half;
    let stack_w = usable_w - mw - INNER + half;
    assert_eq!(rects[1].loc, (stack_x, EDGE).into(), "stack loc");
    assert_eq!(rects[1].size, (stack_w, usable_h).into(), "stack size");
}

#[test]
fn three_windows_stack_splits_vertically() {
    let mfact = 0.5_f32;
    let half = INNER / 2;
    let usable_w = W - 2 * EDGE;
    let usable_h = H - 2 * EDGE;
    let mw = (usable_w as f32 * mfact) as i32;
    let stack_x = EDGE + mw + INNER - half;
    let stack_w = usable_w - mw - INNER + half;
    let stack_h = (usable_h - INNER) / 2;

    let rects = layout(1, mfact).compute_rects(3, area());
    assert_eq!(rects.len(), 3, "three windows should produce 3 rects");

    // master
    assert_eq!(rects[0].loc, (EDGE, EDGE).into(), "master loc");
    assert_eq!(rects[0].size, (mw - half, usable_h).into(), "master size");

    // stack windows split vertically
    assert_eq!(rects[1].loc, (stack_x, EDGE).into(), "stack[0] loc");
    assert_eq!(rects[1].size, (stack_w, stack_h).into(), "stack[0] size");

    let y2 = EDGE + stack_h + INNER;
    assert_eq!(rects[2].loc, (stack_x, y2).into(), "stack[1] loc");
    assert_eq!(rects[2].size, (stack_w, stack_h).into(), "stack[1] size");
}

#[test]
fn master_count_two() {
    let rects = layout(2, 0.5).compute_rects(3, area());
    assert_eq!(rects.len(), 3, "should produce 3 rects");

    // both masters in left column, split vertically
    assert_eq!(rects[0].loc.x, rects[1].loc.x, "masters should share x");
    assert!(
        rects[0].loc.y < rects[1].loc.y,
        "master[0] should be above master[1]"
    );

    // stack in right column
    assert!(
        rects[2].loc.x > rects[0].loc.x,
        "stack should be right of master"
    );
}

#[test]
fn master_count_exceeds_window_count() {
    // master_count=3 but only 2 windows: all go to master
    let rects = layout(3, 0.5).compute_rects(2, area());
    assert_eq!(rects.len(), 2, "should produce 2 rects");
    assert_eq!(rects[0].loc.x, rects[1].loc.x, "both should share x");
    assert!(
        rects[0].loc.y < rects[1].loc.y,
        "first should be above second"
    );
}

#[test]
fn mfact_extremes() {
    // mfact=0.1: narrow master
    let rects = layout(1, 0.1).compute_rects(2, area());
    let master_w = rects[0].size.w;
    let stack_w = rects[1].size.w;
    assert!(
        stack_w > master_w * 5,
        "stack should be much wider: m={master_w} s={stack_w}",
    );

    // mfact=0.9: wide master
    let rects = layout(1, 0.9).compute_rects(2, area());
    let master_w = rects[0].size.w;
    let stack_w = rects[1].size.w;
    assert!(
        master_w > stack_w * 5,
        "master should be much wider: m={master_w} s={stack_w}",
    );
}

#[test]
fn windows_cover_area_without_overlap() {
    let rects = layout(1, 0.5).compute_rects(4, area());
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
        let rects = layout(1, 0.5).compute_rects(count, area());
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
