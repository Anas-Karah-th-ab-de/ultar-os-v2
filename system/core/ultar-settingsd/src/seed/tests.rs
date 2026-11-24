use super::*;
use crate::i18n;
use crate::schema::{DefaultValue, Panel, Risk, Scope};
use crate::value::Value;
use std::collections::BTreeSet;
use ultar_i18n::{Catalog, Lang};
use ultar_profile::Capability;

#[test]
fn seed_builds_and_covers_all_ten_areas() {
    let registry = registry().unwrap();
    assert_eq!(registry.version(), SCHEMA_VERSION);
    let counts: Vec<(Panel, usize)> = Panel::ALL
        .iter()
        .map(|panel| (*panel, registry.panel_defs(*panel).count()))
        .collect();
    assert_eq!(
        counts,
        [
            (Panel::Display, 22),
            (Panel::Appearance, 13),
            (Panel::DesktopBar, 15),
            (Panel::Windows, 8),
            (Panel::Notifications, 11),
            (Panel::Power, 12),
            (Panel::Region, 9),
            (Panel::DateTime, 5),
            (Panel::Privacy, 14),
            (Panel::Account, 6),
        ]
    );
    assert_eq!(registry.defs().len(), 115);
}

fn required_keys() -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = definitions()
        .iter()
        .flat_map(|def| def.i18n_keys())
        .collect();
    keys.extend(Panel::ALL.iter().map(|panel| panel.title_key()));
    keys
}

#[test]
fn every_seed_key_has_label_description_and_choices_in_all_three_languages() {
    let required = required_keys();
    for (lang, source) in i18n::sources() {
        let catalog = Catalog::parse(source).unwrap();
        for key in &required {
            let message = catalog.get(key);
            assert!(message.is_some(), "{} lacks {key}", lang.code());
            let text = message.unwrap().format(lang, &[]);
            assert!(!text.trim().is_empty(), "{} has empty {key}", lang.code());
        }
    }
}

