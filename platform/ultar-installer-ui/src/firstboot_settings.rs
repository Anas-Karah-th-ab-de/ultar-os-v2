//! First start, part 2: the installer's other choices become settings and a selection record.
//!
//! * Language, keyboard, time zone, the two accessibility switches and the device name go into
//!   the store of the settings service (`ultar-settingsd`, `/data/ultar/settings`): each key is
//!   resolved and its value checked against the seed schema, the file is chosen by the key's
//!   scope (`system.toml` or `user-<user>.toml`), written atomically in the service's own format.
//! * Edition, components, codecs and "updates during the installation" go into
//!   `/data/ultar/install/selection.toml`: the Store and the update service install them when the
//!   device is online (the medium carries no component packages and the installer has no network).
//!
//! Rules: a value already stored is never replaced (a start after an interruption, or a disk
//! that kept its data, keeps what is there); a damaged settings file is left untouched for the
//! settings service to quarantine; a value the schema refuses is left out and reported. Nothing
//! here fails the first start: the account (part 1) is what must not be lost.

use crate::engine::answers::Answers;
use std::fmt::Write as _;
use std::path::Path;
use ultar_settingsd::schema::{Registry, Scope};
use ultar_settingsd::store::{decode, encode, write_atomic, FileContent};
use ultar_settingsd::value::Value;

/// Name of the device-wide settings file.
pub const SYSTEM_FILE: &str = "system.toml";
/// File name of the selection record in its directory.
pub const SELECTION_FILE: &str = "selection.toml";
/// Component ids the selection record accepts (`crate::components::Component::id`).
const COMPONENT_IDS: [&str; 12] = [
    "office",
    "developer",
    "media",
    "communication",
    "mesh",
    "assistant",
    "linux",
    "firefox",
    "windows",
    "android",
    "containers",
    "remote",
];

const TZ_NAME: &str = "datetime.timezone.name";
const TZ_AUTOMATIC: &str = "datetime.timezone.automatic";

/// What part 2 did, for the service's log (never a secret: no value of the account).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsReport {
    /// Keys stored now.
    pub stored: Vec<String>,
    /// Keys left as they were (a value was stored already).
    pub kept: Vec<String>,
    /// Keys the schema refused (unknown key or value out of its type).
    pub refused: Vec<String>,
    /// Settings files left untouched because they are damaged or unreadable.
    pub damaged: Vec<String>,
    /// Whether the selection record was written.
    pub selection: bool,
}

/// The settings the answers ask for, in a fixed order (keys of the seed schema).
pub fn settings_from(a: &Answers) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    if let Some(lang) = a.get("lang") {
        out.push(("region.language", Value::text(lang)));
    }
    if let Some((layouts, active)) = a.get("keyboard").and_then(keyboard) {
        out.push(("region.keyboard.layouts", Value::text(layouts)));
        out.push(("region.keyboard.active", Value::text(active)));
    }
    if let Some(tz) = a.get("timezone") {
        out.push(("datetime.timezone.name", Value::text(tz)));
        // Chosen by hand: the automatic time zone would replace it at the first network
        // (stored only together with the name, see `apply_settings`).
        out.push((TZ_AUTOMATIC, Value::Bool(false)));
    }
    for (answer, key) in [
        ("large_text", "accessibility.large_text"),
        ("screen_reader", "accessibility.screen_reader"),
    ] {
        if let Some(flag) = a.get(answer).and_then(flag) {
            out.push((key, Value::Bool(flag)));
        }
    }
    if let Some(device) = a.get("device") {
        out.push(("sharing.device_name", Value::text(device)));
    }
    out
}

/// The installer's keyboard id as (switch list, active layout) of the settings schema.
fn keyboard(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "ar+us" => Some(("ar_en", "ar")),
        "us" => Some(("ar_en", "en")),
        "de" => Some(("ar_de", "de")),
        _ => None,
    }
}

