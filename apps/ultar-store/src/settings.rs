//! The `updates.*` settings the Store follows for **app** updates (`updates.auto_apps`):
//! whether it installs available app updates by itself, and the same window and metered
//! rules the system updater uses (`updates.window.from`/`.until`, `updates.metered`).
//!
//! The system updater (`system/core/ultar-update`) installs only system images; app updates
//! belong to the Store (docs/integration/settings-wiring.md). Its policy helpers are not in a
//! crate the apps area may depend on (`tools/check-layers.toml`), so the small decision logic
//! is kept here, documented, with the same semantics: a daily UTC window that may cross
//! midnight (equal ends = any time), and a metered connection blocks automatic work unless
//! `updates.metered` allows it (an unknown connection is treated as metered).
//!
//! Values are validated before use; a value of the wrong type or out of range is refused and
//! the last good value stays (`settingsd` validates what it stores). Off = only offer updates.

/// `updates.auto_apps` – install available app updates by itself.
pub const AUTO_APPS: &str = "updates.auto_apps";
/// `updates.metered` – automatic downloads may use a metered connection.
pub const METERED: &str = "updates.metered";
/// `updates.window.from` – start of the daily install window (`HH:MM`, UTC).
pub const WINDOW_FROM: &str = "updates.window.from";
/// `updates.window.until` – end of the daily install window (`HH:MM`, UTC).
pub const WINDOW_UNTIL: &str = "updates.window.until";

/// Every setting the Store follows for app updates (read at start and on `settings.changed`).
pub const KEYS: [&str; 4] = [AUTO_APPS, METERED, WINDOW_FROM, WINDOW_UNTIL];

/// Minutes in a day.
const DAY_MINUTES: u16 = 24 * 60;

/// The app-update settings as the person set them. Defaults match the `settingsd` seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateSettings {
    /// `updates.auto_apps`.
    pub auto_apps: bool,
    /// `updates.metered`.
    pub metered: bool,
    /// `updates.window.from` as minutes of the day (UTC).
    pub window_from: u16,
    /// `updates.window.until` as minutes of the day (UTC).
    pub window_until: u16,
}

impl Default for AppUpdateSettings {
    fn default() -> Self {
        Self {
            auto_apps: true,
            metered: false,
            window_from: 2 * 60,
            window_until: 5 * 60,
        }
    }
}

/// `"HH:MM"` (24 hours, two digits each) → minutes of the day; `None` when it does not parse.
/// The same strict rule the system updater uses (`ultar-update::settings::parse_time`).
pub fn parse_time(text: &str) -> Option<u16> {
    let (hours, minutes) = text.split_once(':')?;
    let pair = |part: &str| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u16>().ok())
            .flatten()
    };
    let hours = pair(hours)?;
    let minutes = pair(minutes)?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

impl AppUpdateSettings {
    /// Takes a flag setting (`updates.auto_apps`, `updates.metered`); true when the key is
    /// one of them. A foreign key is refused (the last good value stays).
    pub fn set_flag(&mut self, key: &str, on: bool) -> bool {
        match key {
            AUTO_APPS => {
                self.auto_apps = on;
                true
            }
            METERED => {
                self.metered = on;
                true
            }
            _ => false,
        }
    }

    /// Takes a text setting (`updates.window.from`/`.until`); true when the key is one of
    /// them **and** the value parses as `HH:MM`. An out-of-range time is refused.
    pub fn set_text(&mut self, key: &str, text: &str) -> bool {
        let field = match key {
            WINDOW_FROM => &mut self.window_from,
            WINDOW_UNTIL => &mut self.window_until,
            _ => return false,
        };
        match parse_time(text) {
            Some(v) => {
                *field = v;
                true
            }
            None => false,
        }
    }

    /// Takes one setting from the bus; true when it belongs to the Store and parses. A value
    /// of the wrong type or out of range is refused (the last good value stays).
    #[cfg(feature = "bus")]
    pub fn take(&mut self, key: &str, value: &ultar_bus::messages::settings::SettingValue) -> bool {
        use ultar_bus::messages::settings::SettingValue;
        match value {
            SettingValue::Flag(on) => self.set_flag(key, *on),
            SettingValue::Text(t) => self.set_text(key, t),
            _ => false,
        }
    }