#[test]
fn catalogs_hold_no_setting_text_without_a_declaration() {
    let required = required_keys();
    let catalog = Catalog::parse(i18n::AR).unwrap();
    let stray: Vec<&str> = catalog
        .keys()
        .filter(|key| key.starts_with("settings.") && !required.contains(*key))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

#[test]
fn the_i18n_checker_finds_the_three_catalogs_complete_and_equal() {
    let sources: Vec<(String, String)> = i18n::sources()
        .iter()
        .map(|(lang, source)| (format!("{}.ftl", lang.code()), (*source).to_owned()))
        .collect();
    let report = ultar_i18n::check_dir::check_sources(&sources).unwrap();
    assert_eq!(report.files, ["ar.ftl", "de.ftl", "en.ftl"]);
    assert!(report.is_clean(), "{:?}", report.problems);
    // The same check on the files as shipped.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let report = ultar_i18n::check_dir::check_directory(&dir).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
}

#[test]
fn arabic_labels_are_arabic_and_translations_differ_from_the_keys() {
    let arabic = Catalog::parse(i18n::AR).unwrap();
    let latin_only = [
        "settings.display.hdr.label",
        "settings.region.format.paper.choice.a4",
        "settings.region.format.paper.choice.letter",
    ];
    for def in definitions() {
        let key = def.label_key();
        if latin_only.contains(&key.as_str()) {
            continue;
        }
        let text = arabic.get(&key).unwrap().format(Lang::Ar, &[]);
        assert!(ultar_i18n::contains_rtl(&text), "{key} = {text}");
    }
}

#[test]
fn risky_display_and_network_changes_ask_to_keep_within_15_seconds() {
    let registry = registry().unwrap();
    let keep = Risk::ConfirmRevert { seconds: 15 };
    for key in [
        "display.mode",
        "display.primary",
        "display.mirror_displays",
        "display.single_display",
        "display.<id>.resolution",
        "display.<id>.scale",
        "display.<id>.refresh_rate",
        "display.<id>.orientation",
        "display.<id>.position_x",
        "display.<id>.position_y",
        "privacy.network",
    ] {
        assert_eq!(registry.def(key).unwrap().risk, keep, "{key}");
    }
    let confirm_count = registry
        .defs()
        .iter()
        .filter(|def| def.risk == keep)
        .count();
    assert_eq!(confirm_count, 11);
    for key in [
        "display.<id>.brightness",
        "display.night_light.enabled",
        "appearance.theme",
    ] {
        assert_eq!(registry.def(key).unwrap().risk, Risk::Safe, "{key}");
    }
}

#[test]
fn scopes_and_admin_settings() {
    let registry = registry().unwrap();
    let scope = |key: &str| registry.def(key).unwrap().scope;
    assert_eq!(scope("display.<id>.scale"), Scope::DisplayCombination);
    assert_eq!(scope("display.mode"), Scope::DisplayCombination);
    assert_eq!(scope("display.night_light.enabled"), Scope::User);
    assert_eq!(scope("appearance.theme"), Scope::User);
    assert_eq!(scope("power.mode"), Scope::System);
    assert_eq!(scope("power.show_battery_percent"), Scope::User);
    assert_eq!(scope("datetime.clock_format"), Scope::User);
    let admin: Vec<&str> = registry
        .defs()
        .iter()
        .filter(|def| def.risk == Risk::AdminOnly)
        .map(|def| def.key)
        .collect();
    assert_eq!(
        admin,
        [
            "datetime.automatic",
            "datetime.timezone.automatic",
            "datetime.timezone.name",
            "datetime.hardware_clock",
            "privacy.allow_unsigned_packages",
            "account.server",
        ]
    );
    assert!(admin.iter().all(|key| scope(key) == Scope::System));
}

#[test]
fn defaults_follow_the_specification() {
    let registry = registry().unwrap();
    let default = |key: &str| registry.def(key).unwrap().default.base().clone();
    assert_eq!(default("bar.layout"), Value::text("bottom_bar"));
    assert_eq!(default("bar.icon_alignment"), Value::text("center"));
    assert_eq!(default("appearance.theme"), Value::text("dark"));
    assert_eq!(default("appearance.accent_color"), Value::text("gold"));
    assert_eq!(default("display.mode"), Value::text("extend"));
    assert_eq!(default("account.calls"), Value::Bool(false));
    assert_eq!(default("power.charge_limit.percent"), Value::Int(80));
    assert_eq!(default("power.mode"), Value::text("balanced"));
    assert_eq!(
        default("privacy.allow_unsigned_packages"),
        Value::Bool(false)
    );
    assert_eq!(default("region.language"), Value::text("ar"));
    assert_eq!(
        default("account.server"),
        Value::text("ultar.code-craft.tech")
    );
    let computed: Vec<&str> = registry
        .defs()
        .iter()
        .filter(|def| matches!(def.default, DefaultValue::Computed { .. }))
        .map(|def| def.key)
        .collect();
    assert_eq!(
        computed,
        [
            "display.single_display",
            "display.primary",
            "display.<id>.position_x",
            "display.<id>.position_y",
            "display.<id>.resolution",
            "display.<id>.scale",
            "display.<id>.refresh_rate",
        ]
    );
}

#[test]
fn capability_requirements() {
    let registry = registry().unwrap();
    let battery: Vec<&str> = registry
        .defs()
        .iter()
        .filter(|def| def.requires.contains(Capability::Battery))
        .map(|def| def.key)
        .collect();
    assert_eq!(
        battery,
        [
            "power.charge_limit.enabled",
            "power.charge_limit.percent",
            "power.suspend_after_battery",
            "power.auto_saver.enabled",
            "power.auto_saver.percent",
            "power.show_battery_percent",
        ]
    );
    let needs = |key: &str, capability| registry.def(key).unwrap().requires.contains(capability);
    assert!(needs("privacy.camera", Capability::Camera));
    assert!(needs("privacy.microphone", Capability::AudioIn));
    assert!(needs("privacy.location", Capability::Gnss));
    assert!(needs("privacy.sensors", Capability::Sensors));
    assert!(registry.def("privacy.network").unwrap().requires.is_empty());
}

#[test]
fn every_seed_key_resolves_with_a_sample_instance() {
    let registry = registry().unwrap();
    for def in registry.defs() {
        let key = def.key.replace("<id>", "sample-1");
        let found = registry.resolve(&key).unwrap();
        assert_eq!(found.def.key, def.key);
        // Keys are bus topic names too.
        assert!(ultar_bus::is_valid_name(&key), "{key}");
    }
}
