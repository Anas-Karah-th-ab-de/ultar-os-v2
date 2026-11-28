//! Checks the bus policy shipped in `rootfs/etc/ultar/bus-policy.toml` against the
//! real parser: each identity gets what it needs and nothing beyond that.

use crate::service_files_tests::etc;
use ultar_bus::{Identity, Pattern, Policy};

/// The shipped bus policy.
fn policy() -> Policy {
    Policy::load(&etc().join("bus-policy.toml")).unwrap()
}

fn id(name: &str) -> Identity {
    Identity::new(name).unwrap()
}

#[test]
fn the_policy_grants_each_identity_exactly_what_it_needs() {
    let policy = policy();
    let topic = |name: &str| Pattern::parse(name).unwrap();
    let (logd, settingsd, devd, admin) = (
        id("system.logd"),
        id("system.settingsd"),
        id("system.devd"),
        id("system.admin"),
    );
    // What each service needs.
    assert!(policy.may_register(&logd, "log"));
    assert!(policy.may_register(&settingsd, "settings"));
    assert!(policy.may_register(&devd, "devices"));
    for name in [
        "settings.changed",
        "settings.pending",
        "settings.pending_resolved",
        "settings.store_report",
    ] {
        assert!(policy.may_publish(&settingsd, name), "{name}");
    }
    assert!(policy.may_subscribe(&settingsd, &topic("devices.capabilities")));
    for name in ["devices.capabilities", "devices.driver_state"] {
        assert!(policy.may_publish(&devd, name), "{name}");
    }
    for (service, method) in [
        ("settings", "set"),
        ("settings", "admin"),
        ("devices", "switch"),
        ("devices", "admin"),
        ("log", "query"),
        ("log", "read_all"),
    ] {
        assert!(
            policy.may_call(&admin, service, method),
            "{service}.{method}"
        );
    }
}

#[test]
fn the_policy_grants_the_services_nothing_beyond_that() {
    let policy = policy();
    let topic = |name: &str| Pattern::parse(name).unwrap();
    let (logd, settingsd, devd, admin) = (
        id("system.logd"),
        id("system.settingsd"),
        id("system.devd"),
        id("system.admin"),
    );
    for service in [&logd, &settingsd, &devd] {
        for name in ["log", "settings", "devices", "notify"] {
            let own = (service == &logd && name == "log")
                || (service == &settingsd && name == "settings")
                || (service == &devd && name == "devices");
            assert_eq!(policy.may_register(service, name), own, "{service} {name}");
        }
        for (target, method) in [
            ("settings", "set"),
            ("devices", "switch"),
            ("log", "read_all"),
            ("devices", "admin"),
            ("settings", "admin"),
        ] {
            assert!(
                !policy.may_call(service, target, method),
                "{service} {target}.{method}"
            );
        }
    }
    assert!(!policy.may_publish(&logd, "settings.changed"));
    assert!(!policy.may_publish(&settingsd, "devices.capabilities"));
    assert!(!policy.may_publish(&devd, "settings.changed"));
    assert!(!policy.may_publish(&devd, "devices.other"));
    assert!(!policy.may_subscribe(&devd, &topic("settings.changed")));
    assert!(!policy.may_subscribe(&settingsd, &topic("devices.*")));
    assert!(!policy.may_register(&admin, "settings"));
    assert!(!policy.may_publish(&admin, "settings.changed"));
    // Default deny: an identity without rule may do nothing.
    for stranger in [id("app.phone"), id("system.busd"), id("system")] {
        assert!(!policy.may_call(&stranger, "settings", "get"));
        assert!(!policy.may_call(&stranger, "log", "query"));
        assert!(!policy.may_subscribe(&stranger, &topic("settings.changed")));
        assert!(!policy.may_register(&stranger, "log"));
    }
}

/// Apps: each app id gets exactly what its pages need; an app without a rule only `app.*`.
#[test]
fn each_app_gets_what_its_pages_need() {
    let policy = policy();
    let topic = |name: &str| Pattern::parse(name).unwrap();
    let settings = id("app.settings");
    for (service, method) in [
        ("settings", "list"),
        ("settings", "set"),
        ("display", "identify"),
        ("account", "status"),
        ("devices", "list"),
    ] {
        assert!(
            policy.may_call(&settings, service, method),
            "{service}.{method}"
        );
    }
    for name in [
        "settings.changed",
        "net.state",
        "bluetooth.state",
        "audio.state",
        "display.outputs",
        "power.state",
    ] {
        assert!(policy.may_subscribe(&settings, &topic(name)), "{name}");
    }
    let files = id("app.files");
    assert!(policy.may_call(&files, "launcher", "start"));
    assert!(policy.may_subscribe(&files, &topic("volumes.state")));
    let stranger = id("app.some-store-app");
    assert!(policy.may_call(&stranger, "notify", "post"));
}

