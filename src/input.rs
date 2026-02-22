// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    Monotile,
    config::*,
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
};
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, Focus, GrabStartData, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

impl Mods {
    pub fn matches(&self, other: &ModifiersState) -> bool {
        self.shift == other.shift
            && self.ctrl == other.ctrl
            && self.alt == other.alt
            && self.logo == other.logo
    }
}

impl Monotile {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        let pointer = self.state.seat.get_pointer().unwrap();
        let keyboard = self.state.seat.get_keyboard().unwrap();
        let serial = SERIAL_COUNTER.next_serial();

        match event {
            InputEvent::Keyboard { event, .. } => {
                let time = Event::time_msec(&event);
                let key_code = event.key_code();
                let key_state = event.state();

                // Exclusive layer grabs all keys
                // TODO: check all monitors. Maybe create a helper function.
                if self.state.mon().exclusive_layer_surface().is_some() {
                    keyboard.input::<(), _>(self, key_code, key_state, serial, time, |_, _, _| {
                        FilterResult::Forward
                    });
                    return;
                }

                let action = keyboard.input(
                    self,
                    key_code,
                    key_state,
                    serial,
                    time,
                    |monotile, modifiers, handle| {
                        // Only handle key press, not release
                        if key_state != KeyState::Pressed {
                            return FilterResult::Forward;
                        }

                        for (bind_mods, bind_key, action) in &monotile.state.key_bindings {
                            if bind_mods.matches(modifiers)
                                && handle.raw_syms().contains(&Keysym::new(*bind_key))
                            {
                                return FilterResult::Intercept(Some(*action));
                            }
                        }

                        FilterResult::Forward
                    },
                );

                // the outer option is None for forwarded events
                if let Some(Some(action)) = action {
                    self.handle_action(action);
                }
            }
            // TODO: handle PointerMotion when DRM backend is implemented
            InputEvent::PointerMotion { .. } => {}
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output_geo = self.state.mon().output_geometry();
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

                if FOCUS_FOLLOWS_CURSOR && let Some(we) = self.state.mon().window_under(pos) {
                    self.set_focus(Some(we.id));
                }
                let target = self.state.mon().surface_under(pos);

                // forward event to target surface
                pointer.motion(
                    self,
                    target,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                let button = event.button_code();
                let button_state = event.state();

                if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
                    let mods = keyboard.modifier_state();

                    for (bind_mods, bind_btn, action) in MOUSE_BINDINGS {
                        if bind_mods.matches(&mods) && button == *bind_btn {
                            self.handle_mouse_action(
                                *action,
                                button,
                                pointer.current_location(),
                                serial,
                            );
                            // don't leak compositor binds to clients
                            return;
                        }
                    }

                    // raise window and focus
                    if let Some(we) = self.state.mon().window_under(pointer.current_location()) {
                        let id = we.id;
                        self.state.mon_mut().tag_mut().raise(id);
                        self.set_focus(Some(id));
                    }
                }

                // forward event to the focused client
                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                // scroll forwarding from Smithay/Anvil
                let source = event.source();

                // get pixel amount for each axis
                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });

                // get discrete v120 amount (high-res scroll)
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                // create axis frame and add values only it they are not 0.0
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                // trackpad: stop when the finger lifted
                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                // forward
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }

    pub fn handle_action(&mut self, action: KeyAction) {
        use KeyAction::*;

        match action {
            Quit => self.state.loop_signal.stop(),
            Spawn(cmd, args) => {
                std::process::Command::new(cmd).args(args).spawn().ok();
            }
            FocusStack(delta) => {
                if let Some(id) = self.state.mon().tag().focus_cycle(delta) {
                    self.set_focus(Some(id));
                }
            }
            View(usize::MAX) => self.state.mon_mut().toggle_prev_tag(),
            View(tag) => self.state.mon_mut().set_active_tag(tag),
            Tag(tag) => self.state.mon_mut().move_active_to_tag(tag),
            ToggleTag(tag) => self.state.mon_mut().toggle_active_tag(tag),
            KillClient => self.state.mon_mut().kill_active(),
            ToggleFloating => self.state.mon_mut().toggle_active_floating(),
            MoveStack(delta) => self.state.mon_mut().move_in_stack(delta),
            Zoom => self.state.mon_mut().zoom(),
            IncNMaster(delta) => self.state.mon_mut().adjust_nmaster(delta),
            SetMFact(delta) => self.state.mon_mut().adjust_mfact(delta),
            // TODO: implement fullscreen and multi-monitor
            ToggleFullscreen | FocusMon(_) | TagMon(_) => {}
        }
        self.update_focus();
    }

    fn handle_mouse_action(
        &mut self,
        action: MouseAction,
        btn: u32,
        pos: Point<f64, Logical>,
        serial: smithay::utils::Serial,
    ) {
        let we = self.state.mon().window_under(pos);
        let we = match we {
            Some(we) if we.floating => we,
            _ => return,
        };
        let id = we.id;
        let geo = we.geo();
        let start = GrabStartData {
            focus: self.state.mon().surface_under(pos),
            button: btn,
            location: pos,
        };

        let ptr = self.state.seat.get_pointer().unwrap();
        match action {
            MouseAction::Move => {
                let grab = MoveSurfaceGrab {
                    start_data: start,
                    window_id: id,
                    initial_location: geo.loc,
                };
                ptr.set_grab(self, grab, serial, Focus::Clear);
            }
            MouseAction::Resize => {
                let grab = ResizeSurfaceGrab::start(start, id, geo);
                ptr.set_grab(self, grab, serial, Focus::Clear);
            }
            MouseAction::ToggleFloating => {
                // TODO: implement ToggleFloating
            }
        }
    }
}
