//! The desktop's bus connection: ports and the topics it follows (settings, notifications,
//! power, device capabilities). Without a bus every service part stays disabled.

use crate::model::quick::PowerMode;
use crate::model::Desktop;
use crate::ports::bus::BusPorts;
use crate::ports::SettingsPort;
use std::path::Path;
use ultar_bus::messages::devices::{Availability, Capabilities};
use ultar_bus::messages::notify::Posted;
use ultar_bus::messages::power::PowerState;
use ultar_bus::messages::settings::{Changed, SettingValue};
use ultar_bus::{Client, ClientOptions, TypedSubscription};
use ultar_ui::tokens::ThemeKind;

/// The setting that puts the bar at the bottom (standard) or at the top with a dock.
/// (`bar.layout` of the settings schema: `bottom_bar` | `top_bar_dock`, panel "Desktop bar").
pub const BAR_POSITION: &str = "bar.layout";

/// Settings the bar follows.
pub const FOLLOWED: [&str; 4] = [
    "appearance.theme",
    "notifications.do_not_disturb",
    "display.night_light.enabled",
    "power.mode",
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
        let (mut any, calendar) = self.services.poll(d);
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
        ("display.night_light.enabled", SettingValue::Flag(on)) => d.values.night = *on,
        ("power.mode", SettingValue::Text(t)) => {
            if let Some(m) = PowerMode::parse(t) {
                d.values.power = m;
            }
        }
        (k, SettingValue::Integer(v)) if Some(k) == brightness_key(d).as_deref() => {
            d.values.brightness = Some((*v).clamp(0, 100) as u8);
        }
        _ => {}
    }
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
}
