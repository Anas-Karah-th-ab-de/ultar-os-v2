//! What the services report, as the phone shell shows it: pure functions, tested without a
//! bus (apps from the launcher's list, a notification, a network state, the next
//! appointment).

use crate::model::apps::{self, App};
use crate::model::quick::Toggle;
use crate::model::{Mobile, Note};
use ultar_bus::messages::{calendar, net, notify};
use ultar_bus::{Text, TextArg};
use ultar_i18n::Translator;
use ultar_ui::icon::IconId;

/// Prefix of the shell's names of base-system apps (`org.ultar.files`).
const BASE: &str = "org.ultar.";

/// The launcher's id of an app (`org.ultar.files` → `files`; other ids stay).
pub fn launcher_id(app: &str) -> &str {
    app.strip_prefix(BASE)
        .filter(|rest| !rest.is_empty() && !rest.contains('.'))
        .unwrap_or(app)
}

/// The shell's id of an app the launcher lists (`files` → `org.ultar.files`).
pub fn shell_id(id: &str) -> String {
    if id.contains('.') {
        id.to_owned()
    } else {
        format!("{BASE}{id}")
    }
}

/// The installed apps in home screen order: the system apps first (in their order), then
/// the others by id.
pub fn apps_from(listed: &[String]) -> Vec<App> {
    let ids: Vec<String> = listed.iter().map(|a| shell_id(a)).collect();
    let mut out: Vec<App> = apps::system_apps()
        .into_iter()
        .filter(|a| ids.contains(&a.id))
        .collect();
    let mut others: Vec<&String> = ids
        .iter()
        .filter(|id| !out.iter().any(|a| &a.id == *id))
        .collect();
    others.sort();
    others.dedup();
    for id in others {
        let name = id.rsplit('.').next().unwrap_or(id);
        out.push(App::named(id, name, IconId::Apps));
    }
    out
}

/// A text of the bus in the shell's language; `None` when the catalog does not know it.
pub fn text_of(text: &Text, tr: &Translator) -> Option<String> {
    let values: Vec<(String, String)> = text
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let v = match a {
                TextArg::Verbatim(s) => s.clone(),
                TextArg::Number(n) => n.to_string(),
                TextArg::Key(k) => tr.t(k).into_owned(),
            };
            (format!("arg{i}"), v)
        })
        .collect();
    let args: Vec<(&str, ultar_i18n::Arg<'_>)> = values
        .iter()
        .map(|(k, v)| (k.as_str(), ultar_i18n::Arg::Str(v)))
        .collect();
    let out = tr.t_args(&text.key, &args);
    (out != text.key).then_some(out)
}

/// A notification as the shell shows it: an unknown title falls back to the app's id.
pub fn note_from(p: &notify::Posted, tr: &Translator, offset_s: i32) -> Note {
    let title = text_of(&p.post.title, tr).unwrap_or_else(|| p.post.app_id.clone());
    let body = match &p.post.body {
        notify::Body::Empty => String::new(),
        notify::Body::Localized(t) => text_of(t, tr).unwrap_or_default(),
        notify::Body::Verbatim(s) => s.clone(),
    };
    let time = crate::model::time::LocalTime::at(p.timestamp_ms, offset_s)
        .map(|t| t.clock())
        .unwrap_or_default();
    Note {
        id: p.id,
        app: shell_id(&p.post.app_id),
        title,
        body,
        time,
    }
}

/// Signal percent → bars 0–4.
pub fn bars(percent: Option<u8>) -> Option<u8> {
    percent.map(|p| (u16::from(p.min(100)) * 4 / 100).max(1) as u8)
}

/// What the network service reports.
pub fn apply_net(m: &mut Mobile, n: &net::NetState) {
    m.quick.set(Toggle::Wifi, n.wifi_on);
    m.quick.set(Toggle::Airplane, n.airplane);
    m.quick.network = n.ssid.clone();
    m.status.wifi = if n.ssid.is_some() {
        bars(n.signal_percent).or(Some(1))
    } else {
        None
    };
}

/// The next appointment of the day after `minute` (`14:00 · Dentist`).
pub fn next_of(events: &[calendar::DayEvent], minute: u16) -> Option<String> {
    events
        .iter()
        .filter(|e| e.start_minute >= minute)
        .min_by_key(|e| e.start_minute)
        .map(|e| {
            format!(
                "{:02}:{:02} · {}",
                e.start_minute / 60,
                e.start_minute % 60,
                e.title
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    #[test]
    fn launcher_ids_map_both_ways() {
        assert_eq!(launcher_id("org.ultar.files"), "files");
        assert_eq!(
            launcher_id("km-code-craft.appbuilder"),
            "km-code-craft.appbuilder"
        );
        assert_eq!(shell_id("files"), "org.ultar.files");
        assert_eq!(shell_id("a.b"), "a.b");
    }

    #[test]
    fn installed_apps_keep_the_system_order_then_the_others() {
        let listed = ["store", "x.game", "files", "settings", "a.tool"].map(String::from);
        let ids: Vec<String> = apps_from(&listed).into_iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            [
                "org.ultar.settings",
                "org.ultar.files",
                "org.ultar.store",
                "a.tool",
                "x.game"
            ]
        );
    }

    #[test]
    fn a_notification_keeps_a_person_s_text_and_falls_back_for_unknown_keys() {
        let tr = crate::i18n::translator(Lang::En).unwrap();
        let p = notify::Posted {
            id: 7,
            timestamp_ms: 1_795_636_560_000,
            sender: "phone".into(),
            post: notify::Post {
                app_id: "phone".into(),
                title: Text::key("phone.missed"),
                body: notify::Body::Verbatim("Layla".into()),
                urgency: notify::Urgency::default(),
                category: notify::Category::Call,
                actions: Vec::new(),
            },
        };
        let n = note_from(&p, &tr, 0);
        assert_eq!(
            (n.id, n.app.as_str(), n.title.as_str()),
            (7, "org.ultar.phone", "phone")
        );
        assert_eq!((n.body.as_str(), n.time.as_str()), ("Layla", "19:56"));
    }

    #[test]
    fn the_network_state_sets_tiles_and_bars() {
        let mut m = Mobile::default();
        let n = net::NetState {
            wifi_on: true,
            ssid: Some("KM".into()),
            signal_percent: Some(60),
            airplane: false,
            ..net::NetState::default()
        };
        apply_net(&mut m, &n);
        assert!(m.quick.is_on(Toggle::Wifi));
        assert_eq!(
            (m.quick.network.as_deref(), m.status.wifi),
            (Some("KM"), Some(2))
        );
        assert_eq!(bars(Some(0)), Some(1));
        assert_eq!(bars(Some(100)), Some(4));
    }

    #[test]
    fn the_next_appointment_is_the_first_still_to_come() {
        let e = |start: u16, title: &str| calendar::DayEvent {
            id: 1,
            start_minute: start,
            duration_minutes: 30,
            title: title.into(),
        };
        let events = [e(600, "A"), e(840, "B"), e(720, "C")];
        assert_eq!(next_of(&events, 700).as_deref(), Some("12:00 · C"));
        assert_eq!(next_of(&events, 900), None);
    }
}
