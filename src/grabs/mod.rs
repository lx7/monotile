// SPDX-License-Identifier: GPL-3.0-only

// generate a delegate method for gestures on PointerGrab impl.
macro_rules! forward_gesture {
    ($method:ident, $event_type:ty) => {
        fn $method(
            &mut self,
            data: &mut $crate::Monotile,
            handle: &mut smithay::input::pointer::PointerInnerHandle<'_, $crate::Monotile>,
            event: &$event_type,
        ) {
            handle.$method(data, event)
        }
    };
}

pub(crate) use forward_gesture;

pub mod move_grab;
pub use move_grab::MoveSurfaceGrab;

pub mod resize_grab;
pub use resize_grab::ResizeSurfaceGrab;
