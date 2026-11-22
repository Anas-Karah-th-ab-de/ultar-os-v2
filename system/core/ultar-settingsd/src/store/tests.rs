use super::*;
use crate::clock::ManualClock;
use crate::schema::{Migration, MigrationStep, Panel, Scope, SettingDef};
use crate::testdir::TestDir;
use std::fs;

const DELAY: Duration = Duration::from_millis(200);

fn registry(version: u32) -> Registry {
    let mut builder = Registry::builder()
        .version(version)
        .setting(SettingDef::choice(
            "appearance.theme",
            Panel::Appearance,
            &["dark", "light"],
            "dark",
        ))
        .setting(SettingDef::int("power.dim", Panel::Power, (0, 100, 1), 60).scope(Scope::System))
        .setting(
            SettingDef::int("display.<id>.scale", Panel::Display, (100, 300, 25), 100)
                .scope(Scope::DisplayCombination),
        )
        .setting(SettingDef::flag(
            "notifications.app.<id>.sound",
            Panel::Notifications,
            true,
        ));
    if version >= 2 {
        builder = builder.migration(Migration {
            to_version: 2,
            steps: vec![MigrationStep::Rename {
                from: "look.theme",
                to: "appearance.theme",
            }],
        });
    }
    builder.build().unwrap()
}

struct Fixture {
    dir: TestDir,
    clock: ManualClock,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            dir: TestDir::new(tag),
            clock: ManualClock::new(),
        }
    }

    fn open_with(&self, registry: &Registry) -> (Store, Vec<Notice>) {
        let config = StoreConfig {
            dir: self.dir.path().to_path_buf(),
            user: "anas".into(),
            write_delay: DELAY,
        };
        Store::open(config, registry, Arc::new(self.clock.clone())).unwrap()
    }

    fn open(&self) -> (Store, Vec<Notice>) {
        self.open_with(&registry(1))
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }
}

#[test]
fn empty_directory_gives_an_empty_store_without_notices_or_writes() {
    let fixture = Fixture::new("store-empty");
    let (store, notices) = fixture.open();
    assert!(notices.is_empty());
    assert!(store.is_empty(Slot::User));
    assert!(!store.is_dirty());
    assert_eq!(store.next_deadline(), None);
    assert_eq!(store.stats(), StoreStats::default());
    assert_eq!(store.schema(), 1);
    assert_eq!(store.dir(), fixture.dir.path());
    assert!(format!("{store:?}").contains("Store"));
}

#[test]
fn invalid_user_names_are_rejected() {
    let fixture = Fixture::new("store-user");
    for user in ["", "../etc", "Anas", "a/b"] {
        let config = StoreConfig {
            dir: fixture.dir.path().to_path_buf(),
            user: user.into(),
            write_delay: DELAY,
        };
        let result = Store::open(config, &registry(1), Arc::new(ManualClock::new()));
        assert_eq!(result.unwrap_err(), StoreError::InvalidUser);
    }
}

#[test]
fn values_survive_a_reopen_per_scope() {
    let fixture = Fixture::new("store-reopen");
    let (mut store, _) = fixture.open();
    assert!(store.set(Slot::User, "appearance.theme", Value::text("light")));
    assert!(store.set(Slot::System, "power.dim", Value::Int(30)));
    assert!(store.use_combo("c-1", &["edp".to_owned()]));
    assert!(store.set(Slot::Combo("c-1"), "display.edp.scale", Value::Int(150)));
    assert!(!store.set(Slot::Combo("c-9"), "display.edp.scale", Value::Int(150)));
    assert_eq!(store.flush().unwrap(), 3);
    let (store, notices) = fixture.open();
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(
        store.get(Slot::User, "appearance.theme"),
        Some(&Value::text("light"))
    );
    assert_eq!(store.get(Slot::System, "power.dim"), Some(&Value::Int(30)));
    assert_eq!(
        store.get(Slot::Combo("c-1"), "display.edp.scale"),
        Some(&Value::Int(150))
    );
    assert_eq!(store.get(Slot::System, "appearance.theme"), None);
    assert_eq!(store.combo("c-1").unwrap().displays, ["edp"]);
    assert_eq!(
        store.keys(Slot::User).collect::<Vec<_>>(),
        ["appearance.theme"]
    );
    assert_eq!(store.len(Slot::Combo("c-1")), 1);
    assert!(fixture.file("user-anas.toml").exists());
}

