// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    Monotile,
    config::{Config, KeyAction, Mods, MouseAction},
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
    spawn::spawn,
};
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, DeviceCapability, Event,
        InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, Focus, GrabStartData, MotionEvent},
    },
    reexports::input::Device,
    utils::{Logical, Point, SERIAL_COUNTER},
};

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
                if self.state.mon().exclusive_layer.is_some() {
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
                        if key_state != KeyState::Pressed {
                            return FilterResult::Forward;
                        }

                        let mods = Mods::from(modifiers);
                        for sym in handle.raw_syms() {
                            if let Some(action) = monotile.state.config.key_map.get(&(sym, mods)) {
                                return FilterResult::Intercept(Some(action.clone()));
                            }
                        }

                        FilterResult::Forward
                    },
                );

                if let Some(Some(action)) = action {
                    self.handle_action(action);
                }
            }
            InputEvent::PointerMotion { event, .. } => {
                let geo = self.state.mon().output_geometry();
                let pos = pointer.current_location() + event.delta();
                let pos = pos.constrain(geo.to_f64());
                self.handle_pointer_motion(pos, event.time_msec(), serial);
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let geo = self.state.mon().output_geometry();
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                self.handle_pointer_motion(pos, event.time_msec(), serial);
            }
            InputEvent::PointerButton { event, .. } => {
                let button = event.button_code();
                let button_state = event.state();

                if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
                    let mods = Mods::from(&keyboard.modifier_state());
                    if let Some(action) = self.state.config.mouse_map.get(&(button, mods)) {
                        self.handle_mouse_action(
                            action.clone(),
                            button,
                            pointer.current_location(),
                            serial,
                        );
                        return;
                    }

                    // raise window and focus
                    let tag = self.state.mon().tag();
                    if let Some(we) = self
                        .state
                        .windows
                        .window_under(tag, pointer.current_location())
                    {
                        let id = we.id;
                        self.state.mon_mut().tag_mut().raise(id);
                        self.set_focus(Some(id));
                    }
                }

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
                let source = event.source();

                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });

                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

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

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }

    fn handle_pointer_motion(
        &mut self,
        pos: Point<f64, Logical>,
        time: u32,
        serial: smithay::utils::Serial,
    ) {
        let pointer = self.state.seat.get_pointer().unwrap();

        if self.state.config.input.focus_follows_cursor {
            let tag = self.state.mon().tag();
            if let Some(we) = self.state.windows.window_under(tag, pos) {
                if Some(we.id) != tag.focused_id() {
                    self.set_focus(Some(we.id));
                }
            }
        }

        let target = self.state.surface_under(pos);
        pointer.motion(
            self,
            target,
            &MotionEvent {
                location: pos,
                serial,
                time,
            },
        );
        pointer.frame(self);
        self.backend.schedule_render(&self.state.mon().output);
    }

    pub fn handle_action(&mut self, action: KeyAction) {
        use KeyAction::*;

        match action {
            Noop => {}
            Quit => {
                self.state.loop_signal.stop();
            }
            Spawn(ref args) => {
                if let Some((cmd, args)) = args.split_first() {
                    spawn(cmd, args, false);
                }
            }
            FocusStack(delta) => {
                if let Some(id) = self.state.mon().tag().focus_cycle(delta) {
                    self.set_focus(Some(id));
                }
            }
            FocusTagPrev => {
                self.state.mon_mut().toggle_prev_tag();
            }
            FocusTag(tag) => {
                self.state.mon_mut().set_active_tag(tag);
            }
            SetTag(tag) => {
                let mon = &mut self.state.monitors[self.state.active_monitor];
                mon.move_to_tag(&mut self.state.windows, tag);
            }
            ToggleTag(tag) => {
                self.state.mon_mut().toggle_tag(tag);
            }
            Close => {
                if let Some(id) = self.state.mon().tag().focused_id()
                    && let Some(tl) = self.state.windows[id].window.toplevel()
                {
                    tl.send_close();
                }
            }
            ToggleFloating => {
                if let Some(id) = self.state.mon().tag().focused_id() {
                    let floating = !self.state.windows[id].floating;
                    self.state.windows[id].set_floating(floating);
                }
            }
            MoveStack(delta) => {
                self.state.mon_mut().tag_mut().move_in_stack(delta);
            }
            SwapMaster => {
                self.state.mon_mut().tag_mut().zoom();
            }
            MasterCount(delta) => {
                self.state.mon_mut().tag_mut().adjust_nmaster(delta);
            }
            MasterRatio(delta) => {
                self.state.mon_mut().tag_mut().adjust_mfact(delta);
            }
            ToggleFullscreen => {
                if let Some(id) = self.state.mon().tag().focused_id() {
                    let geo = if self.state.windows[id].fullscreen {
                        None
                    } else {
                        Some(self.state.mon().output_geometry())
                    };
                    self.state.windows[id].set_fullscreen(geo);
                }
            }
            ReloadConfig => {
                self.reload_config();
            }
            // TODO: implement multi-monitor
            FocusMon(_) | MoveMon(_) => {}
        }
        self.recompute_layout();
        self.backend.schedule_render(&self.state.mon().output);
    }

    fn handle_mouse_action(
        &mut self,
        action: MouseAction,
        btn: u32,
        pos: Point<f64, Logical>,
        serial: smithay::utils::Serial,
    ) {
        let tag = self.state.mon().tag();
        let we = match self.state.windows.window_under(tag, pos) {
            Some(we) if we.floating => we,
            _ => return,
        };
        let id = we.id;
        let geo = we.geo();
        let start = GrabStartData {
            focus: self.state.surface_under(pos),
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

pub fn configure_device(dev: &mut Device, config: &Config) {
    let is_touchpad = dev.config_tap_finger_count() > 0;
    let is_mouse = !is_touchpad && dev.has_capability(DeviceCapability::Pointer.into());

    if is_touchpad {
        let tp = &config.input.touchpad;
        let _ = dev.config_accel_set_profile(tp.accel_profile.into());
        let _ = dev.config_accel_set_speed(tp.accel_speed);
        let _ = dev.config_tap_set_enabled(tp.tap);
        let _ = dev.config_tap_set_drag_enabled(tp.tap_and_drag);
        let _ = dev.config_tap_set_drag_lock_enabled(tp.drag_lock);
        let _ = dev.config_scroll_set_natural_scroll_enabled(tp.natural_scroll);
        let _ = dev.config_dwt_set_enabled(tp.disable_while_typing);
        let _ = dev.config_left_handed_set(tp.left_handed);
        let _ = dev.config_middle_emulation_set_enabled(tp.middle_emulation);
    } else if is_mouse {
        let m = &config.input.mouse;
        let _ = dev.config_accel_set_profile(m.accel_profile.into());
        let _ = dev.config_accel_set_speed(m.accel_speed);
        let _ = dev.config_scroll_set_natural_scroll_enabled(m.natural_scroll);
        let _ = dev.config_left_handed_set(m.left_handed);
        let _ = dev.config_middle_emulation_set_enabled(m.middle_emulation);
    }
}
