//! The `updates.*` settings of `settingsd` that the update service follows, and the
//! administrator's ceiling from `/etc/ultar/update.conf` that bounds them.
//!
//! Settings are the one source the person sees (`docs/integration/settings-wiring.md`): the
//! service reads them at start and on `settings.changed` and maps them onto its [`Policy`].
//! The configuration file stays the administrator's ceiling – a setting cannot allow what the
//! file forbids (`allow_automatic`, `allow_metered`, `channels`). Values that do not parse are
//! ignored (the last good value stays; `settingsd` validates what it stores).
//!
//! `updates.auto_apps` is deliberately not here: app updates belong to the launcher and the
//! Store, not to the system updater (its bundles carry only kernel/system/boot roles).

use crate::config::{ConfigError, ConfigErrorKind};
use crate::manifest::valid_ident;
use crate::policy::Policy;
use ultar_bus::messages::settings::SettingValue;

/// `updates.auto_system` – download and install system updates by itself.
pub const AUTO_SYSTEM: &str = "updates.auto_system";
/// `updates.window.from` – start of the daily install window (`HH:MM`, UTC).
pub const WINDOW_FROM: &str = "updates.window.from";
/// `updates.window.until` – end of the daily install window (`HH:MM`, UTC).
pub const WINDOW_UNTIL: &str = "updates.window.until";
/// `updates.channel` – the update channel.
pub const CHANNEL: &str = "updates.channel";
/// `updates.metered` – automatic downloads may use a metered connection.
pub const METERED: &str = "updates.metered";
/// `updates.offline_usb` – offer updates from a drive when one is plugged in.
pub const OFFLINE_USB: &str = "updates.offline_usb";
/// `updates.notify` – notify about updates through `notifyd`.
pub const NOTIFY: &str = "updates.notify";

/// Every setting the service follows (read at start and on `settings.changed`).
pub const KEYS: [&str; 7] = [
    AUTO_SYSTEM,
    WINDOW_FROM,
    WINDOW_UNTIL,
    CHANNEL,
    METERED,
    OFFLINE_USB,
    NOTIFY,
];

/// The channels the Updates page offers (as `settingsd` seeds them).
pub const CHANNELS: [&str; 2] = ["stable", "preview"];

/// Most channels the administrator's ceiling may name.
pub const MAX_CHANNELS: usize = 8;

/// The update settings as the person set them. Defaults match the `settingsd` seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSettings {
    /// `updates.auto_system`.
    pub auto_system: bool,
    /// `updates.window.from` as minutes of the day (UTC).
    pub window_from: u16,
    /// `updates.window.until` as minutes of the day (UTC).
    pub window_until: u16,
    /// `updates.channel`.
    pub channel: String,
    /// `updates.metered`.
    pub metered: bool,
    /// `updates.offline_usb`.
    pub offline_usb: bool,
    /// `updates.notify`.
    pub notify: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_system: true,
            window_from: 2 * 60,
            window_until: 5 * 60,
            channel: "stable".to_string(),
            metered: false,
            offline_usb: true,
            notify: true,
        }
    }
}

/// `"HH:MM"` (24 hours, two digits each) → minutes of the day; `None` when it does not parse.
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

/// True when a change of `key` changes the policy (the others only steer notifying and the
/// offer of offline bundles).
pub fn policy_relevant(key: &str) -> bool {
    matches!(
        key,
        AUTO_SYSTEM | WINDOW_FROM | WINDOW_UNTIL | CHANNEL | METERED
    )
}

impl UpdateSettings {
    /// Takes one setting; true when it belongs to the updater and parses. A value of the wrong
    /// type or out of range is refused (the last good value stays).
    pub fn take(&mut self, key: &str, value: &SettingValue) -> bool {
        fn flag(value: &SettingValue) -> Option<bool> {
            match value {
                SettingValue::Flag(on) => Some(*on),
                _ => None,
            }
        }
        fn text(value: &SettingValue) -> Option<&str> {
            match value {
                SettingValue::Text(t) => Some(t.as_str()),
                _ => None,
            }
        }
        match key {
            AUTO_SYSTEM => flag(value).map(|v| self.auto_system = v).is_some(),
            WINDOW_FROM => text(value)
                .and_then(parse_time)
                .map(|v| self.window_from = v)
                .is_some(),
            WINDOW_UNTIL => text(value)
                .and_then(parse_time)
                .map(|v| self.window_until = v)
                .is_some(),
            CHANNEL => text(value)
                .filter(|t| CHANNELS.contains(t))
                .map(|t| self.channel = t.to_string())
                .is_some(),
            METERED => flag(value).map(|v| self.metered = v).is_some(),
            OFFLINE_USB => flag(value).map(|v| self.offline_usb = v).is_some(),
            NOTIFY => flag(value).map(|v| self.notify = v).is_some(),
            _ => false,
        }
    }

