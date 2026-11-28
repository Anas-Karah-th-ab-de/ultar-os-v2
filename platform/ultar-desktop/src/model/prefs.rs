//! What the person set in Settings that changes how the shell looks and reads (the settings
//! service's values, taken while the shell runs – a change shows at once, without a new
//! start): accent colour, text size, the clock of the bar, the time zone, the bar's icons.

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
}
