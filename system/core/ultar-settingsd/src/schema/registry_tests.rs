use super::super::def::Risk;
use super::super::migrate::MigrationStep;
use super::super::types::TextFormat;
use super::*;
use crate::value::Value;

fn theme() -> SettingDef {
    SettingDef::choice(
        "appearance.theme",
        Panel::Appearance,
        &["dark", "light"],
        "dark",
    )
}

fn scale() -> SettingDef {
    SettingDef::int("display.<id>.scale", Panel::Display, (100, 300, 25), 100)
        .scope(Scope::DisplayCombination)
}

fn build(defs: Vec<SettingDef>) -> Result<Registry, SchemaError> {
    Registry::builder().settings(defs).build()
}

#[test]
fn resolve_exact_and_instance_keys() {
    let registry = build(vec![theme(), scale()]).unwrap();
    let found = registry.resolve("appearance.theme").unwrap();
    assert_eq!(found.def.key, "appearance.theme");
    assert_eq!(found.instance, None);
    let found = registry.resolve("display.edp-1.scale").unwrap();
    assert_eq!(found.def.key, "display.<id>.scale");
    assert_eq!(found.instance.as_deref(), Some("edp-1"));
    assert_eq!(found.key, "display.edp-1.scale");
    assert_eq!(
        registry.resolve("display.scale").unwrap_err(),
        LookupError::UnknownKey
    );
    assert_eq!(
        registry.resolve("nope.key").unwrap_err(),
        LookupError::UnknownKey
    );
    for bad in ["", "Display.x", "a..b", "display.<id>.scale"] {
        assert_eq!(
            registry.resolve(bad).unwrap_err(),
            LookupError::InvalidKey,
            "{bad}"
        );
    }
    assert_eq!(registry.defs().len(), 2);
    assert_eq!(registry.panel_defs(Panel::Display).count(), 1);
    assert!(registry.def("display.<id>.scale").is_some());
    assert!(registry.def("display.edp-1.scale").is_none());
    assert_eq!(registry.version(), 1);
}

#[test]
fn exact_declaration_wins_over_template() {
    let night = SettingDef::flag("display.night_light.scale", Panel::Display, false);
    let registry = build(vec![scale(), night]).unwrap();
    let found = registry.resolve("display.night_light.scale").unwrap();
    assert_eq!(found.def.key, "display.night_light.scale");
}

#[test]
fn duplicates_are_rejected() {
    assert_eq!(
        build(vec![theme(), theme()]).unwrap_err(),
        SchemaError::DuplicateKey("appearance.theme")
    );
    assert_eq!(
        build(vec![scale(), scale()]).unwrap_err(),
        SchemaError::DuplicateKey("display.<id>.scale")
    );
}

#[test]
fn malformed_declarations_are_rejected() {
    let bad_key = SettingDef::flag("Bad Key", Panel::Power, false);
    assert_eq!(
        build(vec![bad_key]).unwrap_err(),
        SchemaError::InvalidKey("Bad Key")
    );

    let bad_default = SettingDef::int("a.b", Panel::Power, (0, 10, 1), 11);
    assert!(matches!(
        build(vec![bad_default]).unwrap_err(),
        SchemaError::InvalidDefault("a.b", ValueError::OutOfRange)
    ));
    let wrong_type = SettingDef::new("a.b", Panel::Power, ValueType::Bool, Value::Int(1));
    assert!(matches!(
        build(vec![wrong_type]).unwrap_err(),
        SchemaError::InvalidDefault("a.b", ValueError::TypeMismatch { .. })
    ));

    for range in [(5, 1, 1), (0, 10, 0)] {
        let def = SettingDef::int("a.b", Panel::Power, range, 5);
        assert_eq!(
            build(vec![def]).unwrap_err(),
            SchemaError::InvalidRange("a.b")
        );
    }
    for range in [(1.0, 0.0, 0.0), (0.0, f64::INFINITY, 0.0), (0.0, 1.0, -1.0)] {
        let def = SettingDef::float("a.b", Panel::Power, range, 0.5);
        assert_eq!(
            build(vec![def]).unwrap_err(),
            SchemaError::InvalidRange("a.b")
        );
    }
    for choices in [&[][..], &["a", "a"][..], &["Not Valid"][..]] {
        let def = SettingDef::new(
            "a.b",
            Panel::Power,
            ValueType::Enum(choices),
            Value::text("a"),
        );
        assert_eq!(
            build(vec![def]).unwrap_err(),
            SchemaError::InvalidChoices("a.b")
        );
    }
    let zero = theme().risk(Risk::ConfirmRevert { seconds: 0 });
    assert_eq!(
        build(vec![zero]).unwrap_err(),
        SchemaError::InvalidRisk("appearance.theme")
    );
}

#[test]
fn instance_kind_and_scope_must_fit_the_key() {
    // A per-display key must be remembered per display combination.
    let user_scoped = scale().scope(Scope::User);
    assert_eq!(
        build(vec![user_scoped]).unwrap_err(),
        SchemaError::InstanceMismatch("display.<id>.scale")
    );
    // A per-app key cannot be remembered per display combination.
    let app = SettingDef::flag("notifications.app.<id>.sound", Panel::Notifications, true)
        .scope(Scope::DisplayCombination);
    assert_eq!(
        build(vec![app]).unwrap_err(),
        SchemaError::InstanceMismatch("notifications.app.<id>.sound")
    );
    // Display-combination scope is reserved for the display panel.
    let wrong_panel = theme().scope(Scope::DisplayCombination);
    assert_eq!(
        build(vec![wrong_panel]).unwrap_err(),
        SchemaError::InstanceMismatch("appearance.theme")
    );
    let mode = SettingDef::choice("display.mode", Panel::Display, &["extend"], "extend")
        .scope(Scope::DisplayCombination);
    assert!(build(vec![mode]).is_ok());
    let mut forced = theme();
    forced.instance = InstanceKind::App;
    assert!(build(vec![forced]).is_err());
}

#[test]
fn migrations_must_ascend_and_stay_within_the_version() {
    let step = |to_version| Migration {
        to_version,
        steps: vec![MigrationStep::Remove { key: "old.key" }],
    };
    let ok = Registry::builder()
        .version(3)
        .migration(step(2))
        .migration(step(3))
        .build()
        .unwrap();
    assert_eq!(ok.version(), 3);
    assert_eq!(ok.migrations().len(), 2);
    for (version, list) in [(3, vec![3, 2]), (2, vec![3]), (3, vec![1]), (3, vec![2, 2])] {
        let mut builder = Registry::builder().version(version);
        for to_version in list {
            builder = builder.migration(step(to_version));
        }
        assert!(matches!(
            builder.build(),
            Err(SchemaError::InvalidMigration(_))
        ));
    }
    assert_eq!(Registry::builder().version(0).build().unwrap().version(), 1);
}

#[test]
fn errors_describe_themselves() {
    let errors = [
        SchemaError::InvalidKey("k"),
        SchemaError::DuplicateKey("k"),
        SchemaError::InvalidDefault("k", ValueError::OutOfRange),
        SchemaError::InvalidRange("k"),
        SchemaError::InvalidChoices("k"),
        SchemaError::InstanceMismatch("k"),
        SchemaError::InvalidRisk("k"),
        SchemaError::InvalidMigration(2),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
    }
    let text = SettingDef::text("a.t", Panel::Region, TextFormat::Time, "25:00");
    assert!(build(vec![text]).is_err());
}
