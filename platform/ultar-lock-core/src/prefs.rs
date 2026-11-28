//! The settings the lock screen follows (`users.*` and `emergency.*`): read at start
//! (`settings.get`) and on `settings.changed`. Every value is validated here before use;
//! a value of the wrong type or outside its range keeps the default of the settings schema
//! (system/core/ultar-settingsd/src/seed/services.rs).

use ultar_bus::messages::settings::SettingValue;

/// The sign-in lists the users (else only the session user's name field).
pub const LIST_ON_LOGIN: &str = "users.list_on_login";
/// The user that signs in at start without a password (empty: nobody).
pub const AUTO_LOGIN: &str = "users.auto_login";
/// Standard users may sign in only in the time window below.
pub const PARENTAL: &str = "users.parental";
/// Start of the window (`HH:MM`).
pub const PARENTAL_FROM: &str = "users.parental.from";
/// End of the window (`HH:MM`).
pub const PARENTAL_UNTIL: &str = "users.parental.until";
/// The emergency button on the cover.
pub const EMERGENCY_ON: &str = "emergency.on_lock_screen";
/// Name of the person told in an emergency (shown only when the person stored one).
pub const CONTACT_NAME: &str = "emergency.contact_name";
/// Their number.
pub const CONTACT_PHONE: &str = "emergency.contact_phone";
/// Blood type, allergies, medicines – what the person chose to show without unlocking.
pub const MEDICAL: &str = "emergency.medical";
/// The number the emergency button calls.
pub const NUMBER: &str = "emergency.number";
/// The screen flashes SOS while the emergency call runs.
pub const SOS_LIGHT: &str = "emergency.sos_light";

/// The system language (the lock screen speaks it from its first frame on).
pub const LANGUAGE: &str = "region.language";
/// Dark or light.
pub const THEME: &str = "appearance.theme";
/// The accent colour.
pub const ACCENT: &str = "appearance.accent_color";
/// The text size in percent.
pub const FONT_SIZE: &str = "appearance.font_size";
/// The time zone of the clock.
pub const ZONE: &str = "datetime.timezone.name";
/// 12 or 24 hours.
pub const CLOCK_FORMAT: &str = "datetime.clock_format";

/// The accent colours of the schema (gold first: the standard).
pub const ACCENTS: [&str; 6] = ["gold", "blue", "green", "red", "purple", "teal"];
/// Smallest and largest text size in percent.
pub const FONT_RANGE: (u16, u16) = (80, 200);

/// Every key the lock screen reads.
pub const KEYS: [&str; 17] = [
    LANGUAGE,
    THEME,
    ACCENT,
    FONT_SIZE,
    ZONE,
    CLOCK_FORMAT,
    LIST_ON_LOGIN,
    AUTO_LOGIN,
    PARENTAL,
    PARENTAL_FROM,
    PARENTAL_UNTIL,
    EMERGENCY_ON,
    CONTACT_NAME,
    CONTACT_PHONE,
    MEDICAL,
    NUMBER,
    SOS_LIGHT,
];

/// Longest account name (as the account service).
const MAX_NAME: usize = 32;
/// Longest emergency number (the schema's `max_chars`).
const MAX_NUMBER: usize = 16;
/// Shortest emergency number (112, 911, 999 …).
const MIN_NUMBER: usize = 3;

