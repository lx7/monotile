use std::path::PathBuf;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Device, DeviceCapability, Event, InputBackend, InputTime,
        PointerMotionAbsoluteEvent, PointerMotionEvent, UnusedEvent,
    },
    utils::{Logical, Point},
};

pub struct TestInput;

#[derive(PartialEq, Eq, Hash)]
pub struct TestDevice;

impl Device for TestDevice {
    fn id(&self) -> String {
        "test".into()
    }

    fn name(&self) -> String {
        "test".into()
    }

    fn has_capability(&self, capability: DeviceCapability) -> bool {
        capability == DeviceCapability::Pointer
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<PathBuf> {
        None
    }
}

pub struct RelativeMotion {
    pub delta: Point<f64, Logical>,
}

impl Event<TestInput> for RelativeMotion {
    fn time(&self) -> InputTime {
        InputTime::from_millis(0)
    }

    fn device(&self) -> TestDevice {
        TestDevice
    }
}

impl PointerMotionEvent<TestInput> for RelativeMotion {
    fn delta_x(&self) -> f64 {
        self.delta.x
    }

    fn delta_y(&self) -> f64 {
        self.delta.y
    }

    fn delta_x_unaccel(&self) -> f64 {
        self.delta.x
    }

    fn delta_y_unaccel(&self) -> f64 {
        self.delta.y
    }
}

pub struct AbsoluteMotion {
    pub position: Point<f64, Logical>,
}

impl Event<TestInput> for AbsoluteMotion {
    fn time(&self) -> InputTime {
        InputTime::from_millis(0)
    }

    fn device(&self) -> TestDevice {
        TestDevice
    }
}

impl AbsolutePositionEvent<TestInput> for AbsoluteMotion {
    fn x(&self) -> f64 {
        self.position.x
    }

    fn y(&self) -> f64 {
        self.position.y
    }

    fn x_transformed(&self, _width: i32) -> f64 {
        self.position.x
    }

    fn y_transformed(&self, _height: i32) -> f64 {
        self.position.y
    }
}

impl PointerMotionAbsoluteEvent<TestInput> for AbsoluteMotion {}

impl InputBackend for TestInput {
    type Device = TestDevice;
    type KeyboardKeyEvent = UnusedEvent;
    type PointerAxisEvent = UnusedEvent;
    type PointerButtonEvent = UnusedEvent;
    type PointerMotionEvent = RelativeMotion;
    type PointerMotionAbsoluteEvent = AbsoluteMotion;
    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;
    type TouchDownEvent = UnusedEvent;
    type TouchUpEvent = UnusedEvent;
    type TouchMotionEvent = UnusedEvent;
    type TouchCancelEvent = UnusedEvent;
    type TouchFrameEvent = UnusedEvent;
    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;
    type SwitchToggleEvent = UnusedEvent;
    type SpecialEvent = ();
}
