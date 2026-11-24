//! A malicious app against its cell (real cells need root; without root the
//! test says so and checks only that the unconfined run is the operator's
//! explicit choice).
//!
//! The app tries to read another app's data, to signal another app, to
//! become root, to gain kernel rights or namespaces, to mount, to write the
//! system, to reach privileged sockets and the network, and to fork without
//! end. Filling memory needs a kernel with a memory controller: it runs in
//! the QEMU run on the Ultar kernel (and here only where Linux has one).

mod common;

use common::{root, wait_state, Collector, World};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use ultar_bus::messages::launcher::{Exit, InstanceState as S, Role};

/// What the malicious app goes for: another app with a secret in its data,
/// privileged sockets (one root-only in the shared directory the cell sees,
/// one outside its view) and a service on the host's loopback.
struct Targets {
    data: std::path::PathBuf,
    args: Vec<String>,
    _sockets: [UnixListener; 2],
    _tcp: std::net::TcpListener,
}

fn targets(world: &World, launcher: &std::sync::Arc<ultar_launcher::Launcher>) -> Targets {
    launcher
        .install(&world.package("other", "1.0.0", "", true))
        .unwrap();
    let victim = launcher.start("other", Role::Ui, None).unwrap();
    let data = world.path("appdata/other");
    std::fs::write(data.join("secret"), b"victim's secret").unwrap();
    let shared = world.path("share/admin.sock");
    let shared_listener = UnixListener::bind(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o600)).unwrap();
    let hidden = world.path("state.sock");
    let hidden_listener = UnixListener::bind(&hidden).unwrap();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let text = |p: &std::path::Path| p.display().to_string();
    let args = vec![
        "attack".into(),
        "--victim-data".into(),
        text(&data),
        "--victim-pid".into(),
        victim.pid.to_string(),
        "--socket".into(),
        text(&shared),
        "--socket".into(),
        text(&hidden),
        "--port".into(),
        tcp.local_addr().unwrap().port().to_string(),
        "--forks".into(),
        "64".into(),
    ];
    Targets {
        data,
        args,
        _sockets: [shared_listener, hidden_listener],
        _tcp: tcp,
    }
}

#[test]
fn a_malicious_app_cannot_leave_its_cell() {
    if !root() {
        eprintln!("cell test skipped: real cells need root (the process tests cover the rest)");
        return;
    }
    let mut world = World::new("evil");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    let targets = targets(&world, &launcher);
    let args: Vec<&str> = targets.args.iter().map(String::as_str).collect();
    let limits = "[limits]\npids = 16\nmemory_mb = 64\n";
    launcher
        .install(&world.package_args("evil", &args, limits, false))
        .unwrap();
    launcher.start("evil", Role::Ui, None).unwrap();
    let ended = wait_state(&launcher, "evil", Role::Ui, &[S::Stopped, S::Crashed]);
    let verdict = collector.wait_for("app-evil", "ATTACKS:");
    let lines = collector.of("app-evil");
    eprintln!("{}", lines.join("\n"));
    assert_eq!(
        verdict.as_deref(),
        Some("ATTACKS: ALL BLOCKED"),
        "{lines:#?}"
    );
    assert_eq!(ended.last_exit, Some(Exit::Code(0)), "{lines:#?}");
    for name in ATTACKS {
        let line = format!("ATTACK {name} blocked");
        assert!(lines.contains(&line), "{name}: {lines:#?}");
    }
    // the victim still runs, its secret is unchanged
    let other = launcher
        .list()
        .into_iter()
        .find(|a| a.app == "other")
        .unwrap();
    assert_eq!(other.instances[0].state, S::Running);
    assert_eq!(
        std::fs::read(targets.data.join("secret")).unwrap(),
        b"victim's secret"
    );
    // the limits reached the group of the app
    let group = world.path("groups/apps/evil");
    let read = |f: &str| std::fs::read_to_string(group.join(f)).unwrap();
    assert_eq!(read("memory.max"), (64u64 << 20).to_string());
    assert_eq!(read("pids.max"), "16");
    assert!(!read("cgroup.procs").is_empty());
}

/// Every attack the test asks for.
const ATTACKS: [&str; 11] = [
    "read_other_data",
    "kill_other_app",
    "become_root",
    "kernel_rights",
    "new_namespace",
    "mount",
    "write_system",
    "write_root",
    "privileged_socket",
    "network",
    "fork_bomb",
];

#[test]
fn the_network_needs_the_users_permission() {
    if !root() {
        eprintln!("network test skipped: network namespaces need root");
        return;
    }
    let mut world = World::new("net");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = tcp.local_addr().unwrap().port().to_string();
    launcher
        .install(&world.package_args(
            "net",
            &["attack", "--port", &port],
            "[permissions]\nnetwork = \"ask\"\n",
            false,
        ))
        .unwrap();
    launcher.start("net", Role::Ui, None).unwrap();
    wait_state(&launcher, "net", Role::Ui, &[S::Stopped, S::Crashed]);
    assert!(collector
        .wait_for("app-net", "ATTACK network blocked")
        .is_some());
    // granted: the same app reaches the host's loopback (its attack "escapes")
    launcher.set_permission("net", "network", true).unwrap();
    launcher.start("net", Role::Ui, None).unwrap();
    let ended = wait_state(&launcher, "net", Role::Ui, &[S::Stopped, S::Crashed]);
    assert!(collector
        .wait_for("app-net", "ATTACK network ESCAPED")
        .is_some());
    assert_eq!(ended.last_exit, Some(Exit::Code(3)));
}