    /// The policy these settings mean, over the stored `base` (whose rate limit is kept – it is
    /// not on the page) and under the administrator's `ceiling`.
    pub fn policy(&self, base: &Policy, ceiling: &Ceiling) -> Policy {
        Policy {
            automatic: self.auto_system && ceiling.allow_automatic,
            window: Some((self.window_from, self.window_until)),
            channel: ceiling.channel(&self.channel),
            allow_metered: self.metered && ceiling.allow_metered,
            rate_limit_kib: base.rate_limit_kib,
        }
    }
}

/// The administrator's ceiling from `/etc/ultar/update.conf`: what the settings may allow at
/// most. Absent keys leave the defaults (the file only restricts when it says so).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceiling {
    /// `allow_automatic=` – whether automatic system work may be switched on at all.
    pub allow_automatic: bool,
    /// `allow_metered=` – whether downloads over metered connections may be allowed.
    pub allow_metered: bool,
    /// `channels=` – the channels the device may use; the first is the fallback.
    pub channels: Vec<String>,
}

impl Default for Ceiling {
    fn default() -> Self {
        Self {
            allow_automatic: true,
            allow_metered: true,
            channels: CHANNELS.iter().map(|c| c.to_string()).collect(),
        }
    }
}

impl Ceiling {
    /// Parses the ceiling from the configuration file's `key=value` lookup. A bad value is a
    /// configuration error (the daemon refuses to start, as for every other key).
    pub fn parse<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Result<Self, ConfigError> {
        let value = || ConfigError {
            line: 0,
            kind: ConfigErrorKind::Value,
        };
        let flag = |key: &str, default: bool| match get(key) {
            None => Ok(default),
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            Some(_) => Err(value()),
        };
        let channels = match get("channels") {
            None => CHANNELS.iter().map(|c| c.to_string()).collect(),
            Some(text) => {
                let mut channels: Vec<String> = Vec::new();
                for name in text.split(',') {
                    let name = name.trim();
                    if !valid_ident(name) || channels.iter().any(|c| c == name) {
                        return Err(value());
                    }
                    channels.push(name.to_string());
                }
                if channels.is_empty() || channels.len() > MAX_CHANNELS {
                    return Err(value());
                }
                channels
            }
        };
        Ok(Self {
            allow_automatic: flag("allow_automatic", true)?,
            allow_metered: flag("allow_metered", true)?,
            channels,
        })
    }

