//! `login.access`: the sign-in screen's narrow right. `system.lock` may call only
//! `settings.set_accessibility` / `settings.accessibility`; the service accepts only keys
//! under `accessibility.` with the schema's types, and every other key is refused.

mod common;

use common::{Rig, WAIT};
use ultar_bus::messages::settings::{
    Accessibility, Get, Set, SetAccessibility, SettingValue, ACCESSIBILITY_PREFIX,
};
use ultar_bus::{BusError, Client, ErrorCode};

fn set_access(client: &Client, key: &str, value: SettingValue) -> Result<bool, BusError> {
    let call = SetAccessibility {
        key: key.into(),
        value,
    };
    client.call(&call, WAIT).map(|r| r.changed)
}

fn refused<T: std::fmt::Debug>(result: Result<T, BusError>, expected: ErrorCode) {
    match result {
        Err(BusError::Remote(code)) => assert_eq!(code, expected),
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

#[test]
fn the_lock_screen_changes_accessibility_and_nothing_else() {
    let rig = Rig::new("svc-access");
    let _server = rig.start();
    let (lock, viewer) = (rig.client("system.lock"), rig.client("app.viewer"));

    // The four switches of the sign-in screen exist and start off.
    let values = lock.call(&Accessibility {}, WAIT).unwrap().values;
    let keys: Vec<&str> = values.iter().map(|(k, _)| k.as_str()).collect();
    for key in [
        "accessibility.large_text",
        "accessibility.high_contrast",
        "accessibility.screen_reader",
        "accessibility.on_screen_keyboard",
    ] {
        assert!(keys.contains(&key), "{key} listed");
    }
    // Besides the switches only the input languages (the sign-in's language chip).
    let (access, keyboard): (Vec<_>, Vec<_>) = values
        .iter()
        .partition(|(k, _)| k.starts_with(ACCESSIBILITY_PREFIX));
    // Every switch starts off (the panel also holds numbers and a choice).
    assert!(access
        .iter()
        .filter(|(_, v)| matches!(v, SettingValue::Flag(_)))
        .all(|(_, v)| *v == SettingValue::Flag(false)));
    assert_eq!(
        keyboard,
        vec![
            &(
                "keyboard.input_languages".to_owned(),
                SettingValue::List(Vec::new())
            ),
            &(
                "region.keyboard.layouts".to_owned(),
                SettingValue::Text("ar_en".into())
            ),
            &(
                "region.keyboard.active".to_owned(),
                SettingValue::Text("ar".into())
            ),
        ]
    );
    // It may store the active input language (a choice of the schema), not the list.
    let active = "region.keyboard.active";
    assert!(set_access(&lock, active, SettingValue::Text("en".into())).unwrap());
    let seen = viewer
        .call(&Get { key: active.into() }, WAIT)
        .unwrap()
        .value;
    assert_eq!(seen, Some(SettingValue::Text("en".into())));
    assert!(matches!(
        set_access(&lock, active, SettingValue::Text("fr".into())),
        Err(BusError::Remote(ErrorCode::Service(_)))
    ));

    // A switch takes effect at once for everybody.
    let key = "accessibility.high_contrast";
    assert!(set_access(&lock, key, SettingValue::Flag(true)).unwrap());
    let seen = viewer.call(&Get { key: key.into() }, WAIT).unwrap().value;
    assert_eq!(seen, Some(SettingValue::Flag(true)));
    assert!(!set_access(&lock, key, SettingValue::Flag(true)).unwrap());

    // The schema's type is kept.
    refused(
        set_access(&lock, key, SettingValue::Text("yes".into())),
        ErrorCode::Service(ultar_bus::messages::settings::code::TYPE_MISMATCH),
    );
    other_keys_are_refused(&lock, &viewer);
}

/// Other keys – plain, admin-only, malformed, unknown – are refused before the schema; the
/// lock screen has no general right and the narrow one is not an app's.
fn other_keys_are_refused(lock: &Client, viewer: &Client) {
    let key = "accessibility.high_contrast";
    for other in [
        "appearance.theme",
        "datetime.timezone.name",
        "accessibility",
        "accessibility.",
        "accessibility..large_text",
        "xaccessibility.large_text",
        "network.proxy",
        "region.keyboard.layouts",
        "region.language",
    ] {
        refused(
            set_access(lock, other, SettingValue::Text("light".into())),
            ErrorCode::PermissionDenied,
        );
    }
    let theme = viewer
        .call(
            &Get {
                key: "appearance.theme".into(),
            },
            WAIT,
        )
        .unwrap()
        .value;
    assert_eq!(theme, Some(SettingValue::Text("dark".into())), "unchanged");

    // The lock screen has no general right: the broker refuses settings.set / get.
    let set = Set {
        key: key.into(),
        value: SettingValue::Flag(false),
    };
    refused(lock.call(&set, WAIT), ErrorCode::PermissionDenied);
    refused(
        lock.call(
            &Get {
                key: "appearance.theme".into(),
            },
            WAIT,
        ),
        ErrorCode::PermissionDenied,
    );
    // And an app without the right cannot use the narrow method either.
    refused(
        set_access(viewer, key, SettingValue::Flag(false)),
        ErrorCode::PermissionDenied,
    );
}
