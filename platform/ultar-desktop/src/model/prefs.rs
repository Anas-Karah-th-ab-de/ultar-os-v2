//! What the person set in Settings that changes how the shell looks and reads (the settings
//! service's values, taken while the shell runs – a change shows at once, without a new
//! start): accent colour, text size, the clock of the bar, the time zone, the bar's icons
//! and the desktop's shortcuts.

use ultar_i18n::datefmt::ClockStyle;
use ultar_ui::tokens::Accent;

/// Smallest and largest text size in percent (`appearance.font_size`).
pub const FONT_RANGE: (u16, u16) = (80, 200);

/// The bar's clock (`datetime.clock_format`, `bar.clock.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockPrefs {
    /// 12 or 24 hours.
    pub style: ClockStyle,
    /// Seconds after the minutes.
    pub seconds: bool,
    /// The weekday in the date line.
    pub weekday: bool,
    /// The date line at all.
    pub date: bool,
    /// The Hijri date after the date.
    pub hijri: bool,
}

impl Default for ClockPrefs {
    /// The schema's defaults: 24 hours, no seconds, weekday and date, no Hijri date.
    fn default() -> ClockPrefs {
        ClockPrefs {
            style: ClockStyle::H24,
            seconds: false,
            weekday: true,
            date: true,
            hijri: false,
        }
    }
}

/// Size of the desktop's shortcuts (`desktop.icons.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IconSize {
    /// Small.
    Small,
    /// Medium (standard).
    #[default]
    Medium,
    /// Large.
    Large,
}

impl IconSize {
    /// All sizes, small to large.
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Large];

    /// The value of the setting (`small`, `medium`, `large`).
    pub fn parse(word: &str) -> Option<IconSize> {
        IconSize::ALL.into_iter().find(|s| s.word() == word)
    }

    /// The word of the setting.
    pub const fn word(self) -> &'static str {
        match self {
            IconSize::Small => "small",
            IconSize::Medium => "medium",
            IconSize::Large => "large",
        }
    }

    /// Edge of the shortcut's tile in points.
    pub fn side(self) -> u16 {
        use ultar_ui::tokens::{size, Space};
        let side = match self {
            IconSize::Small => size::TARGET_POINTER_MIN,
            IconSize::Medium => Space::S48.points(),
            IconSize::Large => 2.0 * Space::S32.points(),
        };
        side as u16
    }
}

/// The corner the first shortcut stands in (`desktop.icons.start_corner`); the others
/// follow down (or up) the column, then in the next column away from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Corner {
    /// Top, start side (standard).
    #[default]
    TopStart,
    /// Top, end side.
    TopEnd,
    /// Bottom, start side.
    BottomStart,
    /// Bottom, end side.
    BottomEnd,
}

impl Corner {
    /// The value of the setting (`top_start` …).
    pub fn parse(word: &str) -> Option<Corner> {
        Some(match word {
            "top_start" => Corner::TopStart,
            "top_end" => Corner::TopEnd,
            "bottom_start" => Corner::BottomStart,
            "bottom_end" => Corner::BottomEnd,
            _ => return None,
        })
    }

    /// The corner is at the bottom.
    pub fn bottom(self) -> bool {
        matches!(self, Corner::BottomStart | Corner::BottomEnd)
    }

    /// The corner is on the end side.
    pub fn end(self) -> bool {
        matches!(self, Corner::TopEnd | Corner::BottomEnd)
    }
}

/// The desktop's shortcuts as set in Settings › Desktop bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeskPrefs {
    /// `desktop.icons.enabled`: shortcuts shown at all.
    pub shown: bool,
    /// `desktop.icons.size`.
    pub size: IconSize,
    /// `desktop.icons.start_corner`.
    pub corner: Corner,
    /// `desktop.metrics.enabled`: the system display (CPU, memory, GPU, network).
    pub metrics: bool,
}

impl Default for DeskPrefs {
    fn default() -> DeskPrefs {
        DeskPrefs {
            shown: true,
            size: IconSize::Medium,
            corner: Corner::TopStart,
            metrics: true,
        }
    }
}

impl DeskPrefs {
    /// The settings of the desktop's icons and its system display.
    pub const KEYS: [&'static str; 4] = [
        "desktop.icons.enabled",
        "desktop.icons.size",
        "desktop.icons.start_corner",
        "desktop.metrics.enabled",
    ];

