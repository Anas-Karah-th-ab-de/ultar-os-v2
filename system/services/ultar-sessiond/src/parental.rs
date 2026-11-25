//! `users.parental`: standard users may sign in only in the time window; the session
//! service warns them shortly before it ends and signs them out at its end. The lock
//! screen refuses the sign-in itself (platform/ultar-desktop, lock/prefs.rs – the same
//! window rules); this is the enforcement for sessions that are already open.

use ultar_bus::messages::settings::SettingValue;

/// The restriction is on.
pub const PARENTAL: &str = "users.parental";
/// Start of the window (`HH:MM`).
pub const FROM: &str = "users.parental.from";
/// End of the window (`HH:MM`).
pub const UNTIL: &str = "users.parental.until";

/// Every key the session service reads.
pub const KEYS: [&str; 3] = [PARENTAL, FROM, UNTIL];

/// Minutes before the end of the window when the warning is shown.
pub const WARN_MIN: u16 = 5;

/// Minutes in a day.
const DAY: u16 = 24 * 60;

/// `HH:MM` as minutes since midnight; `None` for anything else.
pub fn minutes(text: &str) -> Option<u16> {
    let (h, m) = text.split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Whether the window allows sign-in at `min` minutes since midnight. A window whose
/// start is after its end runs over midnight (22:00–06:00); equal ends mean the whole
/// day (no restriction).
pub fn window_allows(from: u16, until: u16, min: u16) -> bool {
    if from == until {
        return true;
    }
    if from < until {
        from <= min && min < until
    } else {
        min >= from || min < until
    }
}

/// What is due for a standard user's session now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// Nothing (restriction off, inside the window, far from its end).
    Nothing,
    /// Warn: the window ends within [`WARN_MIN`] minutes.
    Warn,
    /// End the session: outside the window (at its end, or a session that outlived it).
    End,
}

/// The values in effect, with the defaults of the settings schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parental {
    /// The restriction is on.
    pub on: bool,
    /// Start of the window, minutes since midnight.
    pub from_min: u16,
    /// End of the window, minutes since midnight.
    pub until_min: u16,
}

impl Default for Parental {
    fn default() -> Self {
        Parental {
            on: false,
            from_min: 7 * 60,
            until_min: 21 * 60,
        }
    }
}

impl Parental {
    /// Takes a setting that changed; `true` when it was one of these and differs.
    /// A value of the wrong type or outside its range changes nothing.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = self.clone();
        match (key, value) {
            (PARENTAL, SettingValue::Flag(on)) => self.on = *on,
            (FROM, SettingValue::Text(t)) => {
                if let Some(m) = minutes(t) {
                    self.from_min = m;
                }
            }
            (UNTIL, SettingValue::Text(t)) => {
                if let Some(m) = minutes(t) {
                    self.until_min = m;
                }
            }
            _ => {}
        }
        *self != before
    }

    /// What is due for a standard user at `min` minutes since midnight.
    pub fn due(&self, min: u16) -> Due {
        if !self.on || self.from_min == self.until_min {
            return Due::Nothing;
        }
        if !window_allows(self.from_min, self.until_min, min) {
            return Due::End;
        }
        let to_end = (self.until_min + DAY - min) % DAY;
        if to_end <= WARN_MIN {
            Due::Warn
        } else {
            Due::Nothing
        }
    }
}

/// Minutes since midnight of the wall clock now (UTC – the system clock's zone until
/// `datetime.timezone.name` is wired, the same basis the lock screen uses).
pub fn minute_now() -> u16 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    u16::try_from(secs.rem_euclid(86_400) / 60).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(from: u16, until: u16) -> Parental {
        Parental {
            on: true,
            from_min: from,
            until_min: until,
        }
    }

    #[test]
    fn defaults_and_validation() {
        let mut p = Parental::default();
        assert!(!p.on);
        assert_eq!((p.from_min, p.until_min), (420, 1260));
        assert!(p.on_changed(PARENTAL, &SettingValue::Flag(true)));
        assert!(p.on_changed(FROM, &SettingValue::Text("22:00".into())));
        assert_eq!(p.from_min, 1320);
        for bad in [
            SettingValue::Text("24:00".into()),
            SettingValue::Text("12:60".into()),
            SettingValue::Flag(false),
            SettingValue::Integer(0),
        ] {
            assert!(!p.on_changed(FROM, &bad), "{bad:?}");
        }
        assert_eq!(p.from_min, 1320);
        assert!(!p.on_changed("users.guest", &SettingValue::Flag(true)));
    }

    #[test]
    fn warn_before_the_end_and_end_outside_the_window() {
        let p = on(420, 1260); // 07:00–21:00
        assert_eq!(p.due(12 * 60), Due::Nothing);
        assert_eq!(p.due(20 * 60 + 54), Due::Nothing);
        assert_eq!(p.due(20 * 60 + 55), Due::Warn);
        assert_eq!(p.due(20 * 60 + 59), Due::Warn);
        assert_eq!(p.due(21 * 60), Due::End);
        assert_eq!(
            p.due(23 * 60),
            Due::End,
            "a session that outlived the window"
        );
        assert_eq!(p.due(6 * 60), Due::End);
        assert_eq!(p.due(7 * 60), Due::Nothing, "the window opens again");
        // Off, or equal ends (whole day): nothing is ever due.
        assert_eq!(Parental::default().due(23 * 60), Due::Nothing);
        assert_eq!(on(600, 600).due(23 * 60), Due::Nothing);
    }

    #[test]
    fn a_window_over_midnight_warns_and_ends_at_its_end() {
        let p = on(1320, 360); // 22:00–06:00
        assert_eq!(p.due(23 * 60), Due::Nothing);
        assert_eq!(p.due(5 * 60 + 56), Due::Warn);
        assert_eq!(p.due(6 * 60), Due::End);
        assert_eq!(p.due(12 * 60), Due::End);
        assert_eq!(p.due(22 * 60), Due::Nothing);
        // An end shortly after midnight: the warning wraps correctly.
        let p = on(1320, 2); // 22:00–00:02
        assert_eq!(p.due(23 * 60 + 58), Due::Warn);
        assert_eq!(p.due(0), Due::Warn);
        assert_eq!(p.due(2), Due::End);
    }

    #[test]
    fn minutes_parse_only_valid_clocks() {
        assert_eq!(minutes("07:00"), Some(420));
        assert_eq!(minutes("23:59"), Some(1439));
        assert_eq!(minutes("7:5"), Some(425));
        assert_eq!(minutes("24:00"), None);
        assert_eq!(minutes("noon"), None);
        assert!(minute_now() < 1440);
    }
}
