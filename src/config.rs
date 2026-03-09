// SPDX-License-Identifier: GPL-3.0-only

// TODO: remove when config handling is complete
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inline_default::inline_default;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};
use smithay::reexports::input::AccelProfile as InputAccelProfile;
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

// --- Rules and pattern matching ---

#[derive(Debug, Clone)]
pub struct Pattern(Regex);

impl Pattern {
    pub fn is_match(&self, s: &str) -> bool {
        self.0.is_match(s)
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let exp = format!("^(?:{s})$");
        Regex::new(&exp)
            .map(Pattern)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OutputMatch {
    pub name: Option<Pattern>,
    pub make: Option<Pattern>,
    pub model: Option<Pattern>,
    pub serial: Option<Pattern>,
}

impl OutputMatch {
    pub fn matches(&self, name: &str, make: &str, model: &str, serial: &str) -> bool {
        self.name.as_ref().is_none_or(|p| p.is_match(name))
            && self.make.as_ref().is_none_or(|p| p.is_match(make))
            && self.model.as_ref().is_none_or(|p| p.is_match(model))
            && self.serial.as_ref().is_none_or(|p| p.is_match(serial))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OutputRule {
    pub r#match: OutputMatch,
    pub scale: Option<f64>,
    pub pos: Option<(i32, i32)>,
    // TODO: add mode and transform when output config is implemented
    pub background: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WindowMatch {
    pub app_id: Option<Pattern>,
    pub title: Option<Pattern>,
    pub floating: Option<bool>,
}

impl WindowMatch {
    pub fn matches(&self, app_id: &str, title: &str, floating: bool) -> bool {
        self.app_id.as_ref().is_none_or(|p| p.is_match(app_id))
            && self.title.as_ref().is_none_or(|p| p.is_match(title))
            && self.floating.is_none_or(|v| v == floating)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WindowInit {
    pub floating: Option<bool>,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
    pub output: Option<String>,
    pub tags: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum RenderStep {
    Shadow {
        softness: i32,
        spread: i32,
        offset: (i32, i32),
        color: Color,
    },
    WindowSurface {
        #[serde(default)]
        radius: f32,
        fill: Color,
    },
    Border {
        width: i32,
        color: Color,
    },
    FocusRing {
        width: i32,
        color: Color,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WindowRule {
    pub r#match: WindowMatch,
    pub init: Option<WindowInit>,
    pub render: Option<Vec<RenderStep>>,
}

// --- Layout ---

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
pub enum LayoutMode {
    #[default]
    Tile,
}

inline_default! {
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct TileConfig {
        pub master_factor: f32 = 0.54,
        pub master_count: usize = 1,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Layout {
        pub tags: usize = 9,
        pub inner_gap: i32 = 4,
        pub outer_gap: i32 = 2,
        pub smart_gaps: bool,
        pub smart_borders: bool,
        pub default: LayoutMode = LayoutMode::Tile,
        pub tile: TileConfig = TileConfig::default(),
    }

    // --- Input ---

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Input {
        pub focus_follows_cursor: bool = true,
        pub hide_cursor_when_typing: bool = true,
        pub cursor_warp: bool,
        pub cursor_theme: String = "default".into(),
        pub cursor_size: u32 = 24,
        pub keyboard: Keyboard = Keyboard::default(),
        pub touchpad: Touchpad = Touchpad::default(),
        pub mouse: Mouse = Mouse::default(),
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Keyboard {
        pub layout: String = "de".into(),
        pub variant: String = "nodeadkeys".into(),
        pub options: String,
        pub repeat_rate: i32 = 30,
        pub repeat_delay: i32 = 300,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Touchpad {
        pub accel_profile: AccelProfile = AccelProfile::Adaptive,
        pub accel_speed: f64 = 0.4,
        pub natural_scroll: bool = true,
        pub left_handed: bool,
        pub middle_emulation: bool,
        pub tap: bool = true,
        pub tap_and_drag: bool = true,
        pub drag_lock: bool = true,
        pub disable_while_typing: bool = true,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(default)]
    pub struct Mouse {
        pub accel_profile: AccelProfile = AccelProfile::Flat,
        pub accel_speed: f64,
        pub natural_scroll: bool,
        pub left_handed: bool,
        pub middle_emulation: bool,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum AccelProfile {
    Flat,
    Adaptive,
}

impl From<AccelProfile> for InputAccelProfile {
    fn from(p: AccelProfile) -> Self {
        match p {
            AccelProfile::Flat => Self::Flat,
            AccelProfile::Adaptive => Self::Adaptive,
        }
    }
}

// --- Config ---

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub outputs: Vec<OutputRule>,
    pub layout: Layout,
    pub windows: Vec<WindowRule>,
    pub input: Input,
    #[serde(deserialize_with = "de_keys")]
    pub keybinds: Vec<(Vec<Mod>, Keysym, KeyAction)>,
    pub mousebinds: Vec<(Vec<Mod>, Button, MouseAction)>,
    #[serde(skip)]
    pub key_map: HashMap<(Keysym, Mods), KeyAction>,
    #[serde(skip)]
    pub mouse_map: HashMap<(u32, Mods), MouseAction>,
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
    SetTag(usize),
    ToggleTag(usize),

    MasterCount(i32),
    MasterRatio(f32),
    ToggleFloating,
    ToggleFullscreen,
    SwapMaster,

    Close,
    Quit,
    Spawn(Vec<String>),
}

#[derive(Debug, Clone, Copy, Deserialize)]
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

        assert_eq!(file.layout, code.layout);
        assert_eq!(file.input, code.input);

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
        let ron = "(layout: (inner_gap: 10))";
        let config: Config = ron::from_str(ron).unwrap();
        assert_eq!(config.layout.inner_gap, 10);
        assert_eq!(config.layout.outer_gap, Layout::default().outer_gap);
        assert_eq!(config.input, Input::default());
    }

    #[test]
    fn empty_match_deserializes() {
        let ron = "#![enable(implicit_some)]\n(windows: [(match: (), render: [WindowSurface(fill: \"#000000\")])])";
        let config: Config = ron::from_str(ron).unwrap();
        assert_eq!(config.windows.len(), 1);
        assert!(config.windows[0].r#match.app_id.is_none());
        assert!(config.windows[0].r#match.floating.is_none());
    }

    #[test]
    fn pattern_exact_match() {
        let p: Pattern = ron::from_str("\"firefox\"").unwrap();
        assert!(p.is_match("firefox"));
        assert!(!p.is_match("firefox-esr"));
        assert!(!p.is_match("myfirefox"));
    }

    #[test]
    fn pattern_regex() {
        let p: Pattern = ron::from_str("\"firefox.*\"").unwrap();
        assert!(p.is_match("firefox"));
        assert!(p.is_match("firefox-esr"));
        assert!(!p.is_match("chromium"));
    }

    #[test]
    fn pattern_alternation() {
        let p: Pattern = ron::from_str("\"firefox|chromium\"").unwrap();
        assert!(p.is_match("firefox"));
        assert!(p.is_match("chromium"));
        assert!(!p.is_match("epiphany"));
    }

    #[test]
    fn pattern_invalid_regex() {
        let r = ron::from_str::<Pattern>("\"[invalid\"");
        assert!(r.is_err());
    }

    #[test]
    fn window_match_empty_matches_all() {
        let m = WindowMatch::default();
        assert!(m.matches("firefox", "YouTube", true));
        assert!(m.matches("", "", false));
    }

    #[test]
    fn window_match_app_id() {
        let ron = "#![enable(implicit_some)]\n(app_id: \"firefox\")";
        let m: WindowMatch = ron::from_str(ron).unwrap();
        assert!(m.matches("firefox", "any title", false));
        assert!(!m.matches("chromium", "any title", false));
    }

    #[test]
    fn window_match_floating() {
        let ron = "(floating: Some(true))";
        let m: WindowMatch = ron::from_str(ron).unwrap();
        assert!(m.matches("any", "any", true));
        assert!(!m.matches("any", "any", false));
    }

    #[test]
    fn window_match_combined() {
        let ron = "#![enable(implicit_some)]\n(app_id: \"firefox\", floating: true)";
        let m: WindowMatch = ron::from_str(ron).unwrap();
        assert!(m.matches("firefox", "any", true));
        assert!(!m.matches("firefox", "any", false));
        assert!(!m.matches("chromium", "any", true));
    }

    #[test]
    fn window_match_title_regex() {
        let ron = "#![enable(implicit_some)]\n(title: \".*YouTube.*\")";
        let m: WindowMatch = ron::from_str(ron).unwrap();
        assert!(m.matches("firefox", "Watching YouTube Now", false));
        assert!(!m.matches("firefox", "GitHub", false));
    }

    #[test]
    fn output_match_empty_matches_all() {
        let m = OutputMatch::default();
        assert!(m.matches("DP-1", "Dell", "U2723QE", "ABC123"));
    }

    #[test]
    fn output_match_name() {
        let ron = "#![enable(implicit_some)]\n(name: \"DP-.*\")";
        let m: OutputMatch = ron::from_str(ron).unwrap();
        assert!(m.matches("DP-1", "Dell", "U2723QE", "ABC123"));
        assert!(m.matches("DP-2", "Dell", "U2723QE", "ABC123"));
        assert!(!m.matches("HDMI-A-1", "Dell", "U2723QE", "ABC123"));
    }
}
