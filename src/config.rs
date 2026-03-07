// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inline_default::inline_default;
use serde::{Deserialize, Deserializer};
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};
use tracing::{info, warn};

const DEFAULT_CONFIG: &str = include_str!("../defaults/config.ron");
const DEFAULT_AUTOSTART: &str = include_str!("../defaults/autostart.sh");

// --- Color ---

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Color(pub [f32; 4]);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let hex_str = s.strip_prefix('#').unwrap_or(&s);
        let hex = u32::from_str_radix(hex_str, 16)
            .map_err(|_| serde::de::Error::custom(format!("invalid color: {s}")))?;
        match hex_str.len() {
            6 => Ok(color((hex << 8) | 0xFF)),
            8 => Ok(color(hex)),
            _ => Err(serde::de::Error::custom(format!("invalid color: {s}"))),
        }
    }
}

fn color(hex: u32) -> Color {
    Color([
        ((hex >> 24) & 0xFF) as f32 / 255.0,
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ])
}

// --- Config structs ---

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub colors: Colors,
    pub border: Border,
    pub shadow: Shadow,
    pub layout: Layout,
    pub keyboard: Keyboard,
    pub touchpad: Touchpad,
    pub mouse: Mouse,
    #[serde(deserialize_with = "de_keys")]
    pub keybinds: Vec<(Vec<Mod>, Keysym, KeyAction)>,
    pub mousebinds: Vec<(Vec<Mod>, Button, MouseAction)>,
    #[serde(skip)]
    pub key_map: HashMap<(Keysym, Mods), KeyAction>,
    #[serde(skip)]
    pub mouse_map: HashMap<(u32, Mods), MouseAction>,
}

