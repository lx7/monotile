// SPDX-License-Identifier: GPL-3.0-only
// Clipping technique based on niri's clipped_surface.rs (GPL-3.0)
// https://github.com/YaLTeR/niri

use std::borrow::BorrowMut;

use smithay::{
    backend::renderer::{
        element::{
            Element, Id, Kind, RenderElement, UnderlyingStorage,
            surface::WaylandSurfaceRenderElement,
        },
        gles::{GlesError, GlesFrame, GlesTexProgram, Uniform, UniformValue},
        glow::{GlowFrame, GlowRenderer},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

pub struct ClippedSurface {
    inner: WaylandSurfaceRenderElement<GlowRenderer>,
    program: GlesTexProgram,
    geo: Rectangle<f64, Logical>,
    radius: f32,
    uniforms: Vec<Uniform<'static>>,
}

impl ClippedSurface {
    pub fn new(
        inner: WaylandSurfaceRenderElement<GlowRenderer>,
        program: GlesTexProgram,
        geo: Rectangle<i32, Logical>,
        radius: f32,
        scale: Scale<f64>,
    ) -> Self {
        let geo_f = geo.to_f64();
        let elem = inner.geometry(scale);
        let phys: Rectangle<i32, Physical> = geo_f.to_physical_precise_round(scale);

        let elem_w = elem.size.w as f32;
        let elem_h = elem.size.h as f32;
        let phys_w = phys.size.w.max(1) as f32;
        let phys_h = phys.size.h.max(1) as f32;
        let offset_x = (elem.loc.x - phys.loc.x) as f32 / elem_w.max(1.0);
        let offset_y = (elem.loc.y - phys.loc.y) as f32 / elem_h.max(1.0);

        // Column-major 3x3: maps texture UV to [0,1] geo space
        #[rustfmt::skip]
        let mat: [f32; 9] = [
            elem_w / phys_w, 0.0,             0.0,
            0.0,             elem_h / phys_h, 0.0,
            offset_x,        offset_y,        1.0,
        ];

        let uniforms = vec![
            Uniform::new("geo_size", (geo.size.w as f32, geo.size.h as f32)),
            Uniform::new("inner_radius", radius),
            Uniform::new("scale", scale.x as f32),
            Uniform::new(
                "input_to_geo",
                UniformValue::Matrix3x3 {
                    matrices: vec![mat],
                    transpose: false,
                },
            ),
        ];

        Self {
            inner,
            program,
            geo: geo_f,
            radius,
            uniforms,
        }
    }

    pub fn will_clip(
        inner: &WaylandSurfaceRenderElement<GlowRenderer>,
        geo: Rectangle<i32, Logical>,
        radius: f32,
        scale: Scale<f64>,
    ) -> bool {
        if radius > 0.0 {
            return true;
        }
        let phys: Rectangle<i32, Physical> = geo.to_f64().to_physical_precise_round(scale);
        let elem = inner.geometry(scale);
        !phys.contains_rect(elem)
    }

    fn clip_rect(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        let mut r = self.geo.to_physical_precise_round(scale);
        r.loc -= self.geometry(scale).loc;
        r
    }
}

impl Element for ClippedSurface {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        // clip damage rects to window geometry
        let clip = self.clip_rect(scale);
        self.inner
            .damage_since(scale, commit)
            .into_iter()
            .filter_map(|r| r.intersection(clip))
            .collect()
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        // clip opaque regions
        let clip = self.clip_rect(scale);
        let clipped = self
            .inner
            .opaque_regions(scale)
            .into_iter()
            .filter_map(|r| r.intersection(clip));

        // substract corner areas
        let r = self.radius as f64;
        let g = self.geo;
        let loc = self.geometry(scale).loc;
        let corners = [
            g.loc,
            Point::from((g.loc.x + g.size.w - r, g.loc.y)),
            Point::from((g.loc.x + g.size.w - r, g.loc.y + g.size.h - r)),
            Point::from((g.loc.x, g.loc.y + g.size.h - r)),
        ]
        .into_iter()
        .map(|p| {
            let mut c: Rectangle<i32, Physical> =
                Rectangle::new(p, Size::from((r, r))).to_physical_precise_up(scale);
            c.loc -= loc;
            c
        });

        OpaqueRegions::from_slice(&Rectangle::subtract_rects_many(clipped, corners))
    }
}

impl RenderElement<GlowRenderer> for ClippedSurface {
    fn draw(
        &self,
        frame: &mut GlowFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        // set the custom shader befor drawing
        let gles: &mut GlesFrame = frame.borrow_mut();
        gles.override_default_tex_program(self.program.clone(), self.uniforms.clone());

        // draw ...
        RenderElement::<GlowRenderer>::draw(&self.inner, frame, src, dst, damage, opaque)?;

        // cleanup
        let gles: &mut GlesFrame = frame.borrow_mut();
        gles.clear_tex_program_override();
        Ok(())
    }

    fn underlying_storage(&self, _: &mut GlowRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
