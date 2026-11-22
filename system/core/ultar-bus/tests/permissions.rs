//! Authentication and the default-deny policy.

mod common;

use common::{client, client_with_token, id, socket_path, token_broker, QUIET, WAIT};
use std::sync::Arc;
use std::thread;
use ultar_bus::messages::settings::{Changed, Get, GetReply, Set, SetReply, SettingValue};
use ultar_bus::messages::shell::{Mode, ShellMode};
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, CredentialIdentity, ErrorCode, FixedIdentity,
    PeerCredentials, Policy, PolicyErrorKind, StaticCredentials,
};

/// The policy file used by most tests: a system service, a system app with
/// more rights and a third-party app with fewer – all through the same API.
const POLICY: &str = r#"
[[rule]]
identity  = "system.settingsd"
register  = ["settings"]
publish   = ["settings.changed"]

[[rule]]
identity  = "system.devd"
publish   = ["devices.*"]

# every app may read settings and follow changes
[[rule]]
identity  = "app.*"
call      = ["settings.get"]
subscribe = ["settings.changed", "shell.mode"]

# the phone app is a system app: same API, more rights
[[rule]]
identity  = "app.phone"
call      = ["settings.set"]
subscribe = ["devices.*"]
publish   = ["shell.mode"]
"#;

fn policy() -> Policy {
    Policy::from_toml_str(POLICY).unwrap()
}

fn denied<T: std::fmt::Debug>(result: Result<T, BusError>) {
    assert!(
        matches!(result, Err(BusError::Remote(ErrorCode::PermissionDenied))),
        "expected PermissionDenied, got {result:?}"
    );
}

fn spawn_settings(service: &Client) -> thread::JoinHandle<()> {
    let endpoint = service.register("settings").unwrap();
    thread::spawn(move || {
        endpoint.run(|request| {
            let _ = match request.method() {
                "get" => request.respond::<Get>(|_| Ok(GetReply { value: None })),
                _ => request.respond::<Set>(|_| Ok(SetReply { changed: true })),
            };
        });
    })
}

#[test]
fn built_in_reader_agrees_with_a_full_toml_parser() {
    let samples = [
        POLICY,
        "",
        "# only a comment\n",
        "[[rule]]\nidentity = '*'\ncall = [\n  \"a.b\", # one\n  'c.*',\n]\nsubscribe = []\n",
    ];
    for text in samples {
        let reference: Policy = toml::from_str(text).unwrap();
        assert_eq!(Policy::from_toml_str(text).unwrap(), reference, "{text}");
    }
    // Both reject the same mistakes.
    for text in [
        "[[rule]]\ncall = [\"a\"]\n",
        "[[rule]]\nidentity = \"a\"\nunknown = []\n",
    ] {
        assert!(toml::from_str::<Policy>(text).is_err());
        assert!(Policy::from_toml_str(text).is_err());
    }
    // A policy written by serde is read back by the built-in reader.
    let written = toml::to_string(&policy()).unwrap();
    assert_eq!(Policy::from_toml_str(&written).unwrap(), policy());
    assert_eq!(
        Policy::from_toml_str("[[rule]]").unwrap_err().kind,
        PolicyErrorKind::MissingIdentity
    );
}

#[test]
fn unknown_token_is_rejected_at_the_handshake() {
    let (_broker, path) = token_broker("auth", policy());
    assert!(matches!(
        client_with_token(&path, "app.intruder"),
        Err(BusError::AuthFailed)
    ));
    let no_token = Client::connect(&path, ClientOptions::named("anonymous"));
    assert!(matches!(no_token, Err(BusError::AuthFailed)));
    assert_eq!(
        client_with_token(&path, "app.phone").unwrap().identity(),
        "app.phone"
    );
}

#[test]
fn registering_a_service_needs_permission() {
    let (_broker, path) = token_broker("register", policy());
    let app = client_with_token(&path, "app.thirdparty").unwrap();
    denied(app.register("settings").map(|_| ()));
    let daemon = client_with_token(&path, "system.settingsd").unwrap();
    denied(daemon.register("power").map(|_| ()));
    assert!(daemon.register("settings").is_ok());
}

#[test]
fn system_app_may_call_more_than_a_third_party_app() {
    let (_broker, path) = token_broker("call", policy());
    let daemon = client_with_token(&path, "system.settingsd").unwrap();
    let _worker = spawn_settings(&daemon);
    let phone = client_with_token(&path, "app.phone").unwrap();
    let other = client_with_token(&path, "app.thirdparty").unwrap();
    let get = Get {
        key: "locale.language".into(),
    };
    let set = Set {
        key: "locale.language".into(),
        value: SettingValue::Text("ar".into()),
    };

    assert!(phone.call(&get, WAIT).is_ok());
    assert!(other.call(&get, WAIT).is_ok());
    assert!(phone.call(&set, WAIT).unwrap().changed);
    denied(other.call(&set, WAIT));
    // The daemon itself has no right to call anything.
    denied(daemon.call(&get, WAIT));
    // Denied even for services that do not exist: nothing is revealed.
    denied(other.request::<_, ()>("secrets", "dump", &(), WAIT));
}