/// `HH:MM` as minutes since midnight; `None` for anything else.
pub fn minutes(text: &str) -> Option<u16> {
    let (h, m) = text.split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Minutes since midnight as `HH:MM` (for the message under the field).
pub fn clock_text(min: u16) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

/// A number the emergency button may call: an optional `+` then digits, 3–16 characters.
pub fn valid_number(text: &str) -> bool {
    let digits = text.strip_prefix('+').unwrap_or(text);
    (MIN_NUMBER..=MAX_NUMBER).contains(&text.len())
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Whether the time window allows sign-in at `min` minutes since midnight. A window
/// whose start is after its end runs over midnight (22:00–06:00); equal ends mean no
/// restriction (the whole day).
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

/// The values in effect, with the defaults of the settings schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPrefs {
    /// The sign-in lists the users.
    pub list_on_login: bool,
    /// The user that signs in at start (empty: nobody).
    pub auto_login: String,
    /// The time window restricts standard users.
    pub parental: bool,
    /// Start of the window, minutes since midnight.
    pub from_min: u16,
    /// End of the window, minutes since midnight.
    pub until_min: u16,
    /// The emergency button is on the cover.
    pub emergency_on: bool,
    /// Emergency contact name (data).
    pub contact_name: String,
    /// Emergency contact number (data).
    pub contact_phone: String,
    /// Medical details (data).
    pub medical: String,
    /// The number the emergency button calls.
    pub number: String,
    /// The screen flashes SOS during the emergency call.
    pub sos_light: bool,
    /// Language, theme, accent, text size and clock of the settings.
    pub look: LookPrefs,
}

/// What the lock screen shows of the person's choices: it speaks their language and shows
/// their time from its first frame on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookPrefs {
    /// `region.language` (`None` until the service said: the session's language).
    pub lang: Option<ultar_i18n::Lang>,
    /// `appearance.theme` = light (`None` until the service said: the start option).
    pub light: Option<bool>,
    /// `appearance.accent_color` (one of [`ACCENTS`]).
    pub accent: String,
    /// `appearance.font_size` in percent.
    pub font_percent: u16,
    /// `datetime.timezone.name` (`None`: the clock's own offset).
    pub zone: Option<String>,
    /// `datetime.clock_format` = h12.
    pub h12: bool,
}

impl Default for LookPrefs {
    fn default() -> Self {
        LookPrefs {
            lang: None,
            light: None,
            accent: ACCENTS[0].to_owned(),
            font_percent: 100,
            zone: None,
            h12: false,
        }
    }
}

impl Default for LockPrefs {
    fn default() -> Self {
        LockPrefs {
            list_on_login: true,
            auto_login: String::new(),
            parental: false,
            from_min: 7 * 60,
            until_min: 21 * 60,
            emergency_on: false,
            contact_name: String::new(),
            contact_phone: String::new(),
            medical: String::new(),
            number: "112".into(),
            sos_light: true,
            look: LookPrefs::default(),
        }
    }
}

impl LockPrefs {
    /// Takes a setting that changed; `true` when it was one of these and differs from
    /// what is known. A value of the wrong type or outside its range changes nothing.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = self.clone();
        let text = |v: &SettingValue| match v {
            SettingValue::Text(t) => Some(t.clone()),
            _ => None,
        };
        match (key, value) {
            (LIST_ON_LOGIN, SettingValue::Flag(on)) => self.list_on_login = *on,
            (AUTO_LOGIN, SettingValue::Text(name)) => {
                let name = name.trim();
                self.auto_login = if name.len() <= MAX_NAME {
                    name.to_owned()
                } else {
                    String::new()
                };
            }
            (PARENTAL, SettingValue::Flag(on)) => self.parental = *on,
            (PARENTAL_FROM, v) => {
                if let Some(m) = text(v).as_deref().and_then(minutes) {
                    self.from_min = m;
                }
            }
            (PARENTAL_UNTIL, v) => {
                if let Some(m) = text(v).as_deref().and_then(minutes) {
                    self.until_min = m;
                }
            }
            (EMERGENCY_ON, SettingValue::Flag(on)) => self.emergency_on = *on,
            (CONTACT_NAME, v) => {
                if let Some(t) = text(v) {
                    self.contact_name = t.chars().take(64).collect();
                }
            }
            (CONTACT_PHONE, v) => {
                if let Some(t) = text(v) {
                    self.contact_phone = t.chars().take(32).collect();
                }
            }
            (MEDICAL, v) => {
                if let Some(t) = text(v) {
                    self.medical = t.chars().take(500).collect();
                }
            }
            (NUMBER, v) => {
                if let Some(t) = text(v) {
                    self.number = if valid_number(&t) { t } else { "112".into() };
                }
            }
            (SOS_LIGHT, SettingValue::Flag(on)) => self.sos_light = *on,
            (key, value) => {
                self.look.on_changed(key, value);
            }
        }
        *self != before
    }

    /// Offset of the local time at `unix_ms`: the zone of the settings, else `fallback`.
    pub fn offset_s(&self, unix_ms: u64, fallback: i32) -> i32 {
        self.look.offset_s(unix_ms, fallback)
    }

    /// Whether this user may sign in at `min` minutes since midnight: administrators
    /// always, standard users only in the window while the restriction is on.
    pub fn allows(&self, min: u16, admin: bool) -> bool {
        !self.parental || admin || window_allows(self.from_min, self.until_min, min)
    }
}

