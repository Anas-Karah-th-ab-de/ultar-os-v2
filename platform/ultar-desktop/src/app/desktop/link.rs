//! The desktop's bus connection: ports and the topics it follows (settings, notifications,
//! power, device capabilities). Without a bus every service part stays disabled.

use crate::model::quick::{PowerMode, RemovableMedia};
use crate::model::Desktop;
use crate::ports::bus::BusPorts;
use crate::ports::SettingsPort;
use std::path::Path;
use ultar_bus::messages::devices::{Availability, Capabilities};
use ultar_bus::messages::notify::Posted;
use ultar_bus::messages::power::PowerState;
use ultar_bus::messages::settings::{Changed, SettingValue};
use ultar_bus::{Client, ClientOptions, TypedSubscription};
use ultar_i18n::datefmt::ClockStyle;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{MAX_DOUBLE_CLICK_MS, MIN_DOUBLE_CLICK_MS};

/// The setting that puts the bar at the bottom (standard) or at the top with a dock.
/// (`bar.layout` of the settings schema: `bottom_bar` | `top_bar_dock`, panel "Desktop bar").
pub const BAR_POSITION: &str = "bar.layout";

/// Settings the bar follows.
pub const FOLLOWED: [&str; 36] = [
    "region.language",
    "appearance.theme",
    "appearance.accent_color",
    "appearance.font_size",
    "datetime.clock_format",
    "datetime.timezone.name",
    "bar.clock.show_seconds",
    "bar.clock.show_weekday",
    "bar.clock.show_date",
    "bar.clock.show_hijri",
    "bar.icon_alignment",
    "bar.show_trash",
    "bar.show_volumes",
    "power.show_battery_percent",
    "accessibility.high_contrast",
    "accessibility.large_text",
    "accessibility.reduce_motion",
    "accessibility.double_click_ms",
    "accessibility.visual_alert",
    "accessibility.show_in_bar",
    "search.source.apps",
    "search.source.store",
    "search.source.settings",
    "search.source.calculator",
    "search.source.commands",
    "search.source.characters",
    "search.history",
    "search.arabic_variants",
    "notifications.do_not_disturb",
    "display.night_light.enabled",
    "power.mode",
    "apps.removable_media",
    "desktop.icons.enabled",
    "desktop.icons.size",
    "desktop.icons.start_corner",
    "desktop.metrics.enabled",
];

/// Ports and subscriptions.
pub struct Link {
    /// The ports.
    pub ports: BusPorts,
    changed: Option<TypedSubscription<Changed>>,
    posted: Option<TypedSubscription<Posted>>,
    power: Option<TypedSubscription<PowerState>>,
    caps: Option<TypedSubscription<Capabilities>>,
    services: super::topics::ServiceTopics,
}

impl Link {
    /// Connects as `name` (the socket decides the identity).
    pub fn connect(path: &Path, name: &str) -> Option<Link> {
        let client = Client::connect(path, ClientOptions::named(name)).ok()?;
        Some(Link {
            changed: client.subscribe::<Changed>().ok(),
            posted: client.subscribe::<Posted>().ok(),
            power: client.subscribe::<PowerState>().ok(),
            caps: client.subscribe::<Capabilities>().ok(),
            services: super::topics::ServiceTopics::subscribe(&client),
            ports: BusPorts::new(client),
        })
    }

    /// Reads the followed settings once (and learns whether the service answers).
    pub fn read_settings(&self, d: &mut Desktop) {
        d.presence.settings = false;
        for key in FOLLOWED {
            if let Ok(value) = self.ports.get(key) {
                d.presence.settings = true;
                apply_setting(d, key, &value);
            }
        }
    }

    /// Where the setting puts the bar (`None`: not set, or no answer).
    pub fn bar_place(&self) -> Option<crate::model::BarPlace> {
        match self.ports.get(BAR_POSITION) {
            Ok(SettingValue::Text(word)) => crate::model::BarPlace::from_setting(&word),
            _ => None,
        }
    }

    /// Takes what arrived; returns the changed setting keys, whether anything changed and
    /// whether the calendar's day must be asked for again. `now` is when the notifications
    /// arrive (their banners start then).
    pub fn poll(&self, d: &mut Desktop, now: u64) -> (Vec<String>, bool, bool) {
        let mut keys = Vec::new();
        let (mut any, calendar) = self.services.poll(d, now);
        while let Some(Ok(Some(c))) = self.changed.as_ref().map(|s| s.try_recv()) {
            d.presence.settings = true;
            apply_setting(d, &c.key, &c.value);
            keys.push(c.key);
            any = true;
        }
        while let Some(Ok(Some(p))) = self.posted.as_ref().map(|s| s.try_recv()) {
            let id = p.id;
            d.notes.add(p);
            // A banner unless "do not disturb" is on or the notifications are open anyway.
            if !d.values.dnd && d.open != crate::model::Open::Calendar {
                d.notes.show_banner(id, now);
            }
            any = true;
        }
        while let Some(Ok(Some(p))) = self.power.as_ref().map(|s| s.try_recv()) {
            d.values.battery = p.battery_percent.map(|b| (b.min(100), p.charging));
            any = true;
        }
        while let Some(Ok(Some(c))) = self.caps.as_ref().map(|s| s.try_recv()) {
            let present = |id: &str| {
                c.get(id)
                    .map(|cap| cap.availability != Availability::Unavailable)
            };
            d.presence.wifi = present("wifi");
            d.presence.bluetooth = present("bluetooth");
            any = true;
        }
        (keys, any, calendar)
    }
}

