#![allow(dead_code)]

// TODO: implement runtime config

use smithay::input::keyboard::xkb::keysyms::*;

use crate::input::Mods;

const fn color(hex: u32) -> [f32; 4] {
    [
        ((hex >> 24) & 0xFF) as f32 / 255.0,
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ]
}

/// Appearance
pub const FOCUS_FOLLOWS_CURSOR: bool = true;
pub const BORDER_WIDTH: i32 = 2;
pub const SINGLE_BORDER: bool = false;
pub const GAP: i32 = 0;
pub const SCALE: f64 = 1.0;

pub const BG_COLOR: [f32; 4] = color(0x444444ff);
pub const ROOT_COLOR: [f32; 4] = color(0x000000ff);
pub const BORDER_COLOR: [f32; 4] = color(0x444444ff);
pub const FOCUS_COLOR: [f32; 4] = color(0x458588ff);
pub const URGENT_COLOR: [f32; 4] = color(0xff0000ff);

pub const FLOATING_RADIUS: f32 = 12.0;
pub const TILED_RADIUS: f32 = 0.0;

// Shadow
pub const SHADOW_SOFTNESS: i32 = 25;
pub const SHADOW_SPREAD: i32 = 5;
pub const SHADOW_OFFSET: (i32, i32) = (0, 5);
pub const SHADOW_COLOR: [f32; 4] = color(0x00000073);

/// Tag configuration
pub const TAGCOUNT: usize = 9;

/// Tiling layout parameters
pub const MASTER_FACTOR: f32 = 0.54;
pub const MASTER_COUNT: usize = 1;
pub const RESIZE_STEP: f32 = 0.01;

/// Keyboard configuration
pub const KEYBOARD_LAYOUT: &str = "de";
pub const KEYBOARD_VARIANT: &str = "nodeadkeys";
pub const KEYBOARD_OPTIONS: Option<&str> = None;

/// Keyboard repeat rate and delay
pub const REPEAT_RATE: i32 = 30;
pub const REPEAT_DELAY: i32 = 300;

/// Trackpad configuration
pub const TAP_TO_CLICK: bool = true;
pub const TAP_AND_DRAG: bool = true;
pub const DRAG_LOCK: bool = true;
pub const NATURAL_SCROLL: bool = true;
pub const DISABLE_WHILE_TYPING: bool = true;
pub const LEFT_HANDED: bool = false;
pub const MIDDLE_BUTTON_EMULATION: bool = false;
pub const ACCEL_SPEED: f64 = 0.4;

/// Default terminal
pub const DEFAULT_TERMINAL: &str = "foot";

