mod combos;
mod confirm;

use super::testkit::*;
use super::*;
use crate::access::Caller;
use crate::apply::ApplyError;
use crate::schema::ValueError;
use ultar_bus::messages::settings::{ItemKind, RiskInfo, ScopeInfo, SettingValue};
use ultar_profile::{Capability, CapabilitySet};

#[test]
fn defaults_are_returned_before_anything_is_stored() {
    let kit = Kit::new("core-defaults");
    let core = kit.open();
    assert_eq!(core.get("appearance.theme"), Ok(Value::text("dark")));
    assert_eq!(core.get("bar.layout"), Ok(Value::text("bottom_bar")));
    assert_eq!(
        core.get("notifications.app.mail.sound"),
        Ok(Value::Bool(true))
    );
    assert_eq!(core.next_deadline(), None);
    assert_eq!(core.store_stats().writes, 0);
    assert!(format!("{core:?}").contains("Core"));
    assert_eq!(core.registry().version(), crate::seed::SCHEMA_VERSION);
}

#[test]
fn unknown_keys_and_type_mismatches_are_rejected_with_typed_errors() {
    let kit = Kit::new("core-reject");
    let mut core = kit.open();
    let who = admin();
    assert_eq!(core.get("no.such.key"), Err(SettingsError::UnknownKey));
    assert_eq!(core.get("Bad Key"), Err(SettingsError::InvalidKey));
    assert_eq!(
        core.set(&who, "no.such.key", Value::Bool(true)),
        Err(SettingsError::UnknownKey)
    );
    assert_eq!(
        core.reset(&who, "no.such.key"),
        Err(SettingsError::UnknownKey)
    );
    assert!(matches!(
        core.set(&who, "appearance.theme", Value::Bool(true)),
        Err(SettingsError::Value(ValueError::TypeMismatch {
            expected: "enum",
            found: "bool"
        }))
    ));
    assert_eq!(
        core.set(&who, "appearance.theme", Value::text("pink")),
        Err(SettingsError::Value(ValueError::UnknownChoice))
    );
    assert_eq!(
        core.set(&who, "appearance.font_size", Value::Int(500)),
        Err(SettingsError::Value(ValueError::OutOfRange))
    );
    assert_eq!(
        core.set(&who, "appearance.font_size", Value::Int(105)),
        Err(SettingsError::Value(ValueError::OffStep))
    );
    assert_eq!(
        core.set(&who, "display.night_light.start", Value::text("8pm")),
        Err(SettingsError::Value(ValueError::BadFormat {
            format: "time"
        }))
    );
    // Nothing was applied, announced or queued for writing.
    assert_eq!(kit.applier.total(), 0);
    assert!(core.take_events().is_empty());
    assert_eq!(core.next_deadline(), None);
}

#[test]
fn set_applies_announces_and_persists() {
    let kit = Kit::new("core-set");
    let mut core = kit.open();
    assert_eq!(
        core.set(&user(), "appearance.theme", Value::text("light")),
        Ok(true)
    );
    assert_eq!(core.get("appearance.theme"), Ok(Value::text("light")));
    assert_eq!(
        core.take_events(),
        [changed("appearance.theme", Value::text("light"))]
    );
    assert_eq!(
        kit.applier.last("appearance.theme"),
        Some(Value::text("light"))
    );
    // The same value again changes nothing.
    assert_eq!(
        core.set(&user(), "appearance.theme", Value::text("light")),
        Ok(false)
    );
    assert!(core.take_events().is_empty());
    assert_eq!(kit.applier.total(), 1);
    // Written once the delay has passed, and still there after a restart.
    assert_eq!(core.next_deadline(), Some(DELAY));
    kit.clock.advance(DELAY);
    core.tick().unwrap();
    assert_eq!(core.store_stats().writes, 1);
    drop(core);
    assert_eq!(kit.open().get("appearance.theme"), Ok(Value::text("light")));
}

#[test]
fn many_sets_are_written_once() {
    let kit = Kit::new("core-coalesce");
    let mut core = kit.open();
    for size in (80..=200).step_by(10) {
        core.set(&user(), "appearance.font_size", Value::Int(size))
            .unwrap();
    }
    core.set(&user(), "appearance.theme", Value::text("light"))
        .unwrap();
    core.set(&admin(), "power.mode", Value::text("saver"))
        .unwrap();
    assert_eq!(core.store_stats().writes, 0);
    kit.clock.advance(DELAY);
    core.tick().unwrap();
    // One write per touched file (user and system), not one per set.
    assert_eq!(core.store_stats().writes, 2);
    assert_eq!(core.store_stats().changes, 15);
}