#[test]
fn many_sets_become_one_write_after_a_bounded_delay() {
    let fixture = Fixture::new("store-coalesce");
    let (mut store, _) = fixture.open();
    for step in 0..100 {
        store.set(Slot::System, "power.dim", Value::Int(step));
        // Later changes must not push the deadline further away.
        fixture.clock.advance(Duration::from_millis(1));
        assert_eq!(store.next_deadline(), Some(DELAY));
        assert_eq!(store.tick().unwrap(), 0);
    }
    assert_eq!(store.stats().writes, 0);
    assert!(!fixture.file("system.toml").exists());
    fixture.clock.advance(DELAY);
    assert_eq!(store.tick().unwrap(), 1);
    assert_eq!(store.stats().writes, 1);
    assert_eq!(store.stats().changes, 100);
    assert_eq!(store.next_deadline(), None);
    assert!(!store.is_dirty());
    assert_eq!(store.tick().unwrap(), 0);
    let (store, _) = fixture.open();
    assert_eq!(store.get(Slot::System, "power.dim"), Some(&Value::Int(99)));
}

#[test]
fn unchanged_values_cause_no_write() {
    let fixture = Fixture::new("store-same");
    let (mut store, _) = fixture.open();
    assert!(store.set(Slot::User, "appearance.theme", Value::text("light")));
    store.flush().unwrap();
    assert!(!store.set(Slot::User, "appearance.theme", Value::text("light")));
    assert!(!store.remove(Slot::User, "power.dim"));
    assert!(!store.remove(Slot::Combo("none"), "x"));
    assert!(!store.is_dirty());
    assert!(store.remove(Slot::User, "appearance.theme"));
    assert!(store.is_dirty());
    assert_eq!(store.flush().unwrap(), 1);
    assert_eq!(store.flush().unwrap(), 0);
}

#[test]
fn damaged_file_is_put_aside_and_replaced_by_defaults() {
    let fixture = Fixture::new("store-damaged");
    let (mut store, _) = fixture.open();
    store.set(Slot::User, "appearance.theme", Value::text("light"));
    store.flush().unwrap();
    let good = fs::read_to_string(fixture.file("user-anas.toml")).unwrap();
    let bad = good.replace("light", "lihgt");
    fs::write(fixture.file("user-anas.toml"), &bad).unwrap();

    let (mut store, notices) = fixture.open();
    assert_eq!(
        notices,
        [Notice {
            file: "user-anas.toml".into(),
            kind: StoreNoticeKind::Quarantined,
            detail: "quarantine/user-anas.toml.0".into(),
        }]
    );
    assert_eq!(store.get(Slot::User, "appearance.theme"), None);
    assert_eq!(
        fs::read_to_string(fixture.file("quarantine/user-anas.toml.0")).unwrap(),
        bad
    );
    // A clean file replaces the damaged one at the next write.
    assert!(store.is_dirty());
    fixture.clock.advance(DELAY);
    assert_eq!(store.tick().unwrap(), 1);
    let (_, notices) = fixture.open();
    assert!(notices.is_empty());
}

#[test]
fn file_of_a_newer_system_is_put_aside_not_guessed_at() {
    let fixture = Fixture::new("store-newer");
    let (mut store, _) = fixture.open_with(&registry(2));
    store.set(Slot::User, "appearance.theme", Value::text("light"));
    store.flush().unwrap();
    let (store, notices) = fixture.open_with(&registry(1));
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, StoreNoticeKind::NewerSchema);
    assert_eq!(store.get(Slot::User, "appearance.theme"), None);
    assert!(fixture.file("quarantine/user-anas.toml.0").exists());
}

#[test]
fn older_file_is_migrated_and_rewritten() {
    let fixture = Fixture::new("store-migrate");
    let mut old = FileContent {
        schema: 1,
        ..FileContent::default()
    };
    old.values.insert("look.theme".into(), Value::text("light"));
    fs::write(fixture.file("user-anas.toml"), encode(&old)).unwrap();
    let (mut store, notices) = fixture.open_with(&registry(2));
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, StoreNoticeKind::Migrated);
    assert_eq!(notices[0].detail, "1->2");
    assert_eq!(
        store.get(Slot::User, "appearance.theme"),
        Some(&Value::text("light"))
    );
    assert_eq!(store.flush().unwrap(), 1);
    let (store, notices) = fixture.open_with(&registry(2));
    assert!(notices.is_empty());
    assert_eq!(store.schema(), 2);
    assert_eq!(
        store.get(Slot::User, "appearance.theme"),
        Some(&Value::text("light"))
    );
}