/// Modifier flags
const SHIFT: u32 = 1;
const CTRL: u32 = 4;
const ALT: u32 = 8;
const LOGO: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyAction {
    Quit,
    Spawn(&'static str, &'static [&'static str]),
    FocusStack(i32),
    MoveStack(i32),
    IncNMaster(i32),
    SetMFact(f32),
    Zoom,
    View(usize),
    Tag(usize),
    ToggleTag(usize),
    KillClient,
    ToggleFullscreen,
    ToggleFloating,
    FocusMon(i32),
    TagMon(i32),
}

pub type Key = (Mods, u32, KeyAction);

macro_rules! spawn {
    ($cmd:expr) => {
        KeyAction::Spawn($cmd, &[])
    };
    ($cmd:expr, $($args:expr),+ $(,)?) => {
        KeyAction::Spawn($cmd, &[$($args),+])
    };
}

macro_rules! key {
    ($mods:expr, $key:expr, $action:expr) => {
        (
            Mods {
                shift: ($mods & SHIFT) != 0,
                ctrl: ($mods & CTRL) != 0,
                alt: ($mods & ALT) != 0,
                logo: ($mods & LOGO) != 0,
            },
            $key,
            $action,
        )
    };
}

fn tagkeys(key: u32, tag: usize) -> [Key; 3] {
    [
        key!(LOGO, key, KeyAction::View(tag)),
        key!(LOGO | SHIFT, key, KeyAction::Tag(tag)),
        key!(LOGO | CTRL | SHIFT, key, KeyAction::ToggleTag(tag)),
    ]
}

pub fn key_bindings() -> Vec<Key> {
    let mut keys = vec![
        // Application launching
        key!(LOGO, KEY_d, spawn!("menu_apps")),
        key!(LOGO | SHIFT, KEY_e, spawn!("menu_exit")),
        key!(LOGO, KEY_x, spawn!("notify_status")),
        key!(
            LOGO | SHIFT,
            KEY_x,
            spawn!("dwlb", "-toggle-visibility", "all")
        ),
        key!(LOGO, KEY_Return, spawn!("foot", "--log-level", "error")),
        key!(LOGO | SHIFT, KEY_Return, spawn!("alacritty")),
        key!(LOGO, KEY_w, spawn!("/bin/sh", "-c", "qbrowser")),
        key!(LOGO | SHIFT, KEY_w, spawn!("firefox")),
        key!(LOGO, KEY_f, spawn!("nautilus", "-w")),
        key!(LOGO | SHIFT, KEY_d, spawn!("thunar")),
        key!(LOGO, KEY_m, spawn!("foot", "--log-level", "error", "aerc")),
        key!(LOGO, KEY_v, spawn!("menu_startvm")),
        key!(LOGO | SHIFT, KEY_v, spawn!("menu_stopvm")),
        key!(LOGO, KEY_l, spawn!("systemctl", "suspend")),
        // Media keys (XF86 keysyms)
        key!(0, KEY_XF86AudioMute, spawn!("change_volume", "-t")),
        key!(LOGO, KEY_XF86AudioMute, spawn!("menu_pipewire")),
        key!(
            0,
            KEY_XF86AudioLowerVolume,
            spawn!("change_volume", "-d", "5")
        ),
        key!(
            0,
            KEY_XF86AudioRaiseVolume,
            spawn!("change_volume", "-i", "5")
        ),
        key!(0, KEY_XF86AudioMicMute, spawn!("change_mic_mute", "toggle")),
        key!(0, KEY_XF86MonBrightnessDown, spawn!("light", "-T", "0.8")),
        key!(0, KEY_XF86MonBrightnessUp, spawn!("light", "-T", "1.2")),
        key!(0, KEY_XF86WLAN, spawn!("toggle_wifi")),
        key!(0, KEY_XF86Tools, spawn!("menu_bluetooth")),
        key!(0, KEY_XF86Bluetooth, spawn!("toggle_bluetooth")),
        key!(0, KEY_Print, spawn!("screenshot")),
        key!(LOGO, KEY_Print, spawn!("screenshot", "file")),
        // Window management
        key!(LOGO, KEY_Left, KeyAction::FocusStack(-1)),
        key!(LOGO, KEY_Right, KeyAction::FocusStack(1)),
        key!(LOGO | SHIFT, KEY_Left, KeyAction::MoveStack(-1)),
        key!(LOGO | SHIFT, KEY_Right, KeyAction::MoveStack(1)),
        key!(LOGO, KEY_plus, KeyAction::IncNMaster(1)),
        key!(LOGO, KEY_minus, KeyAction::IncNMaster(-1)),
        key!(LOGO | ALT, KEY_Left, KeyAction::SetMFact(-0.01)),
        key!(LOGO | ALT, KEY_Right, KeyAction::SetMFact(0.01)),
        key!(LOGO | SHIFT, KEY_z, KeyAction::Zoom),
        key!(LOGO, KEY_Tab, KeyAction::View(usize::MAX)), // Toggle to previous tag
        key!(LOGO | SHIFT, KEY_q, KeyAction::KillClient),
        key!(LOGO, KEY_space, KeyAction::ToggleFullscreen),
        key!(LOGO | SHIFT, KEY_space, KeyAction::ToggleFloating),
        key!(LOGO, KEY_comma, KeyAction::FocusMon(-1)),
        key!(LOGO, KEY_period, KeyAction::FocusMon(1)),
        key!(LOGO | SHIFT, KEY_less, KeyAction::TagMon(-1)),
        key!(LOGO | SHIFT, KEY_greater, KeyAction::TagMon(1)),
        // Quit compositor
        key!(CTRL | ALT, KEY_Terminate_Server, KeyAction::Quit),
    ];

    keys.extend(tagkeys(KEY_1, 0));
    keys.extend(tagkeys(KEY_2, 1));
    keys.extend(tagkeys(KEY_3, 2));
    keys.extend(tagkeys(KEY_4, 3));
    keys.extend(tagkeys(KEY_5, 4));
    keys.extend(tagkeys(KEY_6, 5));
    keys.extend(tagkeys(KEY_7, 6));
    keys.extend(tagkeys(KEY_8, 7));
    keys.extend(tagkeys(KEY_9, 8));

    keys
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Resize,
    ToggleFloating,
}

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

pub type MouseButton = (Mods, u32, MouseAction);

pub const MOUSE_BINDINGS: &[MouseButton] = &[
    key!(LOGO, BTN_LEFT, MouseAction::Move),
    key!(LOGO, BTN_MIDDLE, MouseAction::ToggleFloating),
    key!(LOGO, BTN_RIGHT, MouseAction::Resize),
];