#[test]
fn a_whole_number_is_accepted_for_a_fractional_setting() {
    let kit = Kit::new("core-coerce");
    let mut core = kit.open_with_panel();
    core.set(&admin(), "display.boe-ne135.refresh_rate", Value::Int(120))
        .unwrap();
    assert_eq!(
        core.get("display.boe-ne135.refresh_rate"),
        Ok(Value::Float(120.0))
    );
}

#[test]
fn reset_returns_to_the_default_and_unpins_it() {
    let kit = Kit::new("core-reset");
    let mut core = kit.open();
    core.set(&user(), "appearance.theme", Value::text("light"))
        .unwrap();
    core.take_events();
    assert_eq!(core.reset(&user(), "appearance.theme"), Ok(true));
    assert_eq!(core.get("appearance.theme"), Ok(Value::text("dark")));
    assert_eq!(
        core.take_events(),
        [changed("appearance.theme", Value::text("dark"))]
    );
    assert_eq!(core.reset(&user(), "appearance.theme"), Ok(false));
    // A stored value equal to the default is unpinned without an event.
    core.set(&user(), "appearance.theme", Value::text("light"))
        .unwrap();
    core.set(&user(), "appearance.theme", Value::text("dark"))
        .unwrap();
    core.take_events();
    assert_eq!(core.reset(&user(), "appearance.theme"), Ok(false));
    assert!(core.take_events().is_empty());
    core.flush().unwrap();
    let text = std::fs::read_to_string(kit.dir.path().join("user-anas.toml")).unwrap();
    assert!(!text.contains("appearance.theme"), "{text}");
}

#[test]
fn admin_only_settings_need_an_administrator() {
    let kit = Kit::new("core-admin");
    let mut core = kit.open();
    let zone = Value::text("Europe/Berlin");
    assert_eq!(
        core.set(&user(), "datetime.timezone.name", zone.clone()),
        Err(SettingsError::NotPermitted)
    );
    assert_eq!(
        core.reset(&user(), "datetime.timezone.name"),
        Err(SettingsError::NotPermitted)
    );
    assert_eq!(
        core.get("datetime.timezone.name"),
        Ok(Value::text("Asia/Damascus"))
    );
    assert_eq!(
        core.set(&admin(), "datetime.timezone.name", zone.clone()),
        Ok(true)
    );
    assert_eq!(core.get("datetime.timezone.name"), Ok(zone));
    // Everybody may read; a plain user may change plain settings.
    assert_eq!(
        core.set(&user(), "datetime.clock_format", Value::text("h12")),
        Ok(true)
    );
    assert_eq!(
        core.set_displays(&user(), &[panel()]).unwrap_err(),
        SettingsError::NotPermitted
    );
}

#[test]
fn settings_of_missing_hardware_cannot_be_changed() {
    let mut kit = Kit::new("core-caps");
    kit.capabilities.flags = CapabilitySet::of(&[Capability::Keyboard]);
    let mut core = kit.open();
    assert_eq!(
        core.set(&admin(), "power.charge_limit.enabled", Value::Bool(true)),
        Err(SettingsError::Unavailable)
    );
    assert_eq!(
        core.reset(&admin(), "privacy.camera"),
        Err(SettingsError::Unavailable)
    );
    // After the device service reports a battery the same call works.
    core.set_capabilities(laptop_capabilities());
    assert_eq!(
        core.set(&admin(), "power.charge_limit.enabled", Value::Bool(true)),
        Ok(true)
    );
}

#[test]
fn failing_applier_leaves_the_setting_as_it_was() {
    let kit = Kit::new("core-apply-fail");
    let mut core = kit.open();
    kit.applier.fail_next(ApplyError::Unsupported);
    assert_eq!(
        core.set(&user(), "appearance.theme", Value::text("light")),
        Err(SettingsError::Apply(ApplyError::Unsupported))
    );
    assert_eq!(core.get("appearance.theme"), Ok(Value::text("dark")));
    assert!(core.take_events().is_empty());
    assert_eq!(core.next_deadline(), None);
    assert_eq!(
        core.set(&user(), "appearance.theme", Value::text("light")),
        Ok(true)
    );
}

