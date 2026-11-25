//! Checks the service definitions and the bus policy shipped in
//! `rootfs/etc/ultar/` against the real parsers: a typo there would
//! otherwise only show on a booted device.

use crate::args::{parse_arguments, IdentitySource};
use crate::budget;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ultar_bus::{Identity, Pattern, Policy};

fn etc() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar")
}

/// The keys `ultar-init` accepts in a service file (`deny_unknown_fields`
/// in `system/core/ultar-init/src/spec.rs`).
const INIT_KEYS: [&str; 24] = [
    "name",
    "exec",
    "args",
    "env",
    "restart",
    "after",
    "oneshot",
    "console",
    "compat",
    "workdir",
    "manual",
    "editions",
    "profiles",
    "cgroup",
    "user",
    "group",
    "groups",
    "rights",
    "slice",
    "dirs",
    "public_dirs",
    "notify",
    "start_timeout_ms",
    "stop_timeout_ms",
];

struct Service {
    exec: String,
    args: Vec<String>,
    after: Vec<String>,
    restart: String,
    cgroup: BTreeMap<String, String>,
    user: Option<i64>,
    groups: Vec<i64>,
    notify: bool,
    rights: bool,
}

fn service(name: &str) -> Service {
    let text =
        std::fs::read_to_string(etc().join("services").join(format!("{name}.toml"))).unwrap();
    let table: toml::Table = text.parse().unwrap();
    for key in table.keys() {
        assert!(
            INIT_KEYS.contains(&key.as_str()),
            "{name}: unknown key {key}"
        );
    }
    let strings = |key: &str| -> Vec<String> {
        table.get(key).map_or_else(Vec::new, |value| {
            let items = value.as_array().unwrap().iter();
            items
                .map(|item| item.as_str().unwrap().to_owned())
                .collect()
        })
    };
    let cgroup = table["cgroup"].as_table().unwrap();
    Service {
        exec: table["exec"].as_str().unwrap().to_owned(),
        args: strings("args"),
        after: strings("after"),
        restart: table["restart"].as_str().unwrap().to_owned(),
        cgroup: cgroup
            .iter()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_owned()))
            .collect(),
        user: table.get("user").and_then(toml::Value::as_integer),
        groups: table.get("groups").map_or_else(Vec::new, |value| {
            let items = value.as_array().unwrap().iter();
            items.map(|item| item.as_integer().unwrap()).collect()
        }),
        notify: table.get("notify").and_then(toml::Value::as_bool) == Some(true),
        rights: table.contains_key("rights"),
    }
}

/// User ids of the services with own bus sockets (their group ids are the same).
const IDS: [(&str, i64); 27] = [
    ("bus", 801),
    ("logd", 802),
    ("settingsd", 803),
    ("devd", 804),
    ("launcher", 805),
    ("authd", 813),
    ("lock", 814),
    ("sysdialog", 815),
    ("shell", 816),
    ("displayd", 817),
    ("accountd", 818),
    ("powerd", 819),
    ("sessiond", 820),
    ("notifyd", 821),
    ("calendard", 822),
    ("audiod", 823),
    ("mediad", 824),
    ("captured", 825),
    ("netd", 826),
    ("btd", 827),
    ("volumed", 828),
    ("calld", 829),
    ("mesh", 833),
    ("agentd", 834),
    ("llmbus", 835),
    ("llmcloud", 837),
    ("installer", 840),
];

/// The system services of docs/integration/system-services.md (own socket, own log socket).
const SYSTEM_SERVICES: [&str; 16] = [
    "authd",
    "displayd",
    "accountd",
    "powerd",
    "sessiond",
    "notifyd",
    "calendard",
    "mediad",
    "captured",
    "netd",
    "btd",
    "volumed",
    "mesh",
    "agentd",
    "llmbus",
    "llmcloud",
];

fn uid_of(name: &str) -> i64 {
    IDS.iter().find(|(service, _)| *service == name).unwrap().1
}

/// The value after `option` in `args`.
fn value<'a>(args: &'a [String], option: &str) -> &'a str {
    let index = args.iter().position(|arg| arg == option).unwrap();
    &args[index + 1]
}