#[test]
fn publishing_needs_permission() {
    let (_broker, path) = token_broker("publish", policy());
    let daemon = client_with_token(&path, "system.settingsd").unwrap();
    let phone = client_with_token(&path, "app.phone").unwrap();
    let other = client_with_token(&path, "app.thirdparty").unwrap();
    let changes = other.subscribe::<Changed>().unwrap();
    let forged = Changed {
        key: "security.lock".into(),
        value: SettingValue::Flag(false),
    };

    denied(other.publish(&forged));
    denied(phone.publish(&forged));
    // Unconfirmed publications are dropped silently.
    other.publish_unconfirmed(&forged).unwrap();
    assert!(matches!(
        changes.recv_timeout(QUIET),
        Err(BusError::Timeout)
    ));

    let real = Changed {
        key: "display.scale".into(),
        value: SettingValue::Decimal(1.5),
    };
    daemon.publish(&real).unwrap();
    assert_eq!(changes.recv_timeout(WAIT).unwrap(), real);
    // The system app may switch the shell mode, the third-party app may not.
    phone
        .publish(&ShellMode {
            mode: Mode::Desktop,
        })
        .unwrap();
    denied(other.publish(&ShellMode { mode: Mode::Mobile }));
}

#[test]
fn subscribing_needs_permission_for_the_whole_pattern() {
    let (_broker, path) = token_broker("subscribe", policy());
    let phone = client_with_token(&path, "app.phone").unwrap();
    let other = client_with_token(&path, "app.thirdparty").unwrap();
    assert!(other.subscribe_pattern("settings.changed").is_ok());
    assert!(other.subscribe_pattern("shell.mode").is_ok());
    denied(other.subscribe_pattern("settings.*").map(|_| ()));
    denied(other.subscribe_pattern("*").map(|_| ()));
    denied(other.subscribe_pattern("devices.capabilities").map(|_| ()));
    assert!(phone.subscribe_pattern("devices.*").is_ok());
    assert!(phone.subscribe_pattern("devices.usb.*").is_ok());
    assert!(phone.subscribe_pattern("devices.capabilities").is_ok());
    denied(phone.subscribe_pattern("*").map(|_| ()));
}

#[test]
fn empty_policy_denies_everything() {
    let (_broker, path) = token_broker("deny-all", Policy::deny_all());
    let phone = client_with_token(&path, "app.phone").unwrap();
    denied(phone.register("anything").map(|_| ()));
    denied(phone.request::<_, ()>("settings", "get", &(), WAIT));
    denied(phone.publish(&ShellMode { mode: Mode::Mobile }));
    denied(phone.subscribe::<ShellMode>().map(|_| ()));
}

#[test]
fn policy_can_be_replaced_while_running() {
    let (broker, path) = token_broker("reload", Policy::deny_all());
    let phone = client_with_token(&path, "app.phone").unwrap();
    denied(phone.publish(&ShellMode { mode: Mode::Mobile }));
    broker.set_policy(policy());
    phone.publish(&ShellMode { mode: Mode::Mobile }).unwrap();
    broker.set_policy(Policy::deny_all());
    denied(phone.publish(&ShellMode { mode: Mode::Mobile }));
}

#[test]
fn each_listener_gives_its_own_identity() {
    let system_path = socket_path("listener-system");
    let app_path = socket_path("listener-app");
    let _broker = Broker::builder(policy())
        .listen_with_mode(
            &system_path,
            Arc::new(FixedIdentity::new(id("system.devd"))),
            0o600,
        )
        .listen(
            &app_path,
            Arc::new(FixedIdentity::new(id("app.thirdparty"))),
        )
        .start()
        .unwrap();
    let devd = client(&system_path, "devd");
    let app = client(&app_path, "app");
    assert_eq!(devd.identity(), "system.devd");
    assert_eq!(app.identity(), "app.thirdparty");
    assert!(devd
        .publish_raw("devices.capabilities", 1, true, Vec::new(), true)
        .is_ok());
    denied(app.publish_raw("devices.capabilities", 1, true, Vec::new(), true));
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&system_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn kernel_credentials_map_to_identities() {
    let credentials = PeerCredentials {
        uid: 1000,
        gid: 1000,
        pid: None,
    };
    let known_path = socket_path("cred-known");
    let known =
        CredentialIdentity::new(StaticCredentials(credentials)).with_uid(1000, id("app.phone"));
    let _known_broker = Broker::builder(policy())
        .listen(&known_path, Arc::new(known))
        .start()
        .unwrap();
    assert_eq!(client(&known_path, "phone").identity(), "app.phone");

    let unknown_path = socket_path("cred-unknown");
    let unknown =
        CredentialIdentity::new(StaticCredentials(credentials)).with_uid(0, id("system.devd"));
    let _unknown_broker = Broker::builder(policy())
        .listen(&unknown_path, Arc::new(unknown))
        .start()
        .unwrap();
    let rejected = Client::connect(&unknown_path, ClientOptions::named("nobody"));
    assert!(matches!(rejected, Err(BusError::AuthFailed)));
}