impl LookPrefs {
    /// Takes a setting of the look; a value of the wrong type or unknown word changes
    /// nothing (an unknown accent is the standard gold).
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) {
        match (key, value) {
            (LANGUAGE, SettingValue::Text(t)) => {
                if let Some(lang) = ultar_i18n::Lang::parse(t) {
                    self.lang = Some(lang);
                }
            }
            // "auto" follows the dark standard until the time of day is known here.
            (THEME, SettingValue::Text(t)) => self.light = Some(t == "light"),
            (ACCENT, SettingValue::Text(t)) => {
                let word = t.trim();
                self.accent = if ACCENTS.contains(&word) {
                    word.to_owned()
                } else {
                    ACCENTS[0].to_owned()
                };
            }
            (FONT_SIZE, SettingValue::Integer(v)) => {
                let (lo, hi) = FONT_RANGE;
                self.font_percent =
                    u16::try_from((*v).clamp(i64::from(lo), i64::from(hi))).unwrap_or(100);
            }
            (ZONE, SettingValue::Text(t)) => {
                self.zone = ultar_i18n::tz::valid(t).then(|| t.clone());
            }
            (CLOCK_FORMAT, SettingValue::Text(t)) => self.h12 = t == "h12",
            _ => {}
        }
    }

    /// Offset of the local time at `unix_ms`: the zone of the settings, else `fallback`.
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
    fn defaults_are_the_schema_defaults() {
        let p = LockPrefs::default();
        assert!(p.list_on_login && !p.parental && !p.emergency_on && p.sos_light);
        assert_eq!((p.from_min, p.until_min), (420, 1260));
        assert_eq!(p.number, "112");
        assert!(p.auto_login.is_empty() && p.contact_name.is_empty());
    }

    #[test]
    fn language_theme_and_clock_are_followed() {
        let mut p = LockPrefs::default();
        let text = |t: &str| SettingValue::Text(t.into());
        assert!(p.on_changed(LANGUAGE, &text("de")));
        assert_eq!(p.look.lang, Some(ultar_i18n::Lang::De));
        assert!(!p.on_changed(LANGUAGE, &text("klingon")));
        assert!(p.on_changed(THEME, &text("light")));
        assert_eq!(p.look.light, Some(true));
        assert!(p.on_changed(ACCENT, &text("blue")));
        assert!(p.on_changed(CLOCK_FORMAT, &text("h12")) && p.look.h12);
        assert!(p.on_changed(ZONE, &text("Asia/Damascus")));
        assert_eq!(p.offset_s(1_795_636_560_000, 0), 10_800);
        assert!(p.on_changed(FONT_SIZE, &SettingValue::Integer(130)));
        assert_eq!(p.look.font_percent, 130);
    }

    #[test]
    fn good_values_take_effect_and_bad_ones_keep_the_default() {
        let mut p = LockPrefs::default();
        assert!(p.on_changed(LIST_ON_LOGIN, &SettingValue::Flag(false)));
        assert!(!p.list_on_login);
        assert!(p.on_changed(PARENTAL_FROM, &SettingValue::Text("22:30".into())));
        assert_eq!(p.from_min, 22 * 60 + 30);
        // Wrong type, wrong format, out of range: nothing changes.
        for bad in [
            SettingValue::Flag(true),
            SettingValue::Text("25:00".into()),
            SettingValue::Text("12:60".into()),
            SettingValue::Text("noon".into()),
            SettingValue::Integer(420),
        ] {
            assert!(!p.on_changed(PARENTAL_FROM, &bad), "{bad:?}");
        }
        assert_eq!(p.from_min, 22 * 60 + 30);
        assert!(p.on_changed(NUMBER, &SettingValue::Text("911".into())));
        assert_eq!(p.number, "911");
        assert!(!p.on_changed(NUMBER, &SettingValue::Text("911".into())));
        // An invalid number falls back to the schema default (112), never to an
        // arbitrary one; the wrong type keeps what is known.
        for bad in ["", "12", "12345678901234567", "11a", "+", "1 2"] {
            p.on_changed(NUMBER, &SettingValue::Text(bad.into()));
            assert_eq!(p.number, "112", "{bad}");
        }
        assert!(!p.on_changed(NUMBER, &SettingValue::Flag(true)));
        assert_eq!(p.number, "112");
        assert!(p.on_changed(NUMBER, &SettingValue::Text("+49110".into())));
        assert_eq!(p.number, "+49110");
        // An over-long auto-login name falls back to nobody.
        assert!(p.on_changed(AUTO_LOGIN, &SettingValue::Text("anas".into())));
        assert!(p.on_changed(AUTO_LOGIN, &SettingValue::Text("x".repeat(33))));
        assert!(p.auto_login.is_empty());
        assert!(p.on_changed(AUTO_LOGIN, &SettingValue::Text(" anas ".into())));
        assert_eq!(p.auto_login, "anas");
        // A key of another panel changes nothing.
        assert!(!p.on_changed("display.scale", &SettingValue::Decimal(2.0)));
    }

    #[test]
    fn runtime_changes_arrive_one_by_one() {
        let mut p = LockPrefs::default();
        assert!(!p.on_changed(EMERGENCY_ON, &SettingValue::Flag(false)));
        assert!(p.on_changed(EMERGENCY_ON, &SettingValue::Flag(true)));
        assert!(p.on_changed(CONTACT_NAME, &SettingValue::Text("Layla".into())));
        assert!(p.on_changed(MEDICAL, &SettingValue::Text("Blood type A+".into())));
        assert!(!p.on_changed(MEDICAL, &SettingValue::Text("Blood type A+".into())));
        assert_eq!(p.contact_name, "Layla");
        assert!(p.emergency_on);
    }

    #[test]
    fn the_window_includes_its_start_not_its_end_and_may_run_over_midnight() {
        assert!(window_allows(420, 1260, 420));
        assert!(window_allows(420, 1260, 1259));
        assert!(!window_allows(420, 1260, 1260));
        assert!(!window_allows(420, 1260, 419));
        // Over midnight: 22:00–06:00.
        assert!(window_allows(1320, 360, 1380));
        assert!(window_allows(1320, 360, 60));
        assert!(!window_allows(1320, 360, 720));
        // Equal ends: the whole day.
        assert!(window_allows(600, 600, 0));
        assert!(window_allows(600, 600, 1439));
        let mut p = LockPrefs {
            parental: true,
            ..LockPrefs::default()
        };
        assert!(p.allows(720, false));
        assert!(!p.allows(1300, false));
        assert!(p.allows(1300, true), "administrators are not restricted");
        p.parental = false;
        assert!(p.allows(1300, false));
    }

    #[test]
    fn minutes_and_clock_text_round_trip() {
        assert_eq!(minutes("07:00"), Some(420));
        assert_eq!(minutes("00:00"), Some(0));
        assert_eq!(minutes("23:59"), Some(1439));
        assert_eq!(minutes("7:5"), Some(425));
        assert_eq!(minutes("24:00"), None);
        assert_eq!(minutes("07:00:00"), None);
        assert_eq!(clock_text(420), "07:00");
        assert_eq!(clock_text(1439), "23:59");
        assert!(valid_number("112") && valid_number("+49110") && !valid_number("11"));
    }
}