    /// Takes one of [`DeskPrefs::KEYS`]; `false` for any other key or value type.
    pub fn apply(
        &mut self,
        key: &str,
        value: &ultar_bus::messages::settings::SettingValue,
    ) -> bool {
        use ultar_bus::messages::settings::SettingValue;
        match (key, value) {
            ("desktop.icons.enabled", SettingValue::Flag(on)) => self.shown = *on,
            ("desktop.metrics.enabled", SettingValue::Flag(on)) => self.metrics = *on,
            ("desktop.icons.size", SettingValue::Text(t)) => {
                self.size = IconSize::parse(t).unwrap_or_default();
            }
            ("desktop.icons.start_corner", SettingValue::Text(t)) => {
                self.corner = Corner::parse(t).unwrap_or_default();
            }
            _ => return false,
        }
        true
    }
}

/// The shell's preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    /// `appearance.accent_color`.
    pub accent: Accent,
    /// `appearance.font_size` in percent.
    pub font_percent: u16,
    /// The bar's clock.
    pub clock: ClockPrefs,
    /// `datetime.timezone.name`; `None` until the service said (the clock's own offset).
    pub zone: Option<String>,
    /// `bar.icon_alignment` = `start`: the app tiles at the start instead of the centre.
    pub icons_start: bool,
    /// `bar.show_trash`.
    pub show_trash: bool,
    /// `bar.show_volumes`.
    pub show_volumes: bool,
    /// `power.show_battery_percent`.
    pub battery_percent: bool,
    /// The desktop's shortcuts (`desktop.icons.*`).
    pub desk: DeskPrefs,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            accent: Accent::Gold,
            font_percent: 100,
            clock: ClockPrefs::default(),
            zone: None,
            icons_start: false,
            show_trash: true,
            show_volumes: true,
            battery_percent: false,
            desk: DeskPrefs::default(),
        }
    }
}

impl Prefs {
    /// Text factor of `appearance.font_size` (1 = 100 %).
    pub fn font_factor(&self) -> f32 {
        f32::from(self.font_percent.clamp(FONT_RANGE.0, FONT_RANGE.1)) / 100.0
    }

    /// The offset of the chosen zone at `unix_ms`, or `fallback` while none is known.
    pub fn offset_s(&self, unix_ms: u64, fallback: i32) -> i32 {
        match &self.zone {
            Some(zone) => ultar_i18n::tz::offset(zone, i64::try_from(unix_ms / 1000).unwrap_or(0)),
            None => fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_the_zone() {
        let mut p = Prefs::default();
        assert_eq!(p.font_factor(), 1.0);
        assert_eq!(p.offset_s(0, 42), 42);
        p.zone = Some("Asia/Damascus".into());
        assert_eq!(p.offset_s(1_795_636_560_000, 0), 3 * 3600);
        p.font_percent = 500;
        assert_eq!(p.font_factor(), 2.0);
    }

    #[test]
    fn the_desktop_icons_follow_their_settings() {
        use ultar_bus::messages::settings::SettingValue;
        let mut d = DeskPrefs::default();
        assert!(d.shown && d.size == IconSize::Medium && d.corner == Corner::TopStart);
        let text = |t: &str| SettingValue::Text(t.into());
        assert!(d.apply("desktop.icons.enabled", &SettingValue::Flag(false)));
        assert!(d.apply("desktop.icons.size", &text("large")));
        assert!(d.apply("desktop.icons.start_corner", &text("bottom_end")));
        assert!(!d.shown && d.size == IconSize::Large);
        assert!(d.corner.bottom() && d.corner.end());
        assert!(d.apply("desktop.icons.size", &text("huge")));
        assert_eq!(d.size, IconSize::Medium, "an unknown size is the standard");
        assert!(!d.apply("desktop.icons.size", &SettingValue::Flag(true)));
        assert!(!d.apply("bar.show_trash", &SettingValue::Flag(true)));
        let sides: Vec<u16> = IconSize::ALL.iter().map(|s| s.side()).collect();
        assert_eq!(sides, vec![32, 48, 64]);
        assert!(Corner::parse("middle").is_none());
    }
}