#[test]
fn the_four_services_are_ordered_restarted_and_limited() {
    let expected = [
        ("logd", "/usr/bin/ultar-logd", &[][..]),
        ("bus", "/usr/bin/ultar-busd", &["logd"]),
        ("settingsd", "/usr/bin/ultar-settingsd", &["bus"]),
        ("devd", "/usr/bin/ultar-devd", &["bus"]),
    ];
    for (name, exec, after) in expected {
        let file = service(name);
        assert_eq!(file.exec, exec);
        assert_eq!(file.after, after, "{name}");
        assert_eq!(file.restart, "always", "{name}");
        // Every service has a memory and a process limit in its group.
        for limit in ["memory.max", "pids.max"] {
            assert!(file.cgroup.contains_key(limit), "{name}: {limit}");
        }
        // No secret on any command line.
        assert!(!file
            .args
            .iter()
            .any(|arg| arg == "--token" || arg.contains("token=")));
        // Each runs as its own user, without kernel rights, and reports
        // readiness; root is neither needed nor declared.
        assert_eq!(file.user, Some(uid_of(name)), "{name}");
        assert!(!file.rights && file.notify, "{name}");
        assert!(!file.args.iter().any(|arg| arg == "--allow-root"), "{name}");
        // Every service has a policy and nobody runs with --allow-all.
        assert!(!file.args.iter().any(|arg| arg == "--allow-all"), "{name}");
        assert_eq!(value(&file.args, "--policy"), "/etc/ultar/bus-policy.toml");
    }
}

/// Every socket of the broker: a service's is open to its group only (and
/// busd is a member of it), the administrator's and init's to root only.
/// Returns identity → path.
fn check_sockets(parsed: &crate::args::Arguments, bus: &Service) -> BTreeMap<String, PathBuf> {
    let mut sockets = BTreeMap::new();
    for listener in &parsed.listeners {
        if listener.source == IdentitySource::Apps {
            // the apps' socket: open to every app uid (each one is its own identity by its
            // kernel credentials), bounded, outside the services' directory, with the
            // launcher's map of the app range
            assert_eq!(listener.access.mode, Some(0o666));
            assert_eq!(listener.access.group, None);
            assert!(listener.access.connections.is_some());
            assert!(listener.path.starts_with("/run/ultar/appbus/"));
            assert_eq!(parsed.app_map_owner, Some(805));
            assert_eq!(parsed.app_uids, Some((20000, 49999)));
            assert!(parsed
                .app_map
                .as_ref()
                .is_some_and(|m| m.starts_with("/run/ultar/launcher/")));
            continue;
        }
        let IdentitySource::Fixed(identity) = &listener.source else {
            panic!("only fixed sockets besides the apps' socket");
        };
        assert!(Identity::new(identity).is_ok());
        sockets.insert(identity.clone(), listener.path.clone());
        // A service's socket is open to its group only (and busd is a member
        // of it); the administrator's and init's sockets to root only.
        let service = identity.strip_prefix("system.").unwrap();
        let service = if service == "busd" { "bus" } else { service };
        match IDS.iter().find(|(name, _)| *name == service) {
            Some((_, gid)) => {
                assert_eq!(listener.access.mode, Some(0o660), "{identity}");
                assert_eq!(
                    listener.access.group.map(i64::from),
                    Some(*gid),
                    "{identity}"
                );
                assert!(bus.groups.contains(gid), "{identity}");
            }
            None => {
                assert_eq!(listener.access.mode, Some(0o600), "{identity}");
                assert_eq!(listener.access.group, None, "{identity}");
            }
        }
    }
    sockets
}

#[test]
fn the_broker_definition_parses_and_gives_each_service_its_own_private_socket() {
    let bus = service("bus");
    let parsed = parse_arguments(bus.args.iter().cloned(), None).unwrap();
    assert!(!parsed.allow_root && !parsed.allow_all);
    assert_eq!(
        parsed.log_socket,
        Some(PathBuf::from("/run/ultar/log/busd.sock"))
    );
    let sockets = check_sockets(&parsed, &bus);
    // Each service connects to the socket of its own identity.
    let settingsd = service("settingsd");
    assert_eq!(
        Path::new(value(&settingsd.args, "--socket")),
        sockets["system.settingsd"]
    );
    let devd = service("devd");
    assert_eq!(
        Path::new(value(&devd.args, "--bus")),
        sockets["system.devd"]
    );
    let logd = service("logd");
    assert_eq!(
        Path::new(value(&logd.args, "--bus")),
        sockets["system.logd"]
    );
    assert_eq!(
        sockets["system.admin"],
        Path::new("/run/ultar/bus/system.sock")
    );
    assert_eq!(
        sockets["system.init"],
        Path::new("/run/ultar/bus/init.sock")
    );
    let launcher = service("launcher");
    assert_eq!(
        Path::new(value(&launcher.args, "--log-socket")),
        Path::new("/run/ultar/log/launcher.sock")
    );
    // The authentication service, the lock screen, the system dialogs and the shell.
    for (name, option, identity) in [
        ("authd", "--socket", "system.authd"),
        ("lock", "--bus", "system.lock"),
        ("sysdialog", "--bus", "system.sysdialog"),
        ("desktop", "--bus", "system.shell"),
    ] {
        assert_eq!(
            Path::new(value(&service(name).args, option)),
            sockets[identity],
            "{name}"
        );
    }
    // One per service of IDS but the broker itself, plus the administrator's,
    // init's and the modem service's (ril: root on the phone, no group) – and one
    // apps' socket, whose peers are named by the launcher's app map.
    assert_eq!(sockets.len(), IDS.len() + 2);
    let apps = parsed
        .listeners
        .iter()
        .filter(|l| l.source == IdentitySource::Apps);
    assert_eq!(apps.count(), 1);
}