fn values(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

/// A user file and a display file with entries that do not belong there.
fn write_files_with_strays(fixture: &Fixture) {
    let mut user = FileContent {
        schema: 1,
        values: values(&[
            ("appearance.theme", Value::text("pink")),
            ("gone.key", Value::Bool(true)),
            ("power.dim", Value::Int(5)),
            ("notifications.app.mail.sound", Value::Bool(false)),
        ]),
        ..FileContent::default()
    };
    user.combos.insert("c".into(), ComboData::default());
    fs::write(fixture.file("user-anas.toml"), encode(&user)).unwrap();
    let combo = ComboData {
        displays: vec!["edp".into()],
        used: 1,
        values: values(&[
            ("display.edp.scale", Value::Int(125)),
            ("display.hdmi.scale", Value::Int(125)),
            ("display.edp.scale2", Value::Int(1)),
        ]),
    };
    let displays = FileContent {
        schema: 1,
        values: values(&[("appearance.theme", Value::text("dark"))]),
        combos: BTreeMap::from([("c-1".to_owned(), combo)]),
    };
    fs::write(fixture.file("displays.toml"), encode(&displays)).unwrap();
}

#[test]
fn entries_the_schema_does_not_accept_are_dropped_and_reported() {
    let fixture = Fixture::new("store-check");
    write_files_with_strays(&fixture);
    let (store, notices) = fixture.open();
    let mut seen: Vec<(StoreNoticeKind, &str)> = notices
        .iter()
        .map(|notice| (notice.kind, notice.detail.as_str()))
        .collect();
    seen.sort_by_key(|(_, detail)| *detail);
    assert_eq!(
        seen,
        [
            (StoreNoticeKind::InvalidValueDropped, "appearance.theme"),
            (StoreNoticeKind::UnknownKeyDropped, "combo"),
            (StoreNoticeKind::UnknownKeyDropped, "display.edp.scale2"),
            (StoreNoticeKind::UnknownKeyDropped, "display.hdmi.scale"),
            (StoreNoticeKind::UnknownKeyDropped, "gone.key"),
            (StoreNoticeKind::UnknownKeyDropped, "power.dim"),
            (StoreNoticeKind::UnknownKeyDropped, "values"),
        ]
    );
    assert_eq!(store.len(Slot::User), 1);
    assert_eq!(
        store.get(Slot::User, "notifications.app.mail.sound"),
        Some(&Value::Bool(false))
    );
    assert_eq!(store.len(Slot::Combo("c-1")), 1);
    assert!(store.is_dirty());
}

#[test]
fn notices_are_bounded() {
    let fixture = Fixture::new("store-notices");
    let mut user = FileContent {
        schema: 1,
        ..FileContent::default()
    };
    for index in 0..MAX_NOTICES + 20 {
        user.values
            .insert(format!("gone.k{index}"), Value::Bool(true));
    }
    fs::write(fixture.file("user-anas.toml"), encode(&user)).unwrap();
    let (store, notices) = fixture.open();
    assert_eq!(notices.len(), MAX_NOTICES);
    assert!(store.is_empty(Slot::User));
}

#[test]
fn unreadable_entry_gives_defaults_and_a_notice() {
    let fixture = Fixture::new("store-unreadable");
    fs::create_dir(fixture.file("system.toml")).unwrap();
    let (store, notices) = fixture.open();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, StoreNoticeKind::Unreadable);
    assert!(store.is_empty(Slot::System));
}

#[test]
fn failed_write_is_retried_with_growing_pauses() {
    let fixture = Fixture::new("store-retry");
    let (mut store, _) = fixture.open();
    // A non-empty directory in the way makes the rename fail.
    fs::create_dir_all(fixture.file("system.toml").join("x")).unwrap();
    store.set(Slot::System, "power.dim", Value::Int(1));
    store.set(Slot::User, "appearance.theme", Value::text("light"));
    fixture.clock.advance(DELAY);
    let err = store.tick().unwrap_err();
    assert!(matches!(&err, StoreError::Io { file, .. } if file == "system.toml"));
    assert!(!err.to_string().is_empty());
    // The other file was written, the failed one stays queued.
    assert!(fixture.file("user-anas.toml").exists());
    assert!(store.is_dirty());
    assert_eq!(store.stats().failed_writes, 1);
    assert_eq!(store.next_deadline(), Some(DELAY * 3));
    assert_eq!(store.tick().unwrap(), 0);
    fixture.clock.advance(DELAY * 2);
    assert!(store.tick().is_err());
    assert_eq!(store.next_deadline(), Some(DELAY * 7));
    for _ in 0..20 {
        assert!(store.flush().is_err());
    }
    let wait = store.next_deadline().unwrap() - fixture.clock.now();
    assert_eq!(wait, MAX_RETRY);
    fs::remove_dir_all(fixture.file("system.toml")).unwrap();
    assert_eq!(store.flush().unwrap(), 1);
    assert_eq!(store.next_deadline(), None);
}