#[test]
fn no_app_gets_more_than_its_pages_need() {
    let policy = policy();
    let (settings, files, stranger) = (
        id("app.settings"),
        id("app.files"),
        id("app.some-store-app"),
    );
    for app in [
        &settings,
        &files,
        &stranger,
        &id("app.store"),
        &id("app.phone"),
    ] {
        for (service, method) in [
            ("settings", "admin"),
            ("devices", "admin"),
            ("launcher", "admin"),
            ("auth", "verify"),
            ("auth", "admin"),
            ("log", "query"),
            ("init", "control"),
            ("sysdialog", "permission"),
            ("lock", "lock"),
        ] {
            let store_admin = app.as_str() == "app.store" && service == "launcher";
            assert!(
                store_admin || !policy.may_call(app, service, method),
                "{app} {service}.{method}"
            );
        }
        assert!(!policy.may_register(app, "launcher"), "{app}");
        assert!(!policy.may_register(app, "settings"), "{app}");
        assert!(!policy.may_publish(app, "settings.changed"), "{app}");
    }
    for (service, method) in [
        ("settings", "list"),
        ("settings", "set"),
        ("launcher", "start"),
        ("call", "dial"),
        ("net", "join"),
        ("account", "sign_in"),
    ] {
        assert!(
            !policy.may_call(&stranger, service, method),
            "{service}.{method}"
        );
    }
    assert!(!policy.may_call(&files, "settings", "set"));
    assert!(!policy.may_call(&settings, "launcher", "install"));
    assert!(!policy.may_call(&settings, "call", "dial"));
}

#[test]
fn the_service_manager_offers_init_and_only_the_administrator_steers_it() {
    let policy = policy();
    let init = id("system.init");
    assert!(policy.may_register(&init, "init"));
    for other in ["log", "settings", "devices", "update"] {
        assert!(!policy.may_register(&init, other), "{other}");
    }
    let admin = id("system.admin");
    for method in ["status", "control", "admin"] {
        assert!(policy.may_call(&admin, "init", method), "{method}");
    }
    for identity in [
        "app.phone",
        "system.settingsd",
        "system.devd",
        "system.logd",
        "system.init",
    ] {
        assert!(
            !policy.may_call(&id(identity), "init", "status"),
            "{identity}"
        );
        assert!(
            !policy.may_call(&id(identity), "init", "admin"),
            "{identity}"
        );
    }
}

/// Apps from other systems (ultar-foreign): the service may register unsigned
/// `app.foreign` apps with the launcher and ask for an administrator, nothing more.
#[test]
fn the_foreign_service_gets_exactly_its_part() {
    let policy = policy();
    let foreign = id("system.foreign");
    assert!(policy.may_register(&foreign, "foreign"));
    for name in ["foreign.progress", "foreign.changed"] {
        assert!(policy.may_publish(&foreign, name), "{name}");
    }
    for (service, method) in [
        ("launcher", "install_foreign"),
        ("launcher", "uninstall"),
        ("launcher", "foreign"),
        ("sysdialog", "authenticate"),
    ] {
        assert!(
            policy.may_call(&foreign, service, method),
            "{service}.{method}"
        );
    }
    // never the signed path, the administrator's right or a grant
    for (service, method) in [
        ("launcher", "install"),
        ("launcher", "admin"),
        ("launcher", "grant"),
        ("launcher", "start"),
        ("settings", "set"),
        ("net", "connect"),
    ] {
        assert!(
            !policy.may_call(&foreign, service, method),
            "{service}.{method}"
        );
    }
    assert!(!policy.may_register(&foreign, "launcher"));
    assert!(!policy.may_publish(&foreign, "launcher.changed"));
    // only the foreign service holds launcher.foreign and launcher.install_foreign
    // (besides the administrator's tools, which hold launcher.*)
    for other in [
        "system.shell",
        "system.launcher",
        "system.updated",
        "app.store",
        "app.files",
        "app.settings",
    ] {
        let other = id(other);
        assert!(!policy.may_call(&other, "launcher", "foreign"), "{other}");
        assert!(
            !policy.may_call(&other, "launcher", "install_foreign"),
            "{other}"
        );
        assert!(!policy.may_register(&other, "foreign"), "{other}");
    }
}

/// The Files app may only inspect and start an installation, Settings may manage the apps;
/// no other app reaches the foreign service.
#[test]
fn the_callers_of_the_foreign_service_get_exactly_their_part() {
    let policy = policy();
    let topic = |name: &str| Pattern::parse(name).unwrap();
    let files = id("app.files");
    assert!(policy.may_call(&files, "foreign", "inspect"));
    assert!(policy.may_call(&files, "foreign", "install"));
    assert!(policy.may_subscribe(&files, &topic("foreign.progress")));
    for method in ["remove", "set_choices", "runtime_update"] {
        assert!(!policy.may_call(&files, "foreign", method), "{method}");
    }
    let settings = id("app.settings");
    for method in [
        "list",
        "set_choices",
        "remove",
        "runtime_state",
        "runtime_update",
    ] {
        assert!(policy.may_call(&settings, "foreign", method), "{method}");
    }
    for stranger in ["app.some-store-app", "app.foreign.mousepad", "app.phone"] {
        let stranger = id(stranger);
        for method in ["inspect", "install", "list", "remove", "set_choices"] {
            assert!(
                !policy.may_call(&stranger, "foreign", method),
                "{stranger} {method}"
            );
        }
        assert!(!policy.may_subscribe(&stranger, &topic("foreign.progress")));
    }
}