#[test]
fn each_service_logs_to_the_socket_named_after_it() {
    let logd = service("logd");
    // Each service writes its log to the socket the log service names it by.
    for (service_name, option, identity) in [
        ("bus", "--log-socket", "system.busd"),
        ("settingsd", "--log-socket", "system.settingsd"),
        ("devd", "--log-socket", "system.devd"),
        ("launcher", "--log-socket", "system.launcher"),
        ("authd", "--log-socket", "system.authd"),
    ] {
        let path = value(&service(service_name).args, option).to_owned();
        let listen = format!("{path}={identity}");
        let index = logd.args.iter().position(|arg| *arg == listen).unwrap();
        // Open to the writer's group, of which logd is a member.
        let gid = uid_of(service_name);
        assert_eq!(
            logd.args[index + 1..index + 5],
            ["--mode", "0660", "--group", &gid.to_string()]
        );
        assert!(logd.groups.contains(&gid));
    }
}

#[test]
fn the_display_and_account_services_reach_their_own_sockets() {
    let bus = service("bus");
    let parsed = parse_arguments(bus.args.iter().cloned(), None).unwrap();
    let sockets = check_sockets(&parsed, &bus);
    let logd = service("logd");
    for name in SYSTEM_SERVICES {
        let identity = format!("system.{name}");
        let identity = identity.as_str();
        let file = service(name);
        let option = if file.args.iter().any(|a| a == "--bus") {
            "--bus"
        } else {
            "--socket"
        };
        assert_eq!(Path::new(value(&file.args, option)), sockets[identity]);
        // Its log socket is open to its group only; logd is a member.
        let listen = format!("{}={identity}", value(&file.args, "--log-socket"));
        let index = logd.args.iter().position(|arg| *arg == listen).unwrap();
        let gid = uid_of(name);
        assert_eq!(
            logd.args[index + 1..index + 5],
            ["--mode", "0660", "--group", &gid.to_string()]
        );
        assert!(logd.groups.contains(&gid));
    }
}

fn id(name: &str) -> Identity {
    Identity::new(name).unwrap()
}

/// Review round 3, R3-B1: the connection bounds of all sockets together fit
/// the broker's descriptor and thread limits and the group's `pids.max`, so
/// the peers of one socket cannot use up what another socket needs.
#[test]
fn the_sum_of_the_connection_bounds_fits_the_limits() {
    let bus = service("bus");
    let parsed = parse_arguments(bus.args.iter().cloned(), None).unwrap();
    let mut builder = ultar_bus::Broker::builder(Policy::allow_all()).config(budget::config());
    for listener in &parsed.listeners {
        // Every socket names its own bound; none relies on the default.
        assert!(listener.access.connections.is_some(), "{:?}", listener.path);
        let auth = std::sync::Arc::new(ultar_bus::FixedIdentity::new(id("x")));
        builder = builder.listen_with_access(&listener.path, auth, listener.access);
    }
    let needs = builder.needs();
    assert_eq!(budget::check(needs), Ok(()));
    let total = budget::with_reserve(needs);
    let pids: usize = bus.cgroup["pids.max"].parse().unwrap();
    assert!(total.threads <= pids, "{total:?} > pids.max {pids}");
    // The administrator's socket has a bound of its own.
    let admin = parsed
        .listeners
        .iter()
        .find(|listener| listener.source == IdentitySource::Fixed("system.admin".into()))
        .unwrap();
    assert!(admin.access.connections.unwrap() >= 8);
}

#[test]
fn the_policy_grants_each_identity_exactly_what_it_needs() {
    let policy = Policy::load(&etc().join("bus-policy.toml")).unwrap();
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
fn the_policy_grants_nothing_beyond_that() {
    let policy = Policy::load(&etc().join("bus-policy.toml")).unwrap();
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
    // Apps: each app id gets exactly what its pages need; an app without a rule only `app.*`.
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
    let policy = Policy::load(&etc().join("bus-policy.toml")).unwrap();
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