#[test]
fn failed_write_with_zero_delay_still_pauses_before_retrying() {
    let dir = TestDir::new("store-retry-zero");
    let clock = ManualClock::new();
    let config = StoreConfig {
        dir: dir.path().to_path_buf(),
        user: "anas".into(),
        write_delay: Duration::ZERO,
    };
    let (mut store, _) = Store::open(config, &registry(1), Arc::new(clock.clone())).unwrap();
    fs::create_dir_all(dir.path().join("system.toml").join("x")).unwrap();
    store.set(Slot::System, "power.dim", Value::Int(1));
    assert_eq!(store.next_deadline(), Some(Duration::ZERO));
    assert!(store.tick().is_err());
    assert_eq!(store.next_deadline(), Some(MIN_RETRY));
    // Not due yet: no second attempt.
    assert_eq!(store.tick().unwrap(), 0);
    assert_eq!(store.stats().failed_writes, 1);
}

#[test]
fn combos_are_bounded_and_the_least_recently_used_goes() {
    let fixture = Fixture::new("store-combos");
    let (mut store, _) = fixture.open();
    let displays = |n: usize| vec![format!("d{n}")];
    for n in 0..MAX_COMBOS {
        assert!(store.use_combo(&format!("c-{n}"), &displays(n)));
    }
    // Using the first again makes the second the oldest.
    assert!(!store.use_combo("c-0", &displays(0)));
    assert!(store.use_combo("c-new", &displays(999)));
    assert!(store.combo("c-0").is_some());
    assert!(store.combo("c-1").is_none());
    assert!(store.combo("c-new").is_some());
    // Using the active one again changes nothing.
    store.flush().unwrap();
    assert!(!store.use_combo("c-new", &displays(999)));
    assert!(!store.is_dirty());
    // Same identifier, other displays: old values must not leak over.
    store.set(Slot::Combo("c-new"), "display.d999.scale", Value::Int(200));
    assert!(store.use_combo("c-new", &displays(5)));
    assert!(store.is_empty(Slot::Combo("c-new")));
}

#[test]
fn journal_is_written_at_once_and_survives_a_reopen() {
    let fixture = Fixture::new("store-journal");
    let (mut store, _) = fixture.open();
    store.journal_add("display.mode").unwrap();
    store.journal_add("display.mode").unwrap();
    store.journal_add("display.primary").unwrap();
    assert_eq!(store.stats().writes, 2);
    let (mut store, notices) = fixture.open();
    assert!(notices.is_empty());
    assert_eq!(
        store.journal().collect::<Vec<_>>(),
        ["display.mode", "display.primary"]
    );
    store
        .journal_remove(&["display.mode".to_owned(), "x".to_owned()])
        .unwrap();
    store.journal_remove(&["x".to_owned()]).unwrap();
    assert_eq!(store.stats().writes, 1);
    let (store, _) = fixture.open();
    assert_eq!(store.journal().collect::<Vec<_>>(), ["display.primary"]);
}

#[test]
fn journal_entry_is_not_kept_when_it_cannot_be_written() {
    let fixture = Fixture::new("store-journal-fail");
    let (mut store, _) = fixture.open();
    fs::create_dir_all(fixture.file("pending.toml").join("x")).unwrap();
    assert!(store.journal_add("display.mode").is_err());
    assert_eq!(store.journal().count(), 0);
}

#[test]
fn leftovers_of_an_interrupted_write_are_removed() {
    let fixture = Fixture::new("store-temp");
    fs::write(fixture.file("system.toml.tmp"), "half a fi").unwrap();
    let (store, notices) = fixture.open();
    assert!(notices.is_empty());
    assert!(store.is_empty(Slot::System));
    assert!(!fixture.file("system.toml.tmp").exists());
}