inline_default! {
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct General {
        pub focus_follows_cursor: bool = true,
        pub scale: f32 = 1.0,
        pub gap: i32,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Colors {
        pub bg: Color = color(0x444444FF),
        pub root: Color = color(0x000000FF),
        pub border: Color = color(0x444444FF),
        pub focus: Color = color(0x458588FF),
        pub urgent: Color = color(0xFF0000FF),
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Border {
        pub width: i32 = 2,
        pub single: bool,
        pub floating_radius: f32 = 6.0,
        pub tiled_radius: f32,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Shadow {
        pub softness: i32 = 25,
        pub spread: i32 = 5,
        pub offset: (i32, i32) = (0, 5),
        pub color: Color = color(0x00000073),
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Layout {
        pub tags: usize = 9,
        pub master_factor: f32 = 0.54,
        pub master_count: usize = 1,
        pub resize_step: f32 = 0.01,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Keyboard {
        pub layout: String = "us".into(),
        pub variant: String,
        pub options: String,
        pub repeat_rate: i32 = 30,
        pub repeat_delay: i32 = 300,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Touchpad {
        pub tap: bool = true,
        pub tap_and_drag: bool = true,
        pub drag_lock: bool = true,
        pub natural_scroll: bool = true,
        pub dwt: bool = true,
        pub left_handed: bool,
        pub middle_emulation: bool,
        pub accel_speed: f64 = 0.4,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Mouse {
        pub natural_scroll: bool,
        pub left_handed: bool,
        pub middle_emulation: bool,
        pub accel_speed: f64,
    }
}

// --- Bindings ---

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum Mod {
    Shift,
    Ctrl,
    Alt,
    Logo,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

impl From<&ModifiersState> for Mods {
    fn from(m: &ModifiersState) -> Self {
        Self {
            shift: m.shift,
            ctrl: m.ctrl,
            alt: m.alt,
            logo: m.logo,
        }
    }
}

impl From<&[Mod]> for Mods {
    fn from(v: &[Mod]) -> Self {
        Self {
            shift: v.contains(&Mod::Shift),
            ctrl: v.contains(&Mod::Ctrl),
            alt: v.contains(&Mod::Alt),
            logo: v.contains(&Mod::Logo),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[repr(u32)]
pub enum Button {
    Left = 0x110,
    Right = 0x111,
    Middle = 0x112,
}

#[derive(Debug, Clone, Deserialize)]
pub enum KeyAction {
    Noop,

    FocusMon(i32),
    FocusStack(i32),
    MoveMon(i32),
    MoveStack(i32),

    FocusTag(usize),
    FocusTagPrev,
    Tag(usize),
    ToggleTag(usize),

    IncNMaster(i32),
    SetMFact(f32),
    ToggleFloating,
    ToggleFullscreen,
    Zoom,

    KillClient,
    Quit,
    Spawn(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub enum MouseAction {
    Move,
    Resize,
    ToggleFloating,
}

// --- Keysym serde ---

fn de_keys<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<(Vec<Mod>, Keysym, KeyAction)>, D::Error> {
    let raw: Vec<(Vec<Mod>, String, KeyAction)> = Vec::deserialize(d)?;
    raw.into_iter()
        .map(|(mods, name, action)| {
            let sym = resolve_keysym(&name);
            if sym.raw() == 0 {
                return Err(serde::de::Error::custom(format!("unknown key: {name}")));
            }
            Ok((mods, sym, action))
        })
        .collect()
}

fn resolve_keysym(name: &str) -> Keysym {
    let sym = xkb::keysym_from_name(name, xkb::KEYSYM_NO_FLAGS);
    if sym.raw() != 0 {
        return sym;
    }
    xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE)
}

// --- Loading ---

impl Config {
    pub fn new() -> Self {
        let mut config: Self = ron::from_str(DEFAULT_CONFIG).expect("default config");
        config.build_maps();
        config
    }

    pub fn build_maps(&mut self) {
        for (mods, sym, action) in &self.keybinds {
            self.key_map
                .insert((*sym, Mods::from(mods.as_slice())), action.clone());
        }
        for (mods, btn, action) in &self.mousebinds {
            self.mouse_map
                .insert((*btn as u32, Mods::from(mods.as_slice())), action.clone());
        }
    }
}

pub fn load(explicit: Option<PathBuf>) -> Config {
    let path = resolve(explicit, "config.ron", DEFAULT_CONFIG);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("failed to read {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    let mut config: Config = match ron::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error in {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    config.build_maps();
    info!(
        "config: {} ({} keybinds, {} mousebinds)",
        path.display(),
        config.key_map.len(),
        config.mouse_map.len(),
    );
    config
}

// --- CLI ---

pub struct Args {
    pub config: Option<PathBuf>,
    pub autostart: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Self {
        let mut config = None;
        let mut autostart = None;
        let mut iter = std::env::args().skip(1);
        while let Some(flag) = iter.next() {
            match flag.as_str() {
                "-c" | "--config" => {
                    config = Some(PathBuf::from(iter.next().unwrap_or_else(|| {
                        eprintln!("-c requires a path");
                        std::process::exit(1);
                    })));
                }
                "-s" | "--autostart" => {
                    autostart = Some(PathBuf::from(iter.next().unwrap_or_else(|| {
                        eprintln!("-s requires a path");
                        std::process::exit(1);
                    })));
                }
                _ => {
                    eprintln!("usage: monotile [-c <config>] [-s <autostart>]");
                    std::process::exit(1);
                }
            }
        }
        Self { config, autostart }
    }
}

// --- Path and content helpers ---

pub fn xdg_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("monotile")
}

pub(crate) fn resolve_autostart(path: Option<PathBuf>) -> PathBuf {
    resolve(path, "autostart.sh", DEFAULT_AUTOSTART)
}

fn resolve(path: Option<PathBuf>, name: &str, default: &str) -> PathBuf {
    if let Some(p) = path {
        return p;
    }
    let p = xdg_dir().join(name);
    provision(&p, default);
    p
}

fn provision(path: &Path, content: &str) {
    if path.exists() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, content) {
        warn!("failed to write {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn defaults_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defaults/config.ron")
    }

    #[test]
    fn default_config_matches_inline_defaults() {
        let file: Config = ron::from_str(DEFAULT_CONFIG).expect("default config");
        let code = Config::default();

        assert_eq!(file.general, code.general);
        assert_eq!(file.colors, code.colors);
        assert_eq!(file.border, code.border);
        assert_eq!(file.shadow, code.shadow);
        assert_eq!(file.layout, code.layout);
        assert_eq!(file.keyboard, code.keyboard);
        assert_eq!(file.touchpad, code.touchpad);
        assert_eq!(file.mouse, code.mouse);

        assert!(!file.keybinds.is_empty(), "keybinds empty");
        assert!(!file.mousebinds.is_empty(), "mousebinds empty");
    }

    #[test]
    fn load_defaults_file() {
        let config = load(Some(defaults_path()));
        assert!(!config.keybinds.is_empty());
        assert!(!config.key_map.is_empty(), "should populate key_map");
        assert!(!config.mouse_map.is_empty(), "should populate mouse_map");
    }

    #[test]
    fn color_hex6() {
        let c: Color = ron::from_str("\"#ff8800\"").unwrap();
        assert_eq!(c, color(0xFF8800FF));
    }

    #[test]
    fn color_hex8() {
        let c: Color = ron::from_str("\"#ff880080\"").unwrap();
        assert_eq!(c, color(0xFF880080));
    }

    #[test]
    fn color_invalid_hex() {
        let r = ron::from_str::<Color>("\"#zzzzzz\"");
        assert!(r.is_err());
    }

    #[test]
    fn color_wrong_length() {
        let r = ron::from_str::<Color>("\"#fff\"");
        assert!(r.is_err());
    }

    #[test]
    fn keysym_case_insensitive() {
        let sym = resolve_keysym("return");
        assert_ne!(sym.raw(), 0);
        assert_eq!(sym, resolve_keysym("Return"));
    }

    #[test]
    fn keysym_unknown() {
        let ron = r#"(keybinds: [([Logo], "NonExistentKey_XYZ", Quit)])"#;
        let r = ron::from_str::<Config>(ron);
        assert!(r.is_err());
    }

    #[test]
    fn partial_config_uses_defaults() {
        let ron = "(general: (gap: 10))";
        let config: Config = ron::from_str(ron).unwrap();
        assert_eq!(config.general.gap, 10);
        assert_eq!(config.general.scale, General::default().scale);
        assert_eq!(config.border, Border::default());
    }
}
