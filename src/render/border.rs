// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::renderer::{
        element::Kind,
        gles::{GlesPixelProgram, Uniform, element::PixelShaderElement},
    },
    utils::{Logical, Rectangle},
};

fn pieces(
    win: Rectangle<i32, Logical>,
    width: i32,
    radius: f32,
) -> ([(i32, i32, i32, i32); 8], f32) {
    let bw = width;
    let outer_r = if radius != 0.0 { radius + bw as f32 } else { 0.0 };
    let c = outer_r.ceil() as i32;

    // window coordinates
    let x = win.loc.x;
    let y = win.loc.y;
    let w = win.size.w;
    let h = win.size.h;

    // outer border origin
    let ox = x - bw;
    let oy = y - bw;
    let bw_total = w + 2 * bw;
    let bh_total = h + 2 * bw;

    #[rustfmt::skip]
    let rects = [
        (ox,              oy,              c,                c),                // top left corner
        (ox + c,          oy,              bw_total - 2 * c, bw),               // top edge
        (x + w + bw - c,  oy,              c,                c),                // top right corner
        (x + w,           oy + c,          bw,               bh_total - 2 * c), // right edge
        (x + w + bw - c,  y + h + bw - c,  c,                c),                // bottom right corner
        (ox + c,          y + h,           bw_total - 2 * c, bw),               // bottom edge
        (ox,              y + h + bw - c,  c,                c),                // bottom left corner
        (ox,              oy + c,          bw,               bh_total - 2 * c), // left edge
    ];

    (rects, outer_r)
}

fn uniforms(
    win: Rectangle<i32, Logical>,
    border_width: i32,
    outer_r: f32,
    color: [f32; 4],
    offset: (f32, f32),
    scale: f32,
) -> Vec<Uniform<'static>> {
    let outer_size = (
        (win.size.w + 2 * border_width) as f32,
        (win.size.h + 2 * border_width) as f32,
    );
    vec![
        Uniform::new("outer_size", outer_size),
        Uniform::new("border_width", border_width as f32),
        Uniform::new("outer_radius", outer_r),
        Uniform::new("border_color", color),
        Uniform::new("piece_offset", offset),
        Uniform::new("scale", scale),
    ]
}

pub fn create_elements(
    shader: &GlesPixelProgram,
    win: Rectangle<i32, Logical>,
    radius: f32,
    border_width: i32,
    color: [f32; 4],
    scale: f32,
) -> Vec<PixelShaderElement> {
    let (rects, outer_r) = pieces(win, border_width, radius);
    let ox = win.loc.x - border_width;
    let oy = win.loc.y - border_width;

    rects
        .into_iter()
        .filter(|(_, _, rw, rh)| *rw > 0 && *rh > 0)
        .map(|(rx, ry, rw, rh)| {
            let offset = ((rx - ox) as f32, (ry - oy) as f32);
            let rect = Rectangle::new((rx, ry).into(), (rw, rh).into());
            PixelShaderElement::new(
                shader.clone(),
                rect,
                None,
                1.0,
                uniforms(win, border_width, outer_r, color, offset, scale),
                Kind::Unspecified,
            )
        })
        .collect()
}
