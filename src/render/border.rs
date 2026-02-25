// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::renderer::{
        element::Kind,
        gles::{GlesPixelProgram, Uniform, element::PixelShaderElement},
    },
    utils::{Logical, Rectangle},
};

pub fn elements(
    shader: &GlesPixelProgram,
    win: Rectangle<i32, Logical>,
    radius: f32,
    border_width: i32,
    color: [f32; 4],
    scale: f32,
) -> Vec<PixelShaderElement> {
    let bw = border_width;
    let outer_r = radius + bw as f32;
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
    let outer_size = (bw_total as f32, bh_total as f32);

    #[rustfmt::skip]
    // (x, y, width, height)
    let rects: [(i32, i32, i32, i32); 8] = [
        (ox,              oy,              c,                c),                // top left corner
        (ox + c,          oy,              bw_total - 2 * c, bw),               // top edge
        (x + w + bw - c,  oy,              c,                c),                // top right corner
        (x + w,           oy + c,          bw,               bh_total - 2 * c), // right edge
        (x + w + bw - c,  y + h + bw - c,  c,                c),                // bottom right corner
        (ox + c,          y + h,           bw_total - 2 * c, bw),               // bottom edge
        (ox,              y + h + bw - c,  c,                c),                // bottom left corner
        (ox,              oy + c,          bw,               bh_total - 2 * c), // left edge
    ];

    let mut elems = Vec::with_capacity(8);

    for (rx, ry, rw, rh) in rects {
        if rw <= 0 || rh <= 0 {
            continue;
        }

        let offset = ((rx - ox) as f32, (ry - oy) as f32);
        let rect = Rectangle::<i32, Logical>::new((rx, ry).into(), (rw, rh).into());

        elems.push(PixelShaderElement::new(
            shader.clone(),
            rect,
            None,
            1.0,
            vec![
                Uniform::new("outer_size", outer_size),
                Uniform::new("border_width", border_width as f32),
                Uniform::new("outer_radius", outer_r),
                Uniform::new("border_color", color),
                Uniform::new("piece_offset", offset),
                Uniform::new("scale", scale),
            ],
            Kind::Unspecified,
        ));
    }

    elems
}