    /// True when `now` (seconds since 1970, UTC) lies inside the daily window. Equal ends
    /// mean "any time"; the window may cross midnight.
    pub fn in_window(&self, now: u64) -> bool {
        let minute = ((now / 60) % u64::from(DAY_MINUTES)) as u16;
        let (start, end) = (self.window_from, self.window_until);
        if start == end {
            true
        } else if start < end {
            (start..end).contains(&minute)
        } else {
            minute >= start || minute < end
        }
    }

    /// May automatic app updates run now? `metered`: what the network service reports
    /// (`None` = unknown, treated as metered). Off, outside the window, or a metered
    /// connection that is not allowed all block it.
    pub fn allowed_now(&self, now: u64, metered: Option<bool>) -> bool {
        self.auto_apps && self.in_window(now) && (metered == Some(false) || self.metered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds since 1970 at `h:m` UTC on a fixed day.
    fn at(h: u64, m: u64) -> u64 {
        86_400 * 3 + h * 3600 + m * 60
    }

    #[test]
    fn defaults_match_the_seed() {
        let s = AppUpdateSettings::default();
        assert!(s.auto_apps && !s.metered);
        assert_eq!((s.window_from, s.window_until), (120, 300));
    }

    #[test]
    fn good_values_are_taken_and_bad_ones_refused() {
        let mut s = AppUpdateSettings::default();
        assert!(s.set_flag(AUTO_APPS, false));
        assert!(s.set_flag(METERED, true));
        assert!(s.set_text(WINDOW_FROM, "23:30"));
        assert!(s.set_text(WINDOW_UNTIL, "04:15"));
        assert!(!s.auto_apps && s.metered);
        assert_eq!((s.window_from, s.window_until), (1410, 255));
        // Out-of-range times and foreign keys: refused, the last good value stays.
        for (key, text) in [
            (WINDOW_FROM, "24:00"),
            (WINDOW_FROM, "2:00"),
            (WINDOW_UNTIL, "12:60"),
            (WINDOW_UNTIL, ""),
            ("updates.channel", "preview"),
        ] {
            assert!(!s.set_text(key, text), "{key}");
        }
        assert!(!s.set_flag("updates.auto_system", false));
        assert!(!s.set_flag("updates.channel", true));
        assert!(!s.auto_apps && s.metered);
        assert_eq!((s.window_from, s.window_until), (1410, 255));
    }

    #[test]
    fn the_window_and_metered_gate_automatic_work() {
        let mut s = AppUpdateSettings::default(); // 02:00–05:00, metered off
                                                  // Inside the window, unmetered: allowed.
        assert!(s.allowed_now(at(3, 0), Some(false)));
        // Outside the window: blocked.
        assert!(!s.allowed_now(at(12, 0), Some(false)));
        // Metered (or unknown) connection without consent: blocked.
        assert!(!s.allowed_now(at(3, 0), Some(true)));
        assert!(!s.allowed_now(at(3, 0), None));
        // Allowing metered lets an unknown/metered connection through.
        s.set_flag(METERED, true);
        assert!(s.allowed_now(at(3, 0), None));
        assert!(s.allowed_now(at(3, 0), Some(true)));
        // Switched off: never, even in the window on a good connection.
        s.set_flag(AUTO_APPS, false);
        assert!(!s.allowed_now(at(3, 0), Some(false)));
    }

    #[test]
    fn a_window_crossing_midnight_and_equal_ends() {
        let mut s = AppUpdateSettings::default();
        s.set_text(WINDOW_FROM, "23:00");
        s.set_text(WINDOW_UNTIL, "05:00");
        assert!(s.in_window(at(23, 30)));
        assert!(s.in_window(at(0, 30)));
        assert!(s.in_window(at(4, 59)));
        assert!(!s.in_window(at(5, 0)));
        assert!(!s.in_window(at(12, 0)));
        // Equal ends: any time.
        s.set_text(WINDOW_FROM, "07:00");
        s.set_text(WINDOW_UNTIL, "07:00");
        assert!(s.in_window(at(15, 0)));
    }

    #[test]
    fn times_parse_strictly() {
        assert_eq!(parse_time("00:00"), Some(0));
        assert_eq!(parse_time("23:59"), Some(1439));
        for bad in ["24:00", "23:60", "2:00", "0200", "", "aa:bb"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }
}
