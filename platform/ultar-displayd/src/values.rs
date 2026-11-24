//! The display settings of `settingsd` that decide the arrangement, as the
//! service follows them (`settings.list` at start, then `settings.changed`).
//! Values that do not parse are ignored (the setting keeps its last good
//! value here; settingsd validates what it stores).

use std::collections::BTreeMap;
use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::Transform;

/// Most displays followed (as `settingsd`).
pub const MAX_DISPLAYS: usize = 16;

/// `display.mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModeWord {
    /// `extend`.
    #[default]
    Extend,
    /// `mirror`.
    Mirror,
    /// `single`.
    Single,
}

/// The per-display settings.
#[derive(Clone, Debug, PartialEq)]
pub struct PerDisplay {
    /// `resolution` (`None` = automatic: the preferred mode).
    pub resolution: Option<(u32, u32)>,
    /// `scale` in percent.
    pub scale: u32,
    /// `refresh_rate` in hertz (0 = recommended).
    pub refresh_hz: f64,
    /// `orientation`.
    pub transform: Transform,
    /// `position_x`, `position_y`.
    pub position: (i32, i32),
}

impl Default for PerDisplay {
    fn default() -> Self {
        PerDisplay {
            resolution: None,
            scale: 100,
            refresh_hz: 0.0,
            transform: Transform::Normal,
            position: (0, 0),
        }
    }
}

/// Every arrangement setting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayValues {
    /// `display.mode`.
    pub mode: ModeWord,
    /// `display.mirror_displays` (empty: all).
    pub mirror: Vec<String>,
    /// `display.single_display` (empty: automatic).
    pub single: String,
    /// `display.primary` (empty: automatic).
    pub primary: String,
    /// Per display id.
    pub displays: BTreeMap<String, PerDisplay>,
}

fn text(v: &SettingValue) -> Option<&str> {
    match v {
        SettingValue::Text(t) => Some(t),
        _ => None,
    }
}

fn int(v: &SettingValue) -> Option<i64> {
    match v {
        SettingValue::Integer(i) => Some(*i),
        SettingValue::Decimal(d) if d.fract() == 0.0 => Some(*d as i64),
        _ => None,
    }
}

/// `"1920x1080"` → `(1920, 1080)`; `"auto"` → `None`.
pub fn parse_resolution(t: &str) -> Option<Option<(u32, u32)>> {
    if t == "auto" {
        return Some(None);
    }
    let (w, h) = t.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some(Some((w, h)))
}

/// The settings word of a turn.
pub fn orientation_word(t: Transform) -> &'static str {
    match t {
        Transform::Normal => "landscape",
        Transform::Rotate90 => "portrait",
        Transform::Rotate180 => "landscape_flipped",
        Transform::Rotate270 => "portrait_flipped",
    }
}

fn orientation(t: &str) -> Option<Transform> {
    [
        Transform::Normal,
        Transform::Rotate90,
        Transform::Rotate180,
        Transform::Rotate270,
    ]
    .into_iter()
    .find(|&x| orientation_word(x) == t)
}

impl DisplayValues {
    /// Takes one setting; true when it belongs to the arrangement.
    pub fn take(&mut self, key: &str, value: &SettingValue) -> bool {
        let Some(rest) = key.strip_prefix("display.") else {
            return false;
        };
        match rest {
            "mode" => self.take_mode(value),
            "mirror_displays" => match value {
                SettingValue::List(l) => {
                    self.mirror = l.iter().take(MAX_DISPLAYS).cloned().collect();
                    true
                }
                _ => false,
            },
            "single_display" => text(value).map(|t| self.single = t.to_owned()).is_some(),
            "primary" => text(value).map(|t| self.primary = t.to_owned()).is_some(),
            _ => self.take_per_display(rest, value),
        }
    }

    fn take_mode(&mut self, value: &SettingValue) -> bool {
        let mode = match text(value) {
            Some("extend") => ModeWord::Extend,
            Some("mirror") => ModeWord::Mirror,
            Some("single") => ModeWord::Single,
            _ => return false,
        };
        self.mode = mode;
        true
    }