#[test]
fn per_app_settings_take_any_app_but_are_bounded() {
    let kit = Kit::new("core-apps");
    let mut core = kit.open();
    assert_eq!(
        core.set(&user(), "notifications.app.mail.sound", Value::Bool(false)),
        Ok(true)
    );
    assert_eq!(
        core.get("notifications.app.mail.sound"),
        Ok(Value::Bool(false))
    );
    assert_eq!(
        core.get("notifications.app.chat.sound"),
        Ok(Value::Bool(true))
    );
    for index in 0..crate::store::MAX_VALUES {
        let key = format!("notifications.app.a{index}.banner");
        if core.set(&user(), &key, Value::Bool(false)).is_err() {
            assert_eq!(index, crate::store::MAX_VALUES - 1);
            assert_eq!(
                core.set(&user(), &key, Value::Bool(false)),
                Err(SettingsError::Limit)
            );
            // Existing entries can still be changed and removed.
            assert_eq!(
                core.set(&user(), "notifications.app.mail.sound", Value::Bool(true)),
                Ok(true)
            );
            assert_eq!(core.reset(&user(), "notifications.app.a0.banner"), Ok(true));
            assert_eq!(core.set(&user(), &key, Value::Bool(false)), Ok(true));
            return;
        }
    }
    panic!("the limit was never reached");
}

fn keys_of(core: &Core, caller: &Caller, panel: Option<&str>) -> Vec<String> {
    core.list(caller, panel)
        .unwrap()
        .into_iter()
        .flat_map(|panel| panel.items.into_iter().map(|item| item.key))
        .collect()
}

#[test]
fn list_shows_panels_and_items_the_device_has() {
    let kit = Kit::new("core-list");
    let core = kit.open_with_panel();
    let panels = core.list(&admin(), None).unwrap();
    let ids: Vec<&str> = panels.iter().map(|panel| panel.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "display",
            "appearance",
            "desktop_bar",
            "windows",
            "notifications",
            "power",
            "region",
            "date_time",
            "privacy",
            "account"
        ]
    );
    assert_eq!(panels[0].title.key, "settings.panel.display");
    let keys = keys_of(&core, &admin(), None);
    assert!(keys.contains(&"display.boe-ne135.scale".to_owned()));
    assert!(keys.contains(&"power.charge_limit.percent".to_owned()));
    assert!(keys.contains(&"privacy.camera".to_owned()));
    // No GNSS receiver and no sensors on this laptop.
    assert!(!keys.contains(&"privacy.location".to_owned()));
    assert!(!keys.contains(&"privacy.sensors".to_owned()));
    assert!(!keys
        .iter()
        .any(|key| key.contains("<id>") && key.starts_with("display.")));
    assert_eq!(keys_of(&core, &admin(), Some("region")).len(), 11);
    assert_eq!(
        core.list(&admin(), Some("nope")).unwrap_err(),
        SettingsError::BadRequest
    );
}

#[test]
fn without_battery_no_battery_keys_are_listed() {
    let mut kit = Kit::new("core-list-battery");
    kit.capabilities.flags.remove(Capability::Battery);
    let core = kit.open();
    let keys = keys_of(&core, &admin(), Some("power"));
    assert_eq!(
        keys,
        [
            "power.mode",
            "power.screen_dim_after",
            "power.screen_off_after",
            "power.suspend_after_ac",
            "power.button_action",
            "power.lid_action",
        ]
    );
    let with_battery = Kit::new("core-list-battery-2").open();
    assert_eq!(keys_of(&with_battery, &admin(), Some("power")).len(), 12);
}

#[test]
fn without_a_display_the_display_panels_are_hidden() {
    let mut kit = Kit::new("core-list-headless");
    kit.capabilities.display_count = 0;
    let mut core = kit.open();
    let ids: Vec<String> = core
        .list(&admin(), None)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(
        ids,
        [
            "notifications",
            "power",
            "region",
            "date_time",
            "privacy",
            "account"
        ]
    );
    // A display reported by the display service counts, too.
    core.set_displays(&admin(), &[projector()]).unwrap();
    let ids: Vec<String> = core
        .list(&admin(), None)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids.len(), 10);
    assert_eq!(core.capabilities().display_count, 1);
}

