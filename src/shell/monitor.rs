// SPDX-License-Identifier: GPL-3.0-only

use derive_more::{Deref, DerefMut};
use smithay::{
    desktop::layer_map_for_output,
    output::{Output, Scale},
    reexports::wayland_server::{backend::GlobalId, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, Transform},
    wayland::{
        session_lock::LockSurface,
        shell::wlr_layer::{KeyboardInteractivity, Layer},
    },
};

use crate::config::{self, Config, ModeConfig};

use super::{Tag, WindowId, Windows};

#[derive(Debug)]
pub struct MonitorSettings {
    pub tags: Vec<String>,
    pub scale: Option<Scale>,
    pub pos: Point<i32, Logical>,
    pub mode: Option<ModeConfig>,
    pub transform: Option<Transform>,
    pub background: [f32; 4],
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            tags: config::default_tags(),
            scale: None,
            pos: Point::default(),
            mode: None,
            transform: None,
            background: [0.0; 4],
        }
    }
}

impl MonitorSettings {
    pub fn resolve(
        rules: &[config::OutputRule],
        name: &str,
        make: &str,
        model: &str,
        serial: &str,
    ) -> Self {
        let mut s = Self::default();
        for rule in rules {
            if !rule.r#match.matches(name, make, model, serial) {
                continue;
            }
            if let Some(t) = &rule.tags {
                s.tags = t.clone();
            }
            s.scale = rule.scale.map(Scale::Fractional).or(s.scale);
            if let Some((x, y)) = rule.pos {
                s.pos = (x, y).into();
            }
            s.mode = rule.mode.or(s.mode);
            s.transform = rule.transform.map(Transform::from).or(s.transform);
            s.background = rule.background.map_or(s.background, |c| c.0);
        }
        if s.tags.is_empty() {
            s.tags = config::default_tags();
        }
        s
    }
}

#[derive(Debug)]
pub struct Monitor {
    pub output: Output,
    pub global: GlobalId,
    pub settings: MonitorSettings,
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub prev_tag: usize,
    pub exclusive_layer: Option<WlSurface>,
    pub lock_surface: Option<LockSurface>,
}

impl Monitor {
    pub fn tag(&self) -> &Tag {
        &self.tags[self.active_tag]
    }

    pub fn tag_mut(&mut self) -> &mut Tag {
        &mut self.tags[self.active_tag]
    }

    pub fn map(&mut self, ws: &mut Windows, id: WindowId, tags: Option<Vec<usize>>) {
        let area = layer_map_for_output(&self.output).non_exclusive_zone();
        let we = &mut ws[id];

        let fw = if we.float_geo.size.w > 0 { we.float_geo.size.w } else { area.size.w * 3 / 4 };
        let fh = if we.float_geo.size.h > 0 { we.float_geo.size.h } else { area.size.h * 3 / 4 };

        let has_pos = we.float_geo.loc != Point::default();
        let x = if has_pos { we.float_geo.loc.x } else { area.loc.x + (area.size.w - fw) / 2 };
        let y = if has_pos { we.float_geo.loc.y } else { area.loc.y + (area.size.h - fh) / 2 };
        we.float_geo = Rectangle::new((x, y).into(), (fw, fh).into());

        if let Some(tags) = tags {
            for t in tags {
                if t < self.tags.len() {
                    self.tags[t].add(id);
                }
            }
        } else {
            self.tag_mut().add(id);
        }
    }

    pub fn unmap(&mut self, id: WindowId) {
        for tag in &mut self.tags {
            tag.remove(id);
        }
    }

    pub fn move_to_tag(&mut self, ws: &mut Windows, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        let Some(id) = self.tag().focused_id() else {
            return;
        };
        if let Some(we) = ws.get_mut(id) {
            we.set_fullscreen(None);
        }
        for t in &mut self.tags {
            t.remove(id);
        }
        self.tags[tag].add(id);
    }

    pub fn toggle_tag(&mut self, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        let Some(id) = self.tag().focused_id() else {
            return;
        };
        if self.tags[tag].contains(id) {
            let count = self.tags.iter().filter(|t| t.contains(id)).count();
            if count > 1 {
                self.tags[tag].remove(id);
            }
        } else {
            self.tags[tag].add(id);
        }
    }

    pub fn set_active_tag(&mut self, tag: usize) {
        if tag >= self.tags.len() {
            return;
        }
        self.prev_tag = self.active_tag;
        self.active_tag = tag;
    }

    pub fn toggle_prev_tag(&mut self) {
        std::mem::swap(&mut self.active_tag, &mut self.prev_tag);
    }

    pub fn output_geometry(&self) -> Rectangle<i32, Logical> {
        let size = self.output.current_mode().unwrap().size;
        Rectangle::new((0, 0).into(), size.to_logical(1))
    }

    pub fn recompute_layout(&mut self, ws: &mut Windows, config: &Config) {
        let tag = &mut self.tags[self.active_tag];

        tag.tiled
            .retain(|&id| ws.get(id).is_some_and(|we| !we.floating));

        tag.floating
            .retain(|&id| ws.get(id).is_some_and(|we| we.floating));

        for &id in &tag.focus_stack {
            let Some(we) = ws.get(id) else { continue };
            if we.floating {
                if !tag.floating.contains(&id) {
                    tag.floating.push(id);
                }
            } else if !tag.tiled.contains(&id) {
                tag.tiled.push(id);
            }
        }
        tag.fullscreen = tag
            .focus_stack
            .iter()
            .copied()
            .find(|&id| ws.get(id).is_some_and(|we| we.fullscreen));

        let geo = layer_map_for_output(&self.output).non_exclusive_zone();
        let rects = tag
            .layout
            .compute_rects(tag.tiled.len(), geo, &config.layout);
        for (&id, rect) in tag.tiled.iter().zip(rects) {
            if let Some(we) = ws.get_mut(id) {
                we.tiled_geo = rect;
            }
        }
        for &id in &tag.focus_stack {
            if let Some(we) = ws.get_mut(id) {
                we.configure();
            }
        }
    }