    fn take_per_display(&mut self, rest: &str, value: &SettingValue) -> bool {
        let Some((id, field)) = rest.rsplit_once('.') else {
            return false;
        };
        let known = self.displays.contains_key(id);
        if id.contains('.') || (!known && self.displays.len() >= MAX_DISPLAYS) {
            return false;
        }
        let mut entry = self.displays.get(id).cloned().unwrap_or_default();
        let ok = match field {
            "resolution" => text(value)
                .and_then(parse_resolution)
                .map(|r| entry.resolution = r),
            "scale" => int(value)
                .filter(|s| (100..=300).contains(s))
                .map(|s| entry.scale = s as u32),
            "refresh_rate" => match value {
                SettingValue::Decimal(d) if (0.0..=1000.0).contains(d) => {
                    entry.refresh_hz = *d;
                    Some(())
                }
                SettingValue::Integer(i) if (0..=1000).contains(i) => {
                    entry.refresh_hz = *i as f64;
                    Some(())
                }
                _ => None,
            },
            "orientation" => text(value)
                .and_then(orientation)
                .map(|t| entry.transform = t),
            "position_x" => int(value).map(|x| entry.position.0 = clamp_position(x)),
            "position_y" => int(value).map(|y| entry.position.1 = clamp_position(y)),
            _ => None,
        };
        if ok.is_some() {
            self.displays.insert(id.to_owned(), entry);
        }
        ok.is_some()
    }
}

fn clamp_position(v: i64) -> i32 {
    v.clamp(-32_768, 32_768) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> SettingValue {
        SettingValue::Text(s.into())
    }

    #[test]
    fn settings_are_followed_and_nonsense_is_ignored() {
        let mut v = DisplayValues::default();
        assert!(v.take("display.mode", &t("mirror")));
        assert!(!v.take("display.mode", &t("sideways")));
        assert_eq!(v.mode, ModeWord::Mirror);
        assert!(v.take(
            "display.mirror_displays",
            &SettingValue::List(vec!["a".into()])
        ));
        assert!(v.take("display.primary", &t("a")));
        assert!(v.take("display.single_display", &t("b")));
        assert!(v.take("display.a.resolution", &t("1280x720")));
        assert!(v.take("display.a.scale", &SettingValue::Integer(150)));
        assert!(v.take("display.a.refresh_rate", &SettingValue::Decimal(59.94)));
        assert!(v.take("display.a.orientation", &t("portrait")));
        assert!(v.take("display.a.position_x", &SettingValue::Integer(-99_999)));
        assert!(v.take("display.a.position_y", &SettingValue::Integer(20)));
        let a = &v.displays["a"];
        assert_eq!(a.resolution, Some((1280, 720)));
        assert_eq!((a.scale, a.transform), (150, Transform::Rotate90));
        assert_eq!(a.position, (-32_768, 20));
        assert!(v.take("display.a.resolution", &t("auto")));
        assert_eq!(v.displays["a"].resolution, None);
        for (k, val) in [
            ("display.a.scale", SettingValue::Integer(900)),
            ("display.a.resolution", t("0x10")),
            ("display.a.orientation", t("diagonal")),
            ("display.a.brightness", SettingValue::Integer(3)),
            ("display.night_light.enabled", SettingValue::Flag(true)),
            ("display.a.refresh_rate", SettingValue::Decimal(-1.0)),
            ("power.mode", t("x")),
            ("display.mode", SettingValue::Integer(1)),
        ] {
            assert!(!v.take(k, &val), "{k}");
        }
        assert_eq!(v.displays["a"].scale, 150);
        assert_eq!(parse_resolution("12x"), None);
    }

    #[test]
    fn the_number_of_displays_is_bounded() {
        let mut v = DisplayValues::default();
        for i in 0..40 {
            v.take(&format!("display.d{i}.scale"), &SettingValue::Integer(125));
        }
        assert_eq!(v.displays.len(), MAX_DISPLAYS);
    }
}
