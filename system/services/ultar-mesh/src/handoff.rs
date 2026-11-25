//! Deciding where an activity can go and how it arrives there.
//!
//! The decision uses capabilities only: the same app installed means the activity itself
//! continues; a device that keeps files takes the file; a device that only shows pictures
//! can show this device's window. A device that can do none of it is not offered.

use crate::device::Device;
use ultar_bus::messages::mesh::{Activity, Capability, Stage};

/// Longest URI an offer carries.
pub const MAX_URI: usize = 2048;
/// How long the person on the receiving device has to answer.
pub const ASK_SECONDS: u64 = 60;

/// The word of a stage on the wire.
pub fn stage_word(stage: Stage) -> &'static str {
    match stage {
        Stage::State => "state",
        Stage::File => "file",
        Stage::RemoteView => "remote_view",
    }
}

/// The stage a word names.
pub fn stage_from(word: &str) -> Option<Stage> {
    match word {
        "state" => Some(Stage::State),
        "file" => Some(Stage::File),
        "remote_view" => Some(Stage::RemoteView),
        _ => None,
    }
}

/// `true` when `uri` may travel: not empty, not too long, no control characters.
pub fn uri_ok(uri: &str) -> bool {
    !uri.is_empty() && uri.len() <= MAX_URI && !uri.chars().any(char::is_control)
}

/// How `activity` would arrive on `target`; `None` when `target` cannot take it.
/// `remote_view`: this device can show its windows on another device.
pub fn stage_for(activity: &Activity, target: &Device, remote_view: bool) -> Option<Stage> {
    let has_file = activity.file.is_some();
    let same_app = target.can(Capability::Apps) && target.apps.contains(&activity.app);
    // The same app continues: at the place, with the file, or - when the app reported
    // neither - simply opened there.
    if same_app && (!has_file || target.can(Capability::Files)) {
        return Some(Stage::State);
    }
    if has_file && target.can(Capability::Files) {
        return Some(Stage::File);
    }
    if remote_view && target.can(Capability::Screen) {
        return Some(Stage::RemoteView);
    }
    None
}

/// The URI to open on the receiver: a local file's place (`#…` or `?…` of the sender's
/// URI) is kept and put behind the path the file arrived at.
pub fn arrived_uri(sent: Option<&str>, arrived: Option<&std::path::Path>) -> Option<String> {
    let Some(path) = arrived else {
        return sent.filter(|u| uri_ok(u)).map(str::to_owned);
    };
    let place = sent
        .filter(|u| u.starts_with("file://"))
        .and_then(|u| u.find(['#', '?']).map(|at| &u[at..]))
        .unwrap_or("");
    Some(format!("file://{}{place}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::default_capabilities;
    use ultar_bus::messages::mesh::DeviceKind;

    fn device(kind: DeviceKind, apps: &[&str]) -> Device {
        Device {
            id: "x".into(),
            name: "x".into(),
            kind,
            capabilities: default_capabilities(kind),
            apps: apps.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn editing() -> Activity {
        Activity {
            app: "editor".into(),
            title: "notes.txt".into(),
            uri: Some("file:///home/a/notes.txt#line=12".into()),
            file: Some("/home/a/notes.txt".into()),
        }
    }

    #[test]
    fn the_same_app_continues_the_activity() {
        let pc = device(DeviceKind::Desktop, &["editor", "browser"]);
        assert_eq!(stage_for(&editing(), &pc, false), Some(Stage::State));
        let page = Activity {
            app: "browser".into(),
            title: "page".into(),
            uri: Some("https://example.org/a".into()),
            file: None,
        };
        assert_eq!(stage_for(&page, &pc, false), Some(Stage::State));
    }

    #[test]
    fn without_the_app_the_file_travels() {
        let pc = device(DeviceKind::Desktop, &["browser"]);
        assert_eq!(stage_for(&editing(), &pc, false), Some(Stage::File));
        let server = device(DeviceKind::Server, &[]);
        assert_eq!(stage_for(&editing(), &server, true), Some(Stage::File));
    }

    #[test]
    fn a_tv_can_only_show_and_a_speaker_takes_no_document() {
        let tv = device(DeviceKind::Tv, &[]);
        assert_eq!(stage_for(&editing(), &tv, true), Some(Stage::RemoteView));
        assert_eq!(stage_for(&editing(), &tv, false), None);
        let speaker = device(DeviceKind::Speaker, &[]);
        assert_eq!(stage_for(&editing(), &speaker, true), None);
    }

    #[test]
    fn an_app_without_a_place_is_opened_where_it_is_installed() {
        let pc = device(DeviceKind::Desktop, &["clock"]);
        let empty = Activity {
            app: "clock".into(),
            ..Activity::default()
        };
        assert_eq!(stage_for(&empty, &pc, false), Some(Stage::State));
        let without = device(DeviceKind::Desktop, &["editor"]);
        assert_eq!(stage_for(&empty, &without, false), None);
    }

    #[test]
    fn the_place_in_a_file_survives_the_journey() {
        let arrived = std::path::Path::new("/data/inbox/notes.txt");
        assert_eq!(
            arrived_uri(Some("file:///home/a/notes.txt#line=12"), Some(arrived)).as_deref(),
            Some("file:///data/inbox/notes.txt#line=12")
        );
        assert_eq!(
            arrived_uri(Some("https://example.org/a"), None).as_deref(),
            Some("https://example.org/a")
        );
        assert_eq!(arrived_uri(Some("bad\nuri"), None), None);
        assert_eq!(
            stage_from(stage_word(Stage::RemoteView)),
            Some(Stage::RemoteView)
        );
        assert_eq!(stage_from("hologram"), None);
    }
}