    pub fn window_ids(&self) -> Vec<WindowId> {
        self.tags
            .iter()
            .flat_map(|t| &t.focus_stack)
            .copied()
            .collect()
    }

    pub fn update_exclusive_layer(&mut self) {
        let map = layer_map_for_output(&self.output);
        self.exclusive_layer = None;
        for l in [Layer::Overlay, Layer::Top] {
            for s in map.layers_on(l).rev() {
                if s.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive {
                    self.exclusive_layer = Some(s.wl_surface().clone());
                    return;
                }
            }
        }
    }
}

#[derive(Debug, Default, Deref, DerefMut)]
pub struct Monitors(pub Vec<Monitor>);

impl Monitors {
    pub fn by_output(&self, output: &Output) -> Option<(usize, &Monitor)> {
        self.iter().enumerate().find(|(_, m)| m.output == *output)
    }

    pub fn update_rules(&mut self, rules: &[config::OutputRule]) {
        for mon in self.iter_mut() {
            let props = mon.output.physical_properties();
            let s = MonitorSettings::resolve(
                rules,
                &mon.output.name(),
                &props.make,
                &props.model,
                &props.serial_number,
            );
            // move windows from removed tags to the last remaining tag
            let new_len = s.tags.len();
            if new_len < mon.tags.len() {
                let mut orphaned = Vec::new();
                for tag in &mon.tags[new_len..] {
                    orphaned.extend_from_slice(&tag.focus_stack);
                }
                mon.tags.truncate(new_len);
                let dest = new_len - 1;
                for id in orphaned {
                    mon.tags[dest].add(id);
                }
            }
            mon.tags.resize_with(new_len, Tag::default);
            mon.active_tag = mon.active_tag.min(new_len - 1);
            mon.prev_tag = mon.prev_tag.min(new_len - 1);
            mon.settings = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Color, OutputMatch, OutputRule, OutputTransform};

    fn rule(name: Option<&str>) -> OutputRule {
        OutputRule {
            r#match: OutputMatch {
                name: name.map(|n| ron::from_str(&format!("\"{n}\"")).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn empty_rules_gives_default_tags() {
        let s = MonitorSettings::resolve(&[], "DP-1", "Dell", "U2720Q", "ABC123");
        assert_eq!(s.tags, config::default_tags());
    }

    #[test]
    fn catch_all_rule_applies() {
        let mut r = rule(None);
        r.scale = Some(2.0);
        r.pos = Some((100, 200));
        r.transform = Some(OutputTransform::_90);
        r.background = Some(Color([1.0, 0.0, 0.0, 1.0]));

        let s = MonitorSettings::resolve(&[r], "DP-1", "Dell", "U2720Q", "ABC123");
        assert!(matches!(s.scale, Some(Scale::Fractional(v)) if v == 2.0));
        assert_eq!(s.pos, Point::from((100, 200)));
        assert_eq!(s.transform, Some(Transform::_90));
        assert_eq!(s.background, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn name_filter_skips_non_matching() {
        let mut r = rule(Some("HDMI-A-1"));
        r.scale = Some(2.0);

        let s = MonitorSettings::resolve(&[r], "DP-1", "Dell", "U2720Q", "ABC123");
        assert!(s.scale.is_none());
    }

    #[test]
    fn later_rule_overrides_earlier() {
        let mut r1 = rule(None);
        r1.scale = Some(1.0);
        r1.tags = Some(vec!["a".into(), "b".into()]);

        let mut r2 = rule(Some("DP-1"));
        r2.scale = Some(2.0);
        r2.tags = Some(vec!["x".into()]);

        let s = MonitorSettings::resolve(&[r1, r2], "DP-1", "Dell", "U2720Q", "ABC123");
        assert!(matches!(s.scale, Some(Scale::Fractional(v)) if v == 2.0));
        assert_eq!(s.tags, vec!["x"]);
    }

    #[test]
    fn partial_override_preserves_earlier() {
        let mut r1 = rule(None);
        r1.scale = Some(1.5);
        r1.pos = Some((10, 20));

        let mut r2 = rule(None);
        r2.scale = Some(2.0);
        // no pos in r2

        let s = MonitorSettings::resolve(&[r1, r2], "DP-1", "Dell", "U2720Q", "ABC123");
        assert!(matches!(s.scale, Some(Scale::Fractional(v)) if v == 2.0));
        assert_eq!(s.pos, Point::from((10, 20)));
    }

    #[test]
    fn mode_config_resolves() {
        let mut r = rule(None);
        r.mode = Some(ModeConfig {
            size: (2560, 1440),
            refresh: Some(144),
        });

        let s = MonitorSettings::resolve(&[r], "DP-1", "Dell", "U2720Q", "ABC123");
        let m = s.mode.unwrap();
        assert_eq!(m.size, (2560, 1440));
        assert_eq!(m.refresh, Some(144));
    }
}
