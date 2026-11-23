//! The update service over a real bus broker, and the `uupdate` client against it.

mod common;

use common::rig::*;
use common::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, Policy,
};
use ultar_update::bootctl::Slot;
use ultar_update::busfront::{serve, BusPublisher};
use ultar_update::contract::{
    Apply, BootPrevious, Cancel, Check, Download, GetPolicy, History, Network, Offline, Phase,
    Progress, Status,
};
use ultar_update::error::UpdateError;

struct BusRig {
    _broker: Broker,
    dir: tempfile::TempDir,
    rig: Rig,
    server: Option<std::thread::JoinHandle<()>>,
    service: Client,
}

fn connect(dir: &Path, identity: &str) -> Client {
    Client::connect(&dir.join(identity), ClientOptions::named(identity)).unwrap()
}

fn bus_rig() -> BusRig {
    let dir = tempfile::tempdir().unwrap();
    let mut builder = Broker::builder(Policy::allow_all());
    for identity in [ADMIN, USER, NET, "system.updated"] {
        let auth = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
        builder = builder.listen(&dir.path().join(identity), auth);
    }
    let broker = builder.start().unwrap();
    let service = connect(dir.path(), "system.updated");
    let device = Device::new();
    let published = Arc::new(Mutex::new(None));
    let (core, cancel) = core_on(
        &device,
        Arc::new(Published(published.clone())),
        Arc::new(BusPublisher(service.clone())),
    );
    let endpoint = service.register("update").unwrap();
    let serving = core.clone();
    let server = std::thread::spawn(move || serve(serving, endpoint));
    let rig = Rig {
        device,
        core,
        cancel,
        published,
        events: Arc::new(Collect::default()),
    };
    BusRig {
        _broker: broker,
        dir,
        rig,
        server: Some(server),
        service,
    }
}