fn flag(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// One settings file: loaded (or new), changed, written back when something was added.
struct Pending {
    name: String,
    content: FileContent,
    changed: bool,
}

/// Stores the settings of the answers in `dir` (the settings service's store) for the user
/// file `user-<user>.toml`. Every problem ends up in the report, none stops the first start.
pub fn apply_settings(a: &Answers, dir: &Path, user: &str, report: &mut SettingsReport) {
    let Ok(registry) = ultar_settingsd::seed::registry() else {
        report.refused.push("schema".to_owned());
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        report.damaged.push(dir.display().to_string());
        return;
    }
    let user_file = format!("user-{user}.toml");
    let mut files: Vec<Pending> = Vec::new();
    for (key, value) in settings_from(a) {
        if key == TZ_AUTOMATIC && report.refused.iter().any(|k| k == TZ_NAME) {
            continue;
        }
        let Some(scope) = checked_scope(&registry, key, &value) else {
            report.refused.push(key.to_owned());
            continue;
        };
        let name = match scope {
            Scope::System => SYSTEM_FILE,
            Scope::User => user_file.as_str(),
            Scope::DisplayCombination => {
                report.refused.push(key.to_owned());
                continue;
            }
        };
        let Some(file) = open_file(&mut files, dir, name, &registry, report) else {
            continue;
        };
        if file.content.values.contains_key(key) {
            report.kept.push(key.to_owned());
        } else {
            file.content.values.insert(key.to_owned(), value);
            file.changed = true;
            report.stored.push(key.to_owned());
        }
    }
    for file in files.iter().filter(|f| f.changed) {
        if write_atomic(dir, &file.name, encode(&file.content).as_bytes()).is_err() {
            report
                .stored
                .retain(|k| !belongs(&registry, k, &file.name, &user_file));
            report.damaged.push(file.name.clone());
        }
    }
}

/// The loaded file `name` (loaded now when first needed); `None` when it is damaged.
fn open_file<'f>(
    files: &'f mut Vec<Pending>,
    dir: &Path,
    name: &str,
    registry: &Registry,
    report: &mut SettingsReport,
) -> Option<&'f mut Pending> {
    if let Some(i) = files.iter().position(|f| f.name == name) {
        return files.get_mut(i);
    }
    let Some(content) = load(dir, name, registry) else {
        if !report.damaged.iter().any(|d| d == name) {
            report.damaged.push(name.to_owned());
        }
        return None;
    };
    files.push(Pending {
        name: name.to_owned(),
        content,
        changed: false,
    });
    files.last_mut()
}

/// The scope of `key` when the schema knows it and accepts `value`.
fn checked_scope(registry: &Registry, key: &str, value: &Value) -> Option<Scope> {
    let resolved = registry.resolve(key).ok()?;
    resolved.def.value_type.validate(value).ok()?;
    Some(resolved.def.scope)
}

fn belongs(registry: &Registry, key: &str, file: &str, user_file: &str) -> bool {
    match registry.resolve(key).map(|r| r.def.scope) {
        Ok(Scope::System) => file == SYSTEM_FILE,
        Ok(Scope::User) => file == user_file,
        _ => false,
    }
}

/// The file's content; empty content of the current schema when there is none; `None` when it
/// exists but cannot be read or is damaged (left for the settings service).
fn load(dir: &Path, name: &str, registry: &Registry) -> Option<FileContent> {
    match std::fs::read_to_string(dir.join(name)) {
        Ok(text) => decode(&text).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(FileContent {
            schema: registry.version(),
            ..FileContent::default()
        }),
        Err(_) => None,
    }
}

/// The selection record's content (TOML), only known component ids.
pub fn selection_text(a: &Answers) -> String {
    let edition = match a.get("edition") {
        Some("server") => "server",
        _ => "desktop",
    };
    let components: Vec<String> = a
        .get("components")
        .unwrap_or("")
        .split(',')
        .filter(|id| COMPONENT_IDS.contains(id))
        .map(|id| format!("\"{id}\""))
        .collect();
    let mut out = String::from(
        "# Chosen at the installation. The Store and the update service install\n\
         # what is listed here when the device is online, then remove the line \"pending\".\n",
    );
    let _ = writeln!(out, "format = 1");
    let _ = writeln!(out, "pending = true");
    let _ = writeln!(out, "edition = \"{edition}\"");
    let _ = writeln!(out, "components = [{}]", components.join(", "));
    for key in ["codecs", "updates_during_install"] {
        let on = a.get(key).and_then(flag).unwrap_or(false);
        let _ = writeln!(out, "{key} = {on}");
    }
    out
}