/// Puts a setting's value into the state.
pub fn apply_setting(d: &mut Desktop, key: &str, value: &SettingValue) {
    if apply_pref(d, key, value) {
        return;
    }
    match (key, value) {
        ("appearance.theme", SettingValue::Text(t)) => {
            d.values.dark = t != "light";
            d.theme = if d.values.dark {
                ThemeKind::Dark
            } else {
                ThemeKind::Light
            };
        }
        ("notifications.do_not_disturb", SettingValue::Flag(on)) => d.values.dnd = *on,
        ("accessibility.high_contrast", SettingValue::Flag(on)) => d.values.high_contrast = *on,
        ("accessibility.large_text", SettingValue::Flag(on)) => d.values.large_text = *on,
        ("accessibility.reduce_motion", SettingValue::Flag(on)) => d.values.reduce_motion = *on,
        ("accessibility.visual_alert", SettingValue::Flag(on)) => d.notes.visual_alert = *on,
        ("accessibility.show_in_bar", SettingValue::Flag(on)) => d.values.show_in_bar = *on,
        // Off keeps nothing: the remembered entries go (the file is deleted by the app).
        ("search.history", SettingValue::Flag(on)) => {
            d.overview.sources.history = *on;
            if !on {
                d.overview.history.clear();
            }
        }
        ("accessibility.double_click_ms", SettingValue::Integer(v))
            if (i64::from(MIN_DOUBLE_CLICK_MS)..=i64::from(MAX_DOUBLE_CLICK_MS)).contains(v) =>
        {
            d.values.double_click_ms = Some(*v as u16);
        }
        (k, SettingValue::Flag(on)) if d.overview.sources.set(k, *on) => {}
        ("display.night_light.enabled", SettingValue::Flag(on)) => d.values.night = *on,
        ("power.mode", SettingValue::Text(t)) => {
            if let Some(m) = PowerMode::parse(t) {
                d.values.power = m;
            }
        }
        ("apps.removable_media", SettingValue::Text(t)) => {
            d.values.removable_media = RemovableMedia::parse(t);
        }
        (k, SettingValue::Integer(v)) if Some(k) == brightness_key(d).as_deref() => {
            d.values.brightness = Some((*v).clamp(0, 100) as u8);
        }
        _ => {}
    }
}

/// The language, the look and the bar's clock and icons (`model::prefs`); `true` when the
/// key was one of them.
fn apply_pref(d: &mut Desktop, key: &str, value: &SettingValue) -> bool {
    let p = &mut d.prefs;
    match (key, value) {
        ("region.language", SettingValue::Text(t)) => {
            if let Some(lang) = ultar_i18n::Lang::parse(t) {
                d.lang = lang;
            }
        }
        ("appearance.accent_color", SettingValue::Text(t)) => {
            p.accent = ultar_ui::tokens::Accent::parse(t).unwrap_or_default();
        }
        ("appearance.font_size", SettingValue::Integer(v)) => {
            let (lo, hi) = crate::model::prefs::FONT_RANGE;
            p.font_percent = (*v).clamp(i64::from(lo), i64::from(hi)) as u16;
        }
        ("datetime.clock_format", SettingValue::Text(t)) => {
            p.clock.style = if t == "h12" {
                ClockStyle::H12
            } else {
                ClockStyle::H24
            };
        }
        ("datetime.timezone.name", SettingValue::Text(t)) => {
            p.zone = ultar_i18n::tz::valid(t).then(|| t.clone());
        }
        ("bar.clock.show_seconds", SettingValue::Flag(on)) => p.clock.seconds = *on,
        ("bar.clock.show_weekday", SettingValue::Flag(on)) => p.clock.weekday = *on,
        ("bar.clock.show_date", SettingValue::Flag(on)) => p.clock.date = *on,
        ("bar.clock.show_hijri", SettingValue::Flag(on)) => p.clock.hijri = *on,
        ("bar.icon_alignment", SettingValue::Text(t)) => p.icons_start = t == "start",
        ("bar.show_trash", SettingValue::Flag(on)) => p.show_trash = *on,
        ("bar.show_volumes", SettingValue::Flag(on)) => p.show_volumes = *on,
        ("power.show_battery_percent", SettingValue::Flag(on)) => p.battery_percent = *on,
        (k, v) if k.starts_with("desktop.") => return p.desk.apply(k, v),
        _ => return false,
    }
    true
}