impl Drop for BusRig {
    fn drop(&mut self) {
        self.service.close();
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

fn service_error<T: std::fmt::Debug>(result: Result<T, BusError>) -> &'static str {
    match result {
        Err(BusError::Remote(ErrorCode::Service(code))) => UpdateError::key_of_code(code).unwrap(),
        Err(BusError::Remote(ErrorCode::PermissionDenied)) => "update.error.permission_denied",
        other => panic!("expected a service error, got {other:?}"),
    }
}

/// Waits until the operation that was just accepted has ended and returns its last event.
fn finished(events: &ultar_bus::TypedSubscription<Progress>) -> Progress {
    let mut started = false;
    loop {
        let event = events.recv_timeout(WAIT).expect("progress event");
        if event.phase.busy() {
            started = true;
        } else if started {
            return event;
        }
    }
}

const DENIED: &str = "update.error.permission_denied";
const DOWNLOAD: Download = Download {
    allow_metered: false,
};

/// The clients of one test: an app, the administrator, the network service.
struct Clients {
    user: Client,
    admin: Client,
    net: Client,
    events: ultar_bus::TypedSubscription<Progress>,
}

fn clients(bus: &BusRig) -> Clients {
    let user = connect(bus.dir.path(), USER);
    let events = user.subscribe::<Progress>().unwrap();
    Clients {
        admin: connect(bus.dir.path(), ADMIN),
        net: connect(bus.dir.path(), NET),
        user,
        events,
    }
}

/// Check and download as an app; the download waits for an unmetered network.
fn bus_check_and_download(c: &Clients) {
    let status = c.user.call(&Status {}, WAIT).unwrap();
    assert_eq!(status.current_version, "0.7.0");
    c.user.call(&Check {}, WAIT).unwrap();
    let event = finished(&c.events);
    assert_eq!(
        (event.phase, event.version.as_deref()),
        (Phase::Available, Some("0.7.1"))
    );

    c.user.call(&DOWNLOAD, WAIT).unwrap();
    assert_eq!(
        finished(&c.events).error_key.as_deref(),
        Some("update.error.metered")
    );
    let report = Network { metered: false };
    assert_eq!(service_error(c.user.call(&report, WAIT)), DENIED);
    c.net.call(&report, WAIT).unwrap();
    c.user.call(&DOWNLOAD, WAIT).unwrap();
    assert_eq!(finished(&c.events).phase, Phase::Staged);
}

/// Everything that changes the system is for administrators.
fn bus_admin_only_calls_are_denied(c: &Clients) {
    assert_eq!(service_error(c.user.call(&Apply {}, WAIT)), DENIED);
    assert_eq!(service_error(c.user.call(&Cancel {}, WAIT)), DENIED);
    assert_eq!(service_error(c.user.call(&BootPrevious {}, WAIT)), DENIED);
    let offline = Offline {
        path: "/etc".into(),
        allow_downgrade: true,
    };
    assert_eq!(service_error(c.user.call(&offline, WAIT)), DENIED);
    let policy = c.user.call(&GetPolicy {}, WAIT).unwrap();
    assert_eq!(service_error(c.user.call(&policy, WAIT)), DENIED);
    // Even an administrator cannot name a folder outside the configured ones.
    assert_eq!(service_error(c.admin.call(&offline, WAIT)), DENIED);
}

/// After the install: typed errors for nonsense, and the retained state for late subscribers.
fn bus_after_install(c: &Clients) {
    assert_eq!(
        service_error(c.admin.call(&BootPrevious {}, WAIT)),
        "update.error.busy"
    );
    let history = c.user.call(&History { limit: 5 }, WAIT).unwrap();
    assert_eq!(history.entries[0].kind_key, "update.history.applied");
    assert_eq!(history.entries[0].version.as_deref(), Some("0.7.1"));
    // An unknown method and a malformed payload end with typed errors, not with a hang.
    let unknown: Result<(), BusError> = c.user.request("update", "format_disk", &(), WAIT);
    assert!(matches!(
        unknown,
        Err(BusError::Remote(ErrorCode::UnknownMethod))
    ));
    let malformed: Result<(), BusError> = c.user.request("update", "set_policy", &7u8, WAIT);
    assert!(malformed.is_err());
    // A late subscriber gets the current state at once (retained topic).
    let late = c.admin.subscribe::<Progress>().unwrap();
    assert_eq!(
        late.recv_timeout(WAIT).unwrap().phase,
        Phase::RebootRequired
    );
}

#[test]
fn whole_update_over_the_bus() {
    let bus = bus_rig();
    let new = release((0, 7, 1), 2);
    bus.rig.publish(&new);
    let c = clients(&bus);
    bus_check_and_download(&c);
    bus_admin_only_calls_are_denied(&c);
    assert!(bus.rig.device.slot_b_untouched());

    c.admin.call(&Apply {}, WAIT).unwrap();
    let event = finished(&c.events);
    assert_eq!(event.phase, Phase::RebootRequired);
    assert_eq!(bus.rig.device.images(Slot::B), new.payloads);
    bus_after_install(&c);
}

/// Runs `uupdate <command>` (library entry point) as `identity` in `lang`.
fn uupdate(bus: &BusRig, lang: ultar_i18n::Lang, identity: &str, command: &str) -> (u8, String) {
    let tr = ultar_update::msg::translator(lang);
    let mut words: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    words.extend([
        "--socket".to_string(),
        bus.dir.path().join(identity).display().to_string(),
    ]);
    let mut out = Vec::new();
    let code = ultar_update::cli::run(&words, None, &tr, &mut out);
    (code, String::from_utf8(out).unwrap())
}

fn cli_status_check_download_apply(run: &dyn Fn(&str, &str) -> (u8, String)) {
    let (code, text) = run(USER, "status");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Running version: 0.7.0 (slot a)"), "{text}");
    assert!(
        text.contains("Channel: stable") && text.contains("The system is up to date"),
        "{text}"
    );
    assert!(
        text.contains("No previous version can be started"),
        "{text}"
    );

    let (code, text) = run(USER, "check");
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("A system update is available") && text.contains("0.7.1"),
        "{text}"
    );
    let (code, text) = run(USER, "download");
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("metered"), "{text}");
    // An app's own "use the metered connection" does not count; the administrator's does.
    let (code, text) = run(USER, "download --metered");
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("metered"), "{text}");
    let (code, text) = run(ADMIN, "download --metered");
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("100%") && text.contains("ready to install"),
        "{text}"
    );
    let (code, text) = run(USER, "apply");
    assert_eq!(code, 1);
    assert!(text.contains("administrator rights"), "{text}");
    let (code, text) = run(ADMIN, "apply");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Restart the device"), "{text}");
}

fn cli_policy_history_cancel(run: &dyn Fn(&str, &str) -> (u8, String)) {
    let (code, text) = run(ADMIN, "policy set automatic=no window=01:00-04:30 rate=300");
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Automatic updates: no") && text.contains("01:00–04:30"),
        "{text}"
    );
    assert!(text.contains("300 KiB/s"), "{text}");
    assert_eq!(run(USER, "policy set automatic=yes").0, 1);
    assert_eq!(run(ADMIN, "policy set colour=blue").0, 2);
    let (_, text) = run(USER, "policy");
    assert!(text.contains("Automatic updates: no"), "{text}");
    let (code, text) = run(USER, "history 5");
    assert_eq!(code, 0);
    assert!(
        text.contains("Update installed, waiting for restart") && text.contains("0.7.1"),
        "{text}"
    );
    let (code, _) = run(ADMIN, "cancel");
    assert_eq!(code, 0);
    assert!(run(USER, "status").1.contains("The system is up to date"));
}

#[test]
fn command_line_client_against_the_service() {
    let bus = bus_rig();
    let new = release((0, 7, 1), 2);
    bus.rig.publish(&new);
    let run =
        |identity: &str, command: &str| uupdate(&bus, ultar_i18n::Lang::En, identity, command);
    cli_status_check_download_apply(&run);
    cli_policy_history_cancel(&run);
    // Arabic output for the same state.
    let (code, text) = uupdate(&bus, ultar_i18n::Lang::Ar, USER, "status");
    assert_eq!(code, 0);
    assert!(text.contains("النظام محدَّث"));
}
