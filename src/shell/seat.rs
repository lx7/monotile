// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;

use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    desktop::PopupGrab,
    input::Seat,
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::pointer_constraints::{PointerConstraint, with_pointer_constraint},
};

use super::OutputExt;
use crate::Monotile;

struct ActiveOutput(RefCell<Output>);

#[derive(Default)]
struct ActivePopupGrab(RefCell<Option<PopupGrab<Monotile>>>);

pub trait SeatExt {
    fn active_output(&self) -> Output;
    fn set_active_output(&self, output: &Output);
    fn set_popup_grab(&self, grab: PopupGrab<Monotile>);
    fn take_popup_grab(&self) -> Option<PopupGrab<Monotile>>;
    fn pointer_destination(
        &self,
        delta: Point<f64, Logical>,
        under: Option<&(WlSurface, Point<f64, Logical>)>,
    ) -> Point<f64, Logical>;
    fn pointer_confinement(
        &self,
        under: Option<&(WlSurface, Point<f64, Logical>)>,
    ) -> Option<Rectangle<f64, Logical>>;
    fn pointer_locked(&self, under: Option<&(WlSurface, Point<f64, Logical>)>) -> bool;
    fn activate_pointer_constraint(&self, under: Option<&(WlSurface, Point<f64, Logical>)>);
    fn deactivate_pointer_constraint(&self);

    // TODO multi-seat: resolve via the pointer's position instead
    fn pointer_output(&self) -> Output {
        self.active_output()
    }

    fn exclusive_layer(&self) -> Option<WlSurface> {
        self.active_output().exclusive_layer()
    }
}

impl SeatExt for Seat<Monotile> {
    fn active_output(&self) -> Output {
        self.user_data()
            .get::<ActiveOutput>()
            .expect("set when the first monitor was added")
            .0
            .borrow()
            .clone()
    }

    fn set_active_output(&self, output: &Output) {
        let data = self.user_data();
        data.insert_if_missing(|| ActiveOutput(RefCell::new(output.clone())));
        *data.get::<ActiveOutput>().unwrap().0.borrow_mut() = output.clone();
    }

    fn set_popup_grab(&self, grab: PopupGrab<Monotile>) {
        let data = self.user_data();
        data.insert_if_missing(ActivePopupGrab::default);
        data.get::<ActivePopupGrab>().unwrap().0.replace(Some(grab));
    }

    fn take_popup_grab(&self) -> Option<PopupGrab<Monotile>> {
        self.user_data().get::<ActivePopupGrab>()?.0.take()
    }

    fn pointer_destination(
        &self,
        delta: Point<f64, Logical>,
        under: Option<&(WlSurface, Point<f64, Logical>)>,
    ) -> Point<f64, Logical> {
        let pos = self.get_pointer().unwrap().current_location() + delta;
        let pos = clamp_inside(pos, self.pointer_output().geometry().to_f64());
        match self.pointer_confinement(under) {
            Some(rect) => clamp_inside(pos, rect),
            None => pos,
        }
    }

    fn pointer_confinement(
        &self,
        under: Option<&(WlSurface, Point<f64, Logical>)>,
    ) -> Option<Rectangle<f64, Logical>> {
        let pointer = self.get_pointer().unwrap();
        let (surface, surface_loc) = under?;
        let confined = with_pointer_constraint(surface, &pointer, |constraint| {
            let Some(constraint) = constraint else {
                return false;
            };
            match &*constraint {
                PointerConstraint::Confined(_) => constraint.is_active(),
                PointerConstraint::Locked(_) => false,
            }
        });
        if !confined {
            return None;
        }
        let size = with_renderer_surface_state(surface, |state| state.surface_size())??;
        Some(Rectangle::new(*surface_loc, size.to_f64()))
    }

    fn pointer_locked(&self, under: Option<&(WlSurface, Point<f64, Logical>)>) -> bool {
        let pointer = self.get_pointer().unwrap();
        let Some((surface, _)) = under else {
            return false;
        };
        with_pointer_constraint(surface, &pointer, |constraint| {
            let Some(constraint) = constraint else {
                return false;
            };
            match &*constraint {
                PointerConstraint::Locked(_) => constraint.is_active(),
                PointerConstraint::Confined(_) => false,
            }
        })
    }

    fn activate_pointer_constraint(&self, under: Option<&(WlSurface, Point<f64, Logical>)>) {
        let pointer = self.get_pointer().unwrap();
        let Some((surface, surface_loc)) = under else {
            return;
        };
        if pointer.current_focus().as_ref() != Some(surface) {
            return;
        }
        let pos = (pointer.current_location() - *surface_loc).to_i32_floor();
        with_pointer_constraint(surface, &pointer, |constraint| {
            let Some(constraint) = constraint else {
                return;
            };
            if let Some(region) = constraint.region()
                && !region.contains(pos)
            {
                return;
            }
            constraint.activate();
        })
    }

    fn deactivate_pointer_constraint(&self) {
        let pointer = self.get_pointer().unwrap();
        let Some(surface) = pointer.current_focus() else {
            return;
        };
        with_pointer_constraint(&surface, &pointer, |constraint| {
            if let Some(constraint) = constraint {
                constraint.deactivate();
            }
        })
    }
}

fn clamp_inside(pos: Point<f64, Logical>, rect: Rectangle<f64, Logical>) -> Point<f64, Logical> {
    let far = rect.loc + rect.size;
    // exclude edge at max x / y
    Point::new(
        pos.x.max(rect.loc.x).min(far.x.next_down()),
        pos.y.max(rect.loc.y).min(far.y.next_down()),
    )
}