/// Writes the selection record into `dir` unless one exists.
pub fn apply_selection(a: &Answers, dir: &Path, report: &mut SettingsReport) {
    if dir.join(SELECTION_FILE).exists() {
        return;
    }
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| write_atomic(dir, SELECTION_FILE, selection_text(a).as_bytes()));
    match written {
        Ok(()) => report.selection = true,
        Err(_) => report.damaged.push(SELECTION_FILE.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(pairs: &[(&str, &str)]) -> Answers {
        let mut a = Answers::default();
        for (k, v) in pairs {
            a.set(k, v);
        }
        a
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-fb-set-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn read(dir: &Path, name: &str) -> FileContent {
        decode(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    }

    const FULL: &[(&str, &str)] = &[
        ("lang", "de"),
        ("keyboard", "de"),
        ("timezone", "Europe/Berlin"),
        ("large_text", "true"),
        ("screen_reader", "false"),
        ("device", "anas-pc"),
        ("edition", "server"),
        ("components", "office,assistant,bogus,linux"),
        ("codecs", "true"),
    ];

    #[test]
    fn choices_land_in_the_right_files_in_the_service_format() {
        let dir = scratch("full");
        let mut report = SettingsReport::default();
        apply_settings(&answers(FULL), &dir, "default", &mut report);
        assert!(
            report.refused.is_empty() && report.damaged.is_empty(),
            "{report:?}"
        );
        assert_eq!(report.stored.len(), 8);
        let system = read(&dir, SYSTEM_FILE);
        assert_eq!(
            system.values.get("datetime.timezone.name"),
            Some(&Value::text("Europe/Berlin"))
        );
        assert_eq!(
            system.values.get("datetime.timezone.automatic"),
            Some(&Value::Bool(false))
        );
        let user = read(&dir, "user-default.toml");
        assert_eq!(user.values.get("region.language"), Some(&Value::text("de")));
        assert_eq!(
            user.values.get("region.keyboard.layouts"),
            Some(&Value::text("ar_de"))
        );
        assert_eq!(
            user.values.get("accessibility.large_text"),
            Some(&Value::Bool(true))
        );
        // Every key sits in the file of its scope only.
        let registry = ultar_settingsd::seed::registry().unwrap();
        for k in system.values.keys() {
            assert_eq!(registry.resolve(k).unwrap().def.scope, Scope::System, "{k}");
        }
        for k in user.values.keys() {
            assert_eq!(registry.resolve(k).unwrap().def.scope, Scope::User, "{k}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_settings_service_reads_the_files_without_a_notice() {
        use ultar_settingsd::store::{Slot, Store, StoreConfig};
        let dir = scratch("service");
        let mut report = SettingsReport::default();
        apply_settings(&answers(FULL), &dir, "default", &mut report);
        let registry = ultar_settingsd::seed::registry().unwrap();
        let config = StoreConfig {
            dir: dir.clone(),
            user: "default".into(),
            write_delay: std::time::Duration::from_millis(10),
        };
        let clock = std::sync::Arc::new(ultar_settingsd::clock::ManualClock::default());
        let (store, notices) = Store::open(config, &registry, clock).unwrap();
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(
            store.get(Slot::User, "region.keyboard.active"),
            Some(&Value::text("de"))
        );
        assert_eq!(
            store.get(Slot::System, "sharing.device_name"),
            Some(&Value::text("anas-pc"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stored_values_stay_and_damaged_files_are_not_touched() {
        let dir = scratch("keep");
        std::fs::create_dir_all(&dir).unwrap();
        let mut mine = FileContent {
            schema: 1,
            ..FileContent::default()
        };
        mine.values
            .insert("region.language".into(), Value::text("ar"));
        std::fs::write(dir.join("user-default.toml"), encode(&mine)).unwrap();
        std::fs::write(dir.join(SYSTEM_FILE), "junk").unwrap();
        let mut report = SettingsReport::default();
        apply_settings(&answers(FULL), &dir, "default", &mut report);
        assert!(report.kept.contains(&"region.language".to_owned()));
        assert_eq!(report.damaged, vec![SYSTEM_FILE.to_owned()]);
        assert_eq!(
            std::fs::read_to_string(dir.join(SYSTEM_FILE)).unwrap(),
            "junk"
        );
        let user = read(&dir, "user-default.toml");
        assert_eq!(user.values.get("region.language"), Some(&Value::text("ar")));
        assert_eq!(
            user.values.get("region.keyboard.active"),
            Some(&Value::text("de"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_the_schema_refuses_are_left_out() {
        let dir = scratch("bad");
        let mut report = SettingsReport::default();
        let bad = answers(&[
            ("lang", "fr"),
            ("keyboard", "xx"),
            ("timezone", "../etc"),
            ("large_text", "yes"),
            ("device", "-bad-"),
        ]);
        apply_settings(&bad, &dir, "default", &mut report);
        assert!(report.stored.is_empty(), "{report:?}");
        assert!(report.refused.contains(&"region.language".to_owned()));
        assert!(report.refused.contains(&TZ_NAME.to_owned()));
        assert!(report.refused.contains(&"sharing.device_name".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selection_record_lists_only_known_components_once() {
        let text = selection_text(&answers(FULL));
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(table["edition"].as_str(), Some("server"));
        let ids: Vec<&str> = table["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ids, ["office", "assistant", "linux"]);
        assert_eq!(table["codecs"].as_bool(), Some(true));
        assert_eq!(table["updates_during_install"].as_bool(), Some(false));
        assert_eq!(table["pending"].as_bool(), Some(true));
        // Hostile component text cannot break out of the list.
        let evil = selection_text(&answers(&[("components", "office\"]\nx = [\"")]));
        let t: toml::Table = evil.parse().unwrap();
        assert!(t["components"].as_array().unwrap().is_empty());
        let dir = scratch("sel");
        let mut report = SettingsReport::default();
        apply_selection(&answers(FULL), &dir, &mut report);
        assert!(report.selection);
        let mut again = SettingsReport::default();
        apply_selection(&answers(&[]), &dir, &mut again);
        assert!(!again.selection);
        assert!(std::fs::read_to_string(dir.join(SELECTION_FILE))
            .unwrap()
            .contains("server"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn component_ids_match_the_installer() {
        let ids: Vec<&str> = crate::components::Component::ALL
            .iter()
            .map(|c| c.id())
            .collect();
        assert_eq!(ids, COMPONENT_IDS);
    }
}