    /// The channel to use: `wanted` when the ceiling allows it, else the first allowed one.
    pub fn channel(&self, wanted: &str) -> String {
        if self.channels.iter().any(|c| c == wanted) {
            wanted.to_string()
        } else {
            self.channels
                .first()
                .cloned()
                .unwrap_or_else(|| "stable".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag(on: bool) -> SettingValue {
        SettingValue::Flag(on)
    }
    fn text(t: &str) -> SettingValue {
        SettingValue::Text(t.into())
    }

    #[test]
    fn defaults_match_the_seed() {
        let s = UpdateSettings::default();
        assert!(s.auto_system && s.offline_usb && s.notify);
        assert!(!s.metered);
        assert_eq!((s.window_from, s.window_until), (120, 300));
        assert_eq!(s.channel, "stable");
    }

    #[test]
    fn good_values_are_taken_and_bad_ones_refused() {
        let mut s = UpdateSettings::default();
        assert!(s.take(AUTO_SYSTEM, &flag(false)));
        assert!(s.take(WINDOW_FROM, &text("23:30")));
        assert!(s.take(WINDOW_UNTIL, &text("04:15")));
        assert!(s.take(CHANNEL, &text("preview")));
        assert!(s.take(METERED, &flag(true)));
        assert!(s.take(OFFLINE_USB, &flag(false)));
        assert!(s.take(NOTIFY, &flag(false)));
        assert_eq!((s.window_from, s.window_until), (1410, 255));
        assert_eq!(s.channel, "preview");
        assert!(!s.auto_system && s.metered && !s.offline_usb && !s.notify);
        // Wrong types, out-of-range times, an unknown channel and a foreign key: refused,
        // the last good value stays.
        for (key, value) in [
            (AUTO_SYSTEM, text("yes")),
            (WINDOW_FROM, text("24:00")),
            (WINDOW_FROM, text("2:00")),
            (WINDOW_UNTIL, text("12:60")),
            (WINDOW_UNTIL, flag(true)),
            (CHANNEL, text("beta")),
            (CHANNEL, text("../x")),
            (METERED, SettingValue::Integer(1)),
            (NOTIFY, text("on")),
            ("updates.auto_apps", flag(false)),
            ("power.mode", text("x")),
        ] {
            assert!(!s.take(key, &value), "{key}");
        }
        assert!(!s.auto_system && s.metered);
        assert_eq!((s.window_from, s.window_until), (1410, 255));
        assert_eq!(s.channel, "preview");
    }

    #[test]
    fn times_parse_strictly() {
        assert_eq!(parse_time("00:00"), Some(0));
        assert_eq!(parse_time("02:00"), Some(120));
        assert_eq!(parse_time("23:59"), Some(1439));
        for bad in [
            "24:00", "23:60", "2:00", "02:0", "0200", "", "aa:bb", "-1:00",
        ] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_policy_maps_the_settings_and_keeps_the_rate() {
        let base = Policy {
            rate_limit_kib: 512,
            ..Policy::default()
        };
        let s = UpdateSettings {
            auto_system: false,
            window_from: 1380,
            window_until: 300,
            channel: "preview".into(),
            metered: true,
            ..UpdateSettings::default()
        };
        let p = s.policy(&base, &Ceiling::default());
        assert!(!p.automatic);
        assert_eq!(p.window, Some((1380, 300)));
        assert_eq!(p.channel, "preview");
        assert!(p.allow_metered);
        assert_eq!(p.rate_limit_kib, 512, "the rate limit is not on the page");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn the_ceiling_bounds_what_the_settings_allow() {
        let s = UpdateSettings {
            auto_system: true,
            metered: true,
            channel: "preview".into(),
            ..UpdateSettings::default()
        };
        let ceiling = Ceiling {
            allow_automatic: false,
            allow_metered: false,
            channels: vec!["stable".into()],
        };
        let p = s.policy(&Policy::default(), &ceiling);
        assert!(!p.automatic, "the file forbids automatic work");
        assert!(!p.allow_metered, "the file forbids metered downloads");
        assert_eq!(p.channel, "stable", "preview is not allowed: fallback");
        // An allowed channel passes through.
        let open = Ceiling::default();
        assert_eq!(s.policy(&Policy::default(), &open).channel, "preview");
    }

    #[test]
    fn the_ceiling_parses_from_the_file() {
        let c = Ceiling::parse(|_| None).unwrap();
        assert_eq!(c, Ceiling::default());
        let get = |key: &str| match key {
            "allow_automatic" => Some("0"),
            "allow_metered" => Some("0"),
            "channels" => Some("stable, preview"),
            _ => None,
        };
        let c = Ceiling::parse(get).unwrap();
        assert!(!c.allow_automatic && !c.allow_metered);
        assert_eq!(c.channels, ["stable", "preview"]);
        for bad in [
            "allow_automatic=2",
            "allow_metered=yes",
            "channels=",
            "channels=../x",
            "channels=stable,stable",
        ] {
            let (key, value) = bad.split_once('=').unwrap();
            let get = |k: &str| (k == key).then_some(value);
            assert!(Ceiling::parse(get).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_policy_keys_are_relevant() {
        for key in [AUTO_SYSTEM, WINDOW_FROM, WINDOW_UNTIL, CHANNEL, METERED] {
            assert!(policy_relevant(key), "{key}");
        }
        for key in [OFFLINE_USB, NOTIFY, "updates.auto_apps"] {
            assert!(!policy_relevant(key), "{key}");
        }
    }
}