#[test]
fn list_items_carry_everything_a_generic_editor_needs() {
    let kit = Kit::new("core-list-items");
    let mut core = kit.open_with_panel();
    core.set(&admin(), "display.boe-ne135.scale", Value::Int(150))
        .unwrap();
    let display = core.list(&user(), Some("display")).unwrap().remove(0);
    let scale = display
        .items
        .iter()
        .find(|i| i.key == "display.boe-ne135.scale")
        .unwrap();
    assert_eq!(scale.label.key, "settings.display.scale.label");
    assert_eq!(scale.description.key, "settings.display.scale.desc");
    assert_eq!(
        scale.kind,
        ItemKind::Integer {
            min: 100,
            max: 300,
            step: 25
        }
    );
    assert_eq!(scale.value, SettingValue::Integer(150));
    // 192 dpi: the device-computed default is 200 %.
    assert_eq!(scale.default, SettingValue::Integer(200));
    assert_eq!(scale.risk, RiskInfo::ConfirmRevert { seconds: 15 });
    assert_eq!(scale.scope, ScopeInfo::DisplayCombination);
    assert!(scale.pending && scale.writable && !scale.template);
    let mode = display
        .items
        .iter()
        .find(|i| i.key == "display.mode")
        .unwrap();
    let ItemKind::Choice { choices } = &mode.kind else {
        panic!("display.mode must be a choice");
    };
    assert_eq!(choices[1].id, "mirror");
    assert_eq!(choices[1].label.key, "settings.display.mode.choice.mirror");

    let time = core.list(&user(), Some("date_time")).unwrap().remove(0);
    let zone = time
        .items
        .iter()
        .find(|i| i.key == "datetime.timezone.name")
        .unwrap();
    assert!(!zone.writable);
    assert_eq!(
        zone.kind,
        ItemKind::Text {
            format: "time_zone".into()
        }
    );
    assert_eq!(zone.risk, RiskInfo::AdminOnly);
    let admin_view = core.list(&admin(), Some("date_time")).unwrap().remove(0);
    assert!(admin_view.items.iter().all(|item| item.writable));
}

#[test]
fn per_app_items_are_listed_as_family_plus_known_apps() {
    let kit = Kit::new("core-list-apps");
    let mut core = kit.open();
    core.set(&user(), "notifications.app.mail.sound", Value::Bool(false))
        .unwrap();
    let panel = core.list(&user(), Some("notifications")).unwrap().remove(0);
    let sound: Vec<(&str, bool)> = panel
        .items
        .iter()
        .filter(|item| item.key.ends_with(".sound"))
        .map(|item| (item.key.as_str(), item.template))
        .collect();
    assert_eq!(
        sound,
        [
            ("notifications.app.<id>.sound", true),
            ("notifications.app.mail.sound", false)
        ]
    );
    assert_eq!(panel.items.len(), 6 + 5 + 1);
}

#[test]
fn the_list_stays_small_enough_for_one_bus_frame() {
    let kit = Kit::new("core-list-bound");
    let mut core = kit.open();
    for index in 0..MAX_LISTED_APPS + 40 {
        let key = format!("notifications.app.a{index}.sound");
        core.set(&user(), &key, Value::Bool(false)).unwrap();
    }
    let panel = core.list(&user(), Some("notifications")).unwrap().remove(0);
    assert_eq!(panel.items.len(), 6 + 5 + MAX_LISTED_APPS);
    let reply = ultar_bus::messages::settings::ListReply {
        panels: core.list(&user(), None).unwrap(),
    };
    let bytes = ultar_bus::encode(&reply).unwrap().len();
    assert!(bytes < 256 * 1024, "{bytes} bytes");
    // An app beyond the listed ones is still reachable by its key.
    let last = format!("notifications.app.a{}.sound", MAX_LISTED_APPS + 39);
    assert_eq!(core.get(&last), Ok(Value::Bool(false)));
}

#[test]
fn every_listed_key_can_be_read_and_reset() {
    let kit = Kit::new("core-list-all");
    let mut core = kit.open_with_panel();
    for item in core
        .list(&admin(), None)
        .unwrap()
        .into_iter()
        .flat_map(|p| p.items)
    {
        if item.template {
            continue;
        }
        assert_eq!(
            core.get(&item.key).map(SettingValue::from),
            Ok(item.value),
            "{}",
            item.key
        );
        assert_eq!(core.reset(&admin(), &item.key), Ok(false), "{}", item.key);
    }
}
