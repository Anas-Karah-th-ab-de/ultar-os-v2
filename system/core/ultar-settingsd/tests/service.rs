//! The settings service over the real bus: contracts, permissions,
//! confirm-or-revert, capability filtering, store reports.

mod common;

use common::{Rig, DELAY, WAIT};
use std::fmt::Debug;
use ultar_bus::messages::devices;
use ultar_bus::messages::settings::{
    code, Changed, Confirm, Get, ListRequest, PendingConfirmation, PendingOutcome, PendingResolved,
    Reset, Revert, Set, SetDisplays, SettingValue, Shutdown, StoreNoticeKind, StoreReport,
};
use ultar_bus::{BusError, Client, ErrorCode};

fn get(client: &Client, key: &str) -> Result<SettingValue, BusError> {
    let reply = client.call(&Get { key: key.into() }, WAIT)?;
    Ok(reply
        .value
        .expect("the service always answers with a value"))
}

fn set(client: &Client, key: &str, value: SettingValue) -> Result<bool, BusError> {
    let call = Set {
        key: key.into(),
        value,
    };
    client.call(&call, WAIT).map(|reply| reply.changed)
}

fn text(value: &str) -> SettingValue {
    SettingValue::Text(value.into())
}

fn refused<T: Debug>(result: Result<T, BusError>, expected: ErrorCode) {
    match result {
        Err(BusError::Remote(code)) => assert_eq!(code, expected),
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

fn listed_keys(client: &Client, panel: Option<&str>) -> Vec<String> {
    let request = ListRequest {
        panel: panel.map(str::to_owned),
    };
    let reply = client.call(&request, WAIT).unwrap();
    reply
        .panels
        .into_iter()
        .flat_map(|panel| panel.items.into_iter().map(|item| item.key))
        .collect()
}

#[test]
fn get_set_reset_and_change_events() {
    let rig = Rig::new("svc-basic");
    let _server = rig.start();
    let app = rig.client("app.settings");
    let watcher = rig.client("app.viewer");
    let changes = watcher.subscribe::<Changed>().unwrap();

    assert_eq!(get(&app, "appearance.theme").unwrap(), text("dark"));
    assert!(set(&app, "appearance.theme", text("light")).unwrap());
    let change = changes.recv_timeout(WAIT).unwrap();
    assert_eq!(change.key, "appearance.theme");
    assert_eq!(change.value, text("light"));
    // The same value again is no change and causes no event: the next
    // event the watcher sees is the reset below.
    assert!(!set(&app, "appearance.theme", text("light")).unwrap());
    assert_eq!(get(&watcher, "appearance.theme").unwrap(), text("light"));

    let reset = Reset {
        key: "appearance.theme".into(),
    };
    assert!(app.call(&reset, WAIT).unwrap().changed);
    assert_eq!(changes.recv_timeout(WAIT).unwrap().value, text("dark"));
    assert!(!app.call(&reset, WAIT).unwrap().changed);
    assert_eq!(
        rig.applier.last("appearance.theme"),
        Some(ultar_settingsd::value::Value::text("dark"))
    );
}

#[test]
fn unknown_keys_and_wrong_values_are_refused_with_typed_codes() {
    let rig = Rig::new("svc-codes");
    let _server = rig.start();
    let app = rig.client("app.settings");
    refused(
        get(&app, "no.such.key"),
        ErrorCode::Service(code::UNKNOWN_KEY),
    );
    refused(
        get(&app, "display.ghost.scale"),
        ErrorCode::Service(code::UNKNOWN_INSTANCE),
    );
    refused(
        set(&app, "appearance.theme", SettingValue::Flag(true)),
        ErrorCode::Service(code::TYPE_MISMATCH),
    );
    refused(
        set(&app, "appearance.font_size", SettingValue::Integer(1000)),
        ErrorCode::Service(code::OUT_OF_RANGE),
    );
    refused(
        set(&app, "appearance.theme", text("pink")),
        ErrorCode::Service(code::INVALID_VALUE),
    );
    refused(
        set(&app, "privacy.location", SettingValue::Flag(true)),
        ErrorCode::Service(code::UNAVAILABLE),
    );
    refused(
        app.call(
            &Confirm {
                key: Some("display.mode".into()),
            },
            WAIT,
        ),
        ErrorCode::Service(code::NOT_PENDING),
    );
    let bad_panel = ListRequest {
        panel: Some("nope".into()),
    };
    refused(
        app.call(&bad_panel, WAIT),
        ErrorCode::Service(code::BAD_REQUEST),
    );
    // The administrator marker is not a method.
    refused(
        app.request::<(), ()>("settings", "admin", &(), WAIT),
        ErrorCode::UnknownMethod,
    );
    // A payload that is not the contract.
    refused(
        app.request::<u8, ()>("settings", "get", &7, WAIT),
        ErrorCode::VersionMismatch,
    );
}

#[test]
fn bus_policy_and_risk_class_decide_who_may_set_what() {
    let rig = Rig::new("svc-perm");
    let _server = rig.start();
    let (settings, game, viewer) = (
        rig.client("app.settings"),
        rig.client("app.game"),
        rig.client("app.viewer"),
    );
    let zone = "datetime.timezone.name";

    // The viewer has no right to call set at all: the broker refuses.
    refused(
        set(&viewer, "appearance.theme", text("light")),
        ErrorCode::PermissionDenied,
    );
    refused(viewer.call(&Shutdown, WAIT), ErrorCode::PermissionDenied);
    assert_eq!(get(&viewer, zone).unwrap(), text("Asia/Damascus"));

    // The game may set plain settings, but nothing that is admin-only.
    assert!(set(&game, "appearance.theme", text("light")).unwrap());
    refused(
        set(&game, zone, text("Europe/Berlin")),
        ErrorCode::PermissionDenied,
    );
    refused(
        game.call(&Reset { key: zone.into() }, WAIT),
        ErrorCode::PermissionDenied,
    );
    refused(
        game.call(&SetDisplays::default(), WAIT),
        ErrorCode::PermissionDenied,
    );
    assert_eq!(get(&game, zone).unwrap(), text("Asia/Damascus"));

    // The settings app is an administrator by the bus policy.
    assert!(set(&settings, zone, text("Europe/Berlin")).unwrap());
    assert_eq!(get(&viewer, zone).unwrap(), text("Europe/Berlin"));

    // The same list, but only the administrator may write the time zone.
    let writable = |client: &Client| {
        let request = ListRequest {
            panel: Some("date_time".into()),
        };
        let panel = client.call(&request, WAIT).unwrap().panels.remove(0);
        panel
            .items
            .into_iter()
            .find(|item| item.key == zone)
            .unwrap()
            .writable
    };
    assert!(writable(&settings));
    assert!(!writable(&game));
}

#[test]
fn confirm_or_revert_over_the_bus() {
    let rig = Rig::new("svc-confirm");
    let server = rig.start();
    rig.report_panel();
    let game = rig.client("app.game");
    let shell = rig.client("app.viewer");
    let pending = shell.subscribe::<PendingConfirmation>().unwrap();
    let resolved = shell.subscribe::<PendingResolved>().unwrap();
    let key = "display.boe-ne135.scale";

    // 1. Nobody answers "keep?": the old value returns after 15 s.
    assert!(set(&game, key, SettingValue::Integer(150)).unwrap());
    let ask = pending.recv_timeout(WAIT).unwrap();
    assert_eq!((ask.key.as_str(), ask.seconds), (key, 15));
    assert_eq!(ask.value, SettingValue::Integer(150));
    assert_eq!(ask.previous, SettingValue::Integer(200));
    assert_eq!(get(&game, key).unwrap(), SettingValue::Integer(150));
    rig.advance(14);
    assert!(server.tick());
    assert_eq!(get(&game, key).unwrap(), SettingValue::Integer(150));
    rig.advance(1);
    assert!(server.tick());
    let end = resolved.recv_timeout(WAIT).unwrap();
    assert_eq!(end.outcome, PendingOutcome::TimedOut);
    assert_eq!(end.value, SettingValue::Integer(200));
    assert_eq!(get(&game, key).unwrap(), SettingValue::Integer(200));

    // 2. Confirmed: the value stays for good.
    set(&game, key, SettingValue::Integer(150)).unwrap();
    assert_eq!(
        game.call(
            &Confirm {
                key: Some(key.into())
            },
            WAIT
        )
        .unwrap()
        .resolved,
        1
    );
    assert_eq!(
        resolved.recv_timeout(WAIT).unwrap().outcome,
        PendingOutcome::Kept
    );
    rig.advance(3600);
    assert!(server.tick());
    assert_eq!(get(&game, key).unwrap(), SettingValue::Integer(150));

    // 3. Taken back on request.
    set(&game, key, SettingValue::Integer(300)).unwrap();
    assert_eq!(game.call(&Revert { key: None }, WAIT).unwrap().resolved, 1);
    assert_eq!(
        resolved.recv_timeout(WAIT).unwrap().outcome,
        PendingOutcome::Reverted
    );
    assert_eq!(get(&game, key).unwrap(), SettingValue::Integer(150));
}

#[test]
fn service_restart_during_a_pending_change_reverts_and_tells_subscribers() {
    let rig = Rig::new("svc-restart");
    let server = rig.start();
    rig.report_panel();
    let game = rig.client("app.game");
    set(&game, "display.mode", text("mirror")).unwrap();
    assert_eq!(get(&game, "display.mode").unwrap(), text("mirror"));
    server.stop();
    rig.applier.clear_calls();

    let shell = rig.client("app.viewer");
    let resolved = shell.subscribe::<PendingResolved>().unwrap();
    let changes = shell.subscribe::<Changed>().unwrap();
    let _server = rig.start();
    let end = resolved.recv_timeout(WAIT).unwrap();
    assert_eq!(end.key, "display.mode");
    assert_eq!(end.outcome, PendingOutcome::RevertedAtStart);
    assert_eq!(changes.recv_timeout(WAIT).unwrap().value, text("extend"));
    assert_eq!(get(&game, "display.mode").unwrap(), text("extend"));
    assert_eq!(
        rig.applier.last("display.mode"),
        Some(ultar_settingsd::value::Value::text("extend"))
    );
}

#[test]
fn values_are_written_once_after_the_delay_and_survive_a_restart() {
    let rig = Rig::new("svc-persist");
    let server = rig.start();
    let app = rig.client("app.settings");
    for size in [90, 110, 120, 130] {
        set(&app, "appearance.font_size", SettingValue::Integer(size)).unwrap();
    }
    let file = rig.store_dir().join("user-anas.toml");
    assert!(!file.exists());
    rig.clock.advance(DELAY);
    assert!(server.tick());
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(
        written.contains("\"appearance.font_size\" = 130"),
        "{written}"
    );
    server.stop();
    let _server = rig.start();
    assert_eq!(
        get(&app, "appearance.font_size").unwrap(),
        SettingValue::Integer(130)
    );
}

#[test]
fn stop_writes_what_is_still_waiting() {
    let rig = Rig::new("svc-stop");
    let server = rig.start();
    let app = rig.client("app.settings");
    set(&app, "appearance.theme", text("light")).unwrap();
    // No tick, no time passes: the write delay has not run out.
    server.stop();
    let _server = rig.start();
    assert_eq!(get(&app, "appearance.theme").unwrap(), text("light"));
}

#[test]
fn shutdown_call_flushes_and_ends_the_service() {
    let rig = Rig::new("svc-shutdown");
    let server = rig.start();
    let app = rig.client("app.settings");
    set(&app, "appearance.theme", text("light")).unwrap();
    refused(
        rig.client("app.game").call(&Shutdown, WAIT),
        ErrorCode::PermissionDenied,
    );
    app.call(&Shutdown, WAIT).unwrap();
    server.wait();
    let written = std::fs::read_to_string(rig.store_dir().join("user-anas.toml")).unwrap();
    assert!(written.contains("light"));
    // The broker may or may not have noticed yet that the service left.
    match get(&app, "appearance.theme") {
        Err(BusError::Remote(ErrorCode::UnknownService | ErrorCode::ServiceGone)) => {}
        other => panic!("the service must be gone, got {other:?}"),
    }
}

fn capability(id: &str) -> devices::Capability {
    devices::Capability {
        id: id.into(),
        availability: devices::Availability::Available,
        note: None,
    }
}

/// What a device without display and without other reported capabilities lists.
const HEADLESS_PANELS: [&str; 19] = [
    "notifications",
    "power",
    "region",
    "date_time",
    "privacy",
    "account",
    "search",
    "apps",
    "printers",
    "accessibility",
    "sharing",
    "users",
    "storage",
    "updates",
    "backup",
    "mesh",
    "assistant",
    "cells",
    "emergency",
];

#[test]
fn list_follows_the_capabilities_the_device_service_reports() {
    let rig = Rig::new("svc-caps");
    let server = rig.start();
    let app = rig.client("app.settings");
    let devd = rig.client("system.devd");
    assert!(listed_keys(&app, Some("power")).contains(&"power.charge_limit.percent".to_owned()));

    // The battery is gone (a desktop): no battery keys are listed.
    let report = devices::Capabilities {
        revision: 1,
        capabilities: vec![capability("display"), capability("camera")],
    };
    devd.publish(&report).unwrap();
    assert!(server.stats().wait_capability_reports(1, WAIT));
    let power = listed_keys(&app, Some("power"));
    assert!(
        !power
            .iter()
            .any(|key| key.contains("battery") || key.contains("charge")),
        "{power:?}"
    );
    assert!(!power.contains(&"power.auto_saver.enabled".to_owned()));
    assert!(power.contains(&"power.mode".to_owned()));
    refused(
        set(&app, "power.charge_limit.enabled", SettingValue::Flag(true)),
        ErrorCode::Service(code::UNAVAILABLE),
    );

    // A device without any display loses the display panels.
    let headless = devices::Capabilities {
        revision: 2,
        capabilities: vec![devices::Capability {
            availability: devices::Availability::Unavailable,
            ..capability("display")
        }],
    };
    devd.publish(&headless).unwrap();
    assert!(server.stats().wait_capability_reports(2, WAIT));
    let request = ListRequest::default();
    let ids: Vec<String> = app
        .call(&request, WAIT)
        .unwrap()
        .panels
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, HEADLESS_PANELS);
    assert_eq!(server.stats().lost_events(), 0);
    assert!(server.stats().requests() >= 4);
}

#[test]
fn damaged_file_is_reported_to_late_subscribers() {
    let rig = Rig::new("svc-report");
    std::fs::create_dir_all(rig.store_dir()).unwrap();
    std::fs::write(rig.store_dir().join("system.toml"), "not a settings file").unwrap();
    let _server = rig.start();
    // Subscribed after the service started: the report is retained.
    let shell = rig.client("app.viewer");
    let report = shell
        .subscribe::<StoreReport>()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap();
    assert_eq!(report.notices.len(), 1);
    let notice = &report.notices[0];
    assert_eq!(notice.file, "system.toml");
    assert_eq!(notice.kind, StoreNoticeKind::Quarantined);
    assert_eq!(notice.text.key, "settingsd.store.quarantined");
    assert!(rig.store_dir().join(&notice.detail).exists());
    // Defaults are in use and the service works.
    assert_eq!(get(&shell, "power.mode").unwrap(), text("balanced"));
}
