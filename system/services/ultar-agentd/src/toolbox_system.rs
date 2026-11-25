//! Tools that reach the rest of the system: every setting, the state of network, sound,
//! battery and storage, notifications, closing an app, signing out.
//!
//! Settings are the widest door, so it has the most rules: the helper never changes its
//! own switches, the settings service refuses what only an administrator may change, and
//! the person confirms a change in a sensitive panel in the system dialog first.

use crate::toolbox::{done, BusTools, BAD, CALL_TIME};
use crate::tools::Outcome;
use ultar_bus::messages::agent::{StepOutcome, OWN_SETTINGS};
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::launcher::{launcher_id, Role, Stop};
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::notify::{Dismiss, List as ListNotes};
use ultar_bus::messages::power::PowerState;
use ultar_bus::messages::session::{SessionOutcome, SignOut};
use ultar_bus::messages::settings::{
    ItemInfo, ItemKind, ListRequest, PanelInfo, RiskInfo, Set, SettingValue,
};
use ultar_bus::messages::volumes::{Eject, EjectOutcome, Usage};
use ultar_bus::{Message, Topic};

/// Panels whose settings the person confirms before the helper changes them.
pub const SENSITIVE_PANELS: [&str; 9] = [
    "privacy",
    "account",
    "sharing",
    "users",
    "updates",
    "backup",
    "mesh",
    "cells",
    "emergency",
];
/// Longest text a tool tells the model.
const MAX_TOLD: usize = 1500;

fn short(mut text: String) -> String {
    if text.len() > MAX_TOLD {
        let cut = (0..=MAX_TOLD).rev().find(|i| text.is_char_boundary(*i));
        text.truncate(cut.unwrap_or(0));
        text.push_str(" …");
    }
    text
}

fn shown(value: &SettingValue) -> String {
    match value {
        SettingValue::Flag(on) => on.to_string(),
        SettingValue::Integer(n) => n.to_string(),
        SettingValue::Decimal(n) => n.to_string(),
        SettingValue::Text(text) => text.clone(),
        SettingValue::List(list) => list.join(","),
    }
}

fn accepted(kind: &ItemKind) -> String {
    match kind {
        ItemKind::Toggle => "true|false".to_owned(),
        ItemKind::Integer { min, max, .. } => format!("{min}..{max}"),
        ItemKind::Decimal { min, max, .. } => format!("{min}..{max}"),
        ItemKind::Choice { choices } => {
            let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
            ids.join("|")
        }
        ItemKind::Text { format } => format!("text:{format}"),
        ItemKind::List { format, .. } => format!("list of {format}, comma separated"),
    }
}

/// `text` as a value of `kind`; `None` when it is none.
pub fn value_of(kind: &ItemKind, text: &str) -> Option<SettingValue> {
    match kind {
        ItemKind::Toggle => crate::toolbox::flag(text).map(SettingValue::Flag),
        ItemKind::Integer { min, max, .. } => {
            let n = text.parse::<i64>().ok()?;
            (*min..=*max)
                .contains(&n)
                .then_some(SettingValue::Integer(n))
        }
        ItemKind::Decimal { min, max, .. } => {
            let n = text.parse::<f64>().ok()?;
            (n >= *min && n <= *max).then_some(SettingValue::Decimal(n))
        }
        ItemKind::Choice { choices } => choices
            .iter()
            .any(|c| c.id == text)
            .then(|| SettingValue::Text(text.to_owned())),
        ItemKind::Text { .. } => Some(SettingValue::Text(text.to_owned())),
        ItemKind::List { .. } => {
            let list = text.split(',').map(|t| t.trim().to_owned());
            Some(SettingValue::List(list.filter(|t| !t.is_empty()).collect()))
        }
    }
}

/// Why the helper does not change `item` of `panel` at all (`None`: it may try).
pub fn closed(panel: &str, item: &ItemInfo) -> Option<&'static str> {
    if item.key.starts_with(OWN_SETTINGS) || panel == "assistant" {
        Some("the helper never changes its own settings; the person does that in Settings")
    } else if item.template {
        Some("this setting exists once per app; the person changes it in Settings")
    } else if !item.writable || item.risk == RiskInfo::AdminOnly {
        Some("only an administrator changes this setting, in Settings")
    } else {
        None
    }
}