/// `display.<id>.brightness` of the main display.
pub fn brightness_key(d: &Desktop) -> Option<String> {
    d.svc
        .main_display
        .as_ref()
        .map(|id| format!("display.{id}.brightness"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    #[test]
    fn language_look_and_clock_reach_the_state() {
        let mut d = Desktop::new(Lang::Ar);
        let text = |t: &str| SettingValue::Text(t.into());
        apply_setting(&mut d, "region.language", &text("de"));
        assert_eq!(d.lang, Lang::De);
        apply_setting(&mut d, "region.language", &text("xx"));
        assert_eq!(d.lang, Lang::De, "an unknown language changes nothing");
        apply_setting(&mut d, "appearance.accent_color", &text("teal"));
        assert_eq!(d.prefs.accent, ultar_ui::tokens::Accent::Teal);
        apply_setting(&mut d, "appearance.font_size", &SettingValue::Integer(150));
        assert_eq!(d.prefs.font_percent, 150);
        apply_setting(&mut d, "datetime.clock_format", &text("h12"));
        assert_eq!(d.prefs.clock.style, ClockStyle::H12);
        apply_setting(&mut d, "datetime.timezone.name", &text("Europe/Berlin"));
        assert_eq!(d.prefs.zone.as_deref(), Some("Europe/Berlin"));
        apply_setting(&mut d, "datetime.timezone.name", &text("../etc"));
        assert_eq!(d.prefs.zone, None, "a bad zone name is not used");
        for (key, on) in [
            ("bar.clock.show_seconds", true),
            ("bar.clock.show_hijri", true),
            ("bar.show_trash", false),
            ("power.show_battery_percent", true),
        ] {
            apply_setting(&mut d, key, &SettingValue::Flag(on));
        }
        apply_setting(&mut d, "bar.icon_alignment", &text("start"));
        let p = &d.prefs;
        assert!(p.clock.seconds && p.clock.hijri && !p.show_trash && p.battery_percent);
        assert!(p.icons_start);
        // every followed key is a key of the schema's shape (area.name…)
        assert!(FOLLOWED.iter().all(|k| k.contains('.')));
    }

    #[test]
    fn settings_reach_the_state() {
        let mut d = Desktop::new(Lang::Ar);
        apply_setting(
            &mut d,
            "appearance.theme",
            &SettingValue::Text("light".into()),
        );
        assert_eq!(d.theme, ThemeKind::Light);
        apply_setting(
            &mut d,
            "notifications.do_not_disturb",
            &SettingValue::Flag(true),
        );
        apply_setting(
            &mut d,
            "display.night_light.enabled",
            &SettingValue::Flag(true),
        );
        apply_setting(&mut d, "power.mode", &SettingValue::Text("saver".into()));
        assert!(d.values.dnd && d.values.night && d.values.power == PowerMode::Saver);
        d.svc.main_display = Some("edp-1".into());
        apply_setting(
            &mut d,
            "display.edp-1.brightness",
            &SettingValue::Integer(140),
        );
        assert_eq!(d.values.brightness, Some(100));
        apply_setting(&mut d, "x", &SettingValue::Flag(true));
    }

    #[test]
    fn switching_history_off_clears_the_remembered_entries() {
        use crate::model::history::Record;
        let mut d = Desktop::new(Lang::En);
        d.overview.history.record(Record::Search("weather".into()));
        assert!(d.overview.sources.history && !d.overview.history.entries.is_empty());
        apply_setting(&mut d, "search.history", &SettingValue::Flag(false));
        assert!(!d.overview.sources.history);
        assert!(d.overview.history.entries.is_empty(), "off keeps nothing");
        // On again: storing resumes (the list starts empty).
        apply_setting(&mut d, "search.history", &SettingValue::Flag(true));
        assert!(d.overview.sources.history);
        d.overview
            .history
            .record(Record::App("org.ultar.files".into()));
        assert_eq!(d.overview.history.entries.len(), 1);
    }

    #[test]
    fn the_double_click_time_reaches_the_state_within_its_range() {
        let mut d = Desktop::new(Lang::En);
        assert_eq!(d.values.double_click_ms, None, "not read yet");
        assert!(FOLLOWED.contains(&"accessibility.double_click_ms"));
        apply_setting(
            &mut d,
            "accessibility.double_click_ms",
            &SettingValue::Integer(700),
        );
        assert_eq!(d.values.double_click_ms, Some(700));
        // Outside the range or of the wrong type: the last good value stays.
        apply_setting(
            &mut d,
            "accessibility.double_click_ms",
            &SettingValue::Integer(5000),
        );
        apply_setting(
            &mut d,
            "accessibility.double_click_ms",
            &SettingValue::Flag(true),
        );
        assert_eq!(d.values.double_click_ms, Some(700));
    }

    #[test]
    fn the_removable_media_setting_reaches_the_state() {
        use crate::model::quick::RemovableMedia;
        let mut d = Desktop::new(Lang::En);
        assert!(FOLLOWED.contains(&"apps.removable_media"));
        assert_eq!(d.values.removable_media, RemovableMedia::Ask);
        apply_setting(
            &mut d,
            "apps.removable_media",
            &SettingValue::Text("open_files".into()),
        );
        assert_eq!(d.values.removable_media, RemovableMedia::OpenFiles);
        apply_setting(
            &mut d,
            "apps.removable_media",
            &SettingValue::Text("nothing".into()),
        );
        assert_eq!(d.values.removable_media, RemovableMedia::Nothing);
        // an unknown word keeps the default (ask)
        apply_setting(
            &mut d,
            "apps.removable_media",
            &SettingValue::Text("nonsense".into()),
        );
        assert_eq!(d.values.removable_media, RemovableMedia::Ask);
    }

    #[test]
    fn the_desktop_icon_settings_reach_the_state() {
        use crate::model::prefs::{DeskPrefs, IconSize};
        let mut d = Desktop::new(Lang::En);
        for key in DeskPrefs::KEYS {
            assert!(FOLLOWED.contains(&key), "{key} is not read at start");
        }
        apply_setting(
            &mut d,
            "desktop.icons.size",
            &SettingValue::Text("small".into()),
        );
        apply_setting(&mut d, "desktop.icons.enabled", &SettingValue::Flag(false));
        assert_eq!(d.prefs.desk.size, IconSize::Small);
        assert!(!d.prefs.desk.shown);
    }

    #[test]
    fn the_search_and_motion_settings_reach_the_state() {
        let mut d = Desktop::new(Lang::En);
        assert!(d.overview.sources.apps && !d.values.reduce_motion);
        for key in crate::model::search::Sources::KEYS {
            assert!(FOLLOWED.contains(&key), "{key} is not read at start");
        }
        apply_setting(&mut d, "search.source.apps", &SettingValue::Flag(false));
        apply_setting(&mut d, "search.arabic_variants", &SettingValue::Flag(false));
        apply_setting(
            &mut d,
            "accessibility.reduce_motion",
            &SettingValue::Flag(true),
        );
        let sources = d.overview.sources;
        assert!(!sources.apps && !sources.arabic_variants && sources.settings);
        assert!(d.values.reduce_motion);
    }

    #[test]
    fn the_store_source_switch_reaches_the_state() {
        let mut d = Desktop::new(Lang::En);
        assert!(
            d.overview.sources.store,
            "on until the settings say otherwise"
        );
        assert!(FOLLOWED.contains(&"search.source.store"));
        apply_setting(&mut d, "search.source.store", &SettingValue::Flag(false));
        assert!(!d.overview.sources.store);
        apply_setting(&mut d, "search.source.store", &SettingValue::Flag(true));
        assert!(d.overview.sources.store);
        // A value of the wrong type changes nothing (the last good value stays).
        apply_setting(&mut d, "search.source.store", &SettingValue::Integer(1));
        assert!(d.overview.sources.store);
    }

    #[test]
    fn the_visual_alert_and_bar_button_settings_reach_the_state() {
        let mut d = Desktop::new(Lang::En);
        assert!(FOLLOWED.contains(&"accessibility.visual_alert"));
        assert!(FOLLOWED.contains(&"accessibility.show_in_bar"));
        assert!(!d.notes.visual_alert && !d.values.show_in_bar);
        apply_setting(
            &mut d,
            "accessibility.visual_alert",
            &SettingValue::Flag(true),
        );
        apply_setting(
            &mut d,
            "accessibility.show_in_bar",
            &SettingValue::Flag(true),
        );
        assert!(d.notes.visual_alert && d.values.show_in_bar);
        // A wrong type changes nothing; off turns them off again.
        apply_setting(
            &mut d,
            "accessibility.visual_alert",
            &SettingValue::Integer(1),
        );
        assert!(d.notes.visual_alert);
        apply_setting(
            &mut d,
            "accessibility.visual_alert",
            &SettingValue::Flag(false),
        );
        apply_setting(
            &mut d,
            "accessibility.show_in_bar",
            &SettingValue::Flag(false),
        );
        assert!(!d.notes.visual_alert && !d.values.show_in_bar);
    }
}