impl BusTools<'_> {
    fn retained<T: Topic + Message>(&self) -> Option<T> {
        let state = self.client.subscribe::<T>().ok()?;
        state.recv_timeout(CALL_TIME).ok()
    }

    fn panels(&self) -> Option<Vec<PanelInfo>> {
        let list = self.client.call(&ListRequest { panel: None }, CALL_TIME);
        list.ok().map(|l| l.panels)
    }

    fn item(&self, key: &str) -> Option<(String, ItemInfo)> {
        let panels = self.panels()?;
        panels.into_iter().find_map(|p| {
            let item = p.items.into_iter().find(|i| i.key == key)?;
            Some((p.id, item))
        })
    }

    pub(crate) fn settings_panels(&self) -> Outcome {
        let Some(panels) = self.panels() else {
            return done(false, "the settings are not reachable");
        };
        let list: Vec<String> = panels
            .iter()
            .map(|p| format!("{} ({})", p.id, p.items.len()))
            .collect();
        done(true, &list.join(", "))
    }

    pub(crate) fn settings_list(&self, panel: &str) -> Outcome {
        let panels = self.panels().unwrap_or_default();
        let Some(panel) = panels.iter().find(|p| p.id == panel) else {
            return done(false, "no such panel; use settings_panels");
        };
        let lines: Vec<String> = panel
            .items
            .iter()
            .map(|i| format!("{} = {} [{}]", i.key, shown(&i.value), accepted(&i.kind)))
            .collect();
        done(true, &short(lines.join("; ")))
    }

    pub(crate) fn setting_get(&self, key: &str) -> Outcome {
        match self.item(key) {
            Some((_, item)) => done(
                true,
                &format!("{} [{}]", shown(&item.value), accepted(&item.kind)),
            ),
            None => done(false, "no such setting; use settings_list"),
        }
    }

    pub(crate) fn setting_set(&self, key: &str, value: &str) -> Outcome {
        let Some((panel, item)) = self.item(key) else {
            return done(false, "no such setting; use settings_list");
        };
        if let Some(why) = closed(&panel, &item) {
            return (StepOutcome::NotAllowed, why.to_owned());
        }
        let Some(value) = value_of(&item.kind, value) else {
            return done(false, &format!("accepted: {}", accepted(&item.kind)));
        };
        if SENSITIVE_PANELS.contains(&panel.as_str()) && !self.confirmed("setting_set") {
            return (
                StepOutcome::NotConfirmed,
                "the person did not confirm".to_owned(),
            );
        }
        let set = Set {
            key: key.to_owned(),
            value,
        };
        let waits = matches!(item.risk, RiskInfo::ConfirmRevert { .. });
        match self.client.call(&set, CALL_TIME) {
            Ok(_) if waits => done(true, "set; the system asks the person whether to keep it"),
            Ok(_) => done(true, ""),
            Err(_) => done(false, "the settings refused this value"),
        }
    }

    pub(crate) fn network_status(&self) -> Outcome {
        let Some(net) = self.retained::<NetState>() else {
            return done(false, "the network state is not reachable");
        };
        let bluetooth = self.retained::<BluetoothState>().unwrap_or_default();
        let told = format!(
            "wifi {}, network {}, signal {}, cable {}, bluetooth {} ({} connected), airplane {}",
            net.wifi_on,
            net.ssid.as_deref().unwrap_or("none"),
            net.signal_percent.unwrap_or(0),
            net.wired.is_some(),
            bluetooth.on,
            bluetooth.connected,
            net.airplane
        );
        done(true, &told)
    }

    pub(crate) fn sound_status(&self) -> Outcome {
        match self.retained::<AudioState>() {
            Some(a) if a.present => done(
                true,
                &format!(
                    "volume {}, muted {}, output {}",
                    a.volume_percent, a.muted, a.output_name
                ),
            ),
            Some(_) => done(true, "this device has no sound output"),
            None => done(false, "the sound state is not reachable"),
        }
    }

    pub(crate) fn battery(&self) -> Outcome {
        match self.retained::<PowerState>() {
            Some(p) => match p.battery_percent {
                Some(percent) => done(true, &format!("{percent} percent, charging {}", p.charging)),
                None => done(true, "this device has no battery"),
            },
            None => done(false, "the power state is not reachable"),
        }
    }

    pub(crate) fn notifications(&self) -> Outcome {
        let Ok(list) = self.client.call(&ListNotes {}, CALL_TIME) else {
            return done(false, "the notifications are not reachable");
        };
        let lines: Vec<String> = list
            .notes
            .iter()
            .map(|n| format!("{} from {}", n.id, n.sender))
            .collect();
        done(true, &short(lines.join("; ")))
    }

    pub(crate) fn dismiss_notification(&self, id: &str) -> Outcome {
        let Ok(id) = id.parse::<u64>() else {
            return done(false, BAD);
        };
        done(self.client.call(&Dismiss { id }, CALL_TIME).is_ok(), "")
    }

    pub(crate) fn storage(&self) -> Outcome {
        let Ok(usage) = self.client.call(&Usage {}, CALL_TIME) else {
            return done(false, "the storage is not reachable");
        };
        let lines: Vec<String> = usage
            .places
            .iter()
            .map(|s| {
                let gb = |bytes: u64| bytes / 1_000_000_000;
                format!(
                    "{} {}: {} of {} GB free",
                    s.id,
                    s.name,
                    gb(s.free_bytes),
                    gb(s.total_bytes)
                )
            })
            .collect();
        done(true, &lines.join("; "))
    }

    pub(crate) fn eject(&self, drive: &str) -> Outcome {
        let eject = Eject {
            id: drive.to_owned(),
        };
        match self.client.call(&eject, CALL_TIME).map(|r| r.outcome) {
            Ok(EjectOutcome::Done) => done(true, "it can be pulled out"),
            Ok(EjectOutcome::Busy) => done(false, "files on it are still open"),
            Ok(EjectOutcome::Unknown) => done(false, "no such drive; use storage"),
            _ => done(false, "the drive could not be unmounted"),
        }
    }

    pub(crate) fn close_app(&self, app: &str) -> Outcome {
        let stop = Stop {
            app: launcher_id(app).to_owned(),
            role: Role::Ui,
        };
        done(self.client.call(&stop, CALL_TIME).is_ok(), "")
    }

    pub(crate) fn sign_out(&self) -> Outcome {
        let reply = self.client.call(&SignOut {}, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == SessionOutcome::Done), "")
    }

    /// The tools of this file by name.
    pub(crate) fn run_system(&self, tool: &str, call: &crate::tools::Call) -> Option<Outcome> {
        Some(match tool {
            "settings_panels" => self.settings_panels(),
            "settings_list" => self.settings_list(call.arg("panel")),
            "setting_get" => self.setting_get(call.arg("key")),
            "setting_set" => self.setting_set(call.arg("key"), call.arg("value")),
            "network_status" => self.network_status(),
            "sound_status" => self.sound_status(),
            "battery" => self.battery(),
            "notifications" => self.notifications(),
            "dismiss_notification" => self.dismiss_notification(call.arg("id")),
            "storage" => self.storage(),
            "eject" => self.eject(call.arg("drive")),
            "close_app" => self.close_app(call.arg("app")),
            "sign_out" => self.sign_out(),
            "what_can_you_do" => done(true, &self.reach),
            "what_did_you_do" => done(true, &self.lately),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::settings::{ChoiceInfo, ScopeInfo};
    use ultar_bus::Text;

    fn item(key: &str, kind: ItemKind, risk: RiskInfo) -> ItemInfo {
        ItemInfo {
            key: key.to_owned(),
            label: Text::default(),
            description: Text::default(),
            kind,
            value: SettingValue::Flag(false),
            default: SettingValue::Flag(false),
            risk,
            scope: ScopeInfo::System,
            writable: risk != RiskInfo::AdminOnly,
            pending: false,
            template: false,
        }
    }

    #[test]
    fn the_helper_never_changes_its_own_or_an_administrators_settings() {
        let own = item("assistant.reach.power", ItemKind::Toggle, RiskInfo::Safe);
        assert!(closed("assistant", &own).is_some());
        let admin = item("sharing.ssh", ItemKind::Toggle, RiskInfo::AdminOnly);
        assert!(closed("sharing", &admin).is_some());
        let plain = item("appearance.dark", ItemKind::Toggle, RiskInfo::Safe);
        assert!(closed("appearance", &plain).is_none());
        assert!(SENSITIVE_PANELS.contains(&"privacy"));
        assert!(!SENSITIVE_PANELS.contains(&"appearance"));
    }

    #[test]
    fn a_value_is_only_what_the_setting_accepts() {
        let range = ItemKind::Integer {
            min: 0,
            max: 100,
            step: 1,
        };
        assert_eq!(value_of(&range, "40"), Some(SettingValue::Integer(40)));
        assert_eq!(value_of(&range, "400"), None);
        assert_eq!(value_of(&ItemKind::Toggle, "maybe"), None);
        let choice = ItemKind::Choice {
            choices: vec![ChoiceInfo {
                id: "dark".into(),
                label: Text::default(),
            }],
        };
        assert_eq!(
            value_of(&choice, "dark"),
            Some(SettingValue::Text("dark".into()))
        );
        assert_eq!(value_of(&choice, "pink"), None);
        assert!(short("x".repeat(5000)).len() < 1600);
    }
}
