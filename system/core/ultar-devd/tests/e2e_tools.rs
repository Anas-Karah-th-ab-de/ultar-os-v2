//! End to end: `udevctl` (in process, three languages) and the two binaries as real
//! processes against a real broker.

mod rig;

use rig::kernel_fmt::beelink;
use rig::{availability, builtin_manifest, Rig};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::devices::{Availability, Capabilities};
use ultar_bus::{Broker, BusError, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_devd::cli;
use ultar_devd::daemon::Daemon;

const WAIT: Duration = Duration::from_secs(30);
const NVME0: &str = "pci:01:00.0";

fn switch_rig(tag: &str) -> Rig {
    let rig = Rig::new(tag);
    let machine = beelink();
    machine.write_ultar_root(&rig.root, &machine.selftest_log());
    rig.add_repo_drivers();
    rig.add_planned_drivers();
    let rule = "pci_class = [\"010802\"]";
    rig.add_builtin(&builtin_manifest("nvme", "1.1.0", "storage", rule), "nvme");
    let turbo = builtin_manifest("nvme-turbo", "1.0.0", "storage", rule);
    rig.add_builtin(&turbo, "nvme-turbo");
    rig.write_index();
    rig
}

fn start_broker(rig: &Rig) -> (Broker, PathBuf) {
    let socket = rig.root.parent().unwrap().join("bus.sock");
    let identity = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&socket, identity)
        .start()
        .unwrap();
    (broker, socket)
}

/// Broker + daemon on a thread; `udevctl` runs in process through `cli::run`.
struct Tool {
    _rig: Rig,
    _broker: Broker,
    socket: PathBuf,
    daemon: std::thread::JoinHandle<Result<(), BusError>>,
}

fn tool(tag: &str) -> Tool {
    let rig = switch_rig(tag);
    let (broker, socket) = start_broker(&rig);
    let bus = Client::connect(&socket, ClientOptions::named("ultar-devd")).unwrap();
    let endpoint = Daemon::register(&bus).unwrap();
    let mut daemon = Daemon::start(rig.config(), Box::new(bus.clone())).unwrap();
    Tool {
        _rig: rig,
        _broker: broker,
        socket,
        daemon: std::thread::spawn(move || daemon.serve(&endpoint)),
    }
}

impl Tool {
    fn run(&self, lang: &str, words: &[&str]) -> (i32, String) {
        let mut args = vec![
            "--bus".to_string(),
            self.socket.to_string_lossy().into_owned(),
        ];
        args.extend(["--lang", lang].iter().map(|w| (*w).to_string()));
        args.extend(words.iter().map(|w| (*w).to_string()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli::run(&args, &mut out, &mut err);
        assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
        (code, String::from_utf8(out).unwrap())
    }

    fn shut_down(self) {
        let (code, text) = self.run("en", &["shutdown"]);
        assert_eq!(code, cli::EXIT_OK);
        assert_eq!(text, "The device service was stopped.\n");
        assert!(self.daemon.join().unwrap().is_ok());
    }
}

#[test]
fn udevctl_lists_devices_with_reasons_and_notices() {
    let t = tool("ctl-list");
    let (code, text) = t.run("en", &["list"]);
    assert_eq!(code, cli::EXIT_OK);
    for expected in [
        "pci:01:00.0  pci:c0a9:560b  [working]\n    driver: nvme 1.1.0\n",
        "    alternative: nvme-turbo 1.0.0\n",
        "pci:ac:00.0  pci:8086:272b  [not working]",
        "reason: The driver package needs a loadable kernel module",
        "pci:00:0b.0  pci:8086:7d1d  [no driver]",
        "notice: The class of some devices is not known exactly",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    let note = "Note: the running kernel cannot switch its built-in drivers while running.\n";
    assert!(text.ends_with(note));
    let (code, text) = t.run("ar", &["list"]);
    assert_eq!(code, cli::EXIT_OK);
    assert!(text.contains("pci:01:00.0  pci:c0a9:560b  [يعمل]\n    التعريف: nvme 1.1.0\n"));
    assert!(text.contains("[بلا تعريف]"));
    let (code, text) = t.run("en", &["rescan"]);
    assert_eq!(code, cli::EXIT_OK);
    let rows = text
        .lines()
        .filter(|l| l.starts_with("pci:") || l.starts_with("usb:"));
    assert_eq!(rows.count(), 14);
    t.shut_down();
}

#[test]
fn udevctl_switch_and_rollback() {
    let t = tool("ctl-switch");
    let (code, text) = t.run("en", &["switch", NVME0, "nvme", "1.0.0"]);
    assert_eq!(code, cli::EXIT_OK);
    let head = "Done.\npci:01:00.0  pci:c0a9:560b  [working]\n    driver: nvme 1.0.0\n";
    assert!(text.starts_with(head), "{text}");
    assert!(text.contains("    previous: nvme 1.1.0\n"));
    let (code, text) = t.run("ar", &["rollback", NVME0]);
    assert_eq!(code, cli::EXIT_OK);
    assert!(text.starts_with("تمّ.\n"), "{text}");
    assert!(text.contains("التعريف: nvme 1.1.0"));
    t.shut_down();
}

#[test]
fn udevctl_reports_refusals_in_three_languages() {
    let t = tool("ctl-refuse");
    let turbo = ["switch", NVME0, "nvme-turbo", "1.0.0"];
    let (code, text) = t.run("de", &turbo);
    assert_eq!(code, cli::EXIT_FAILED);
    let german = "Abgelehnt: Das Gerät pci:01:00.0 lässt sich nicht auf den Treiber nvme-turbo 1.0.0 umstellen";
    assert!(text.starts_with(german), "{text}");
    let (code, text) = t.run("ar", &turbo);
    assert_eq!(code, cli::EXIT_FAILED);
    let arabic = "رُفض الطلب: لا يمكن تبديل الجهاز pci:01:00.0 إلى التعريف nvme-turbo 1.0.0";
    assert!(text.starts_with(arabic), "{text}");
    let (code, text) = t.run("en", &turbo);
    assert_eq!(code, cli::EXIT_FAILED);
    assert!(text
        .starts_with("Refused: Device pci:01:00.0 cannot be switched to driver nvme-turbo 1.0.0"));
    let cases = [
        (
            "en",
            &["rollback", "pci:02:00.0"][..],
            "Refused: Device pci:02:00.0 has no earlier driver to return to.\n",
        ),
        (
            "en",
            &["rescan", NVME0],
            "Refused: Not possible while device pci:01:00.0 is in the state: working.\n",
        ),
        (
            "de",
            &["clear-quarantine", "pci:ff:00.0"],
            "Abgelehnt: Unter der Adresse pci:ff:00.0 gibt es kein Gerät.\n",
        ),
    ];
    for (lang, words, expected) in cases {
        assert_eq!(t.run(lang, words), (cli::EXIT_FAILED, expected.to_string()));
    }
    let (code, text) = t.run("ar", &["clear-quarantine"]);
    assert_eq!(
        (code, text.as_str()),
        (cli::EXIT_OK, "لا يوجد حجر لإزالته.\n")
    );
    t.shut_down();
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn daemon_command(rig: &Rig) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ultar-devd"));
    command
        .args(["--depot", &text(&rig.depot), "--keys", &text(&rig.keys)])
        .args(["--state", &text(&rig.state), "--root", &text(&rig.root)]);
    command
}

fn spawn_daemon(rig: &Rig, socket: &Path) -> Child {
    daemon_command(rig)
        .args(["--bus", &text(socket), "--lang", "en"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn udevctl(socket: &Path, words: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_udevctl"))
        .args(["--bus", &text(socket), "--lang", "en"])
        .args(words)
        .output()
        .unwrap()
}

#[test]
fn real_processes_start_serve_and_shut_down() {
    let rig = switch_rig("process");
    let (_broker, socket) = start_broker(&rig);
    let observer = Client::connect(&socket, ClientOptions::named("observer")).unwrap();
    let capabilities = observer.subscribe::<Capabilities>().unwrap();
    let daemon = spawn_daemon(&rig, &socket);
    // The retained capability message marks the end of the start-up sequence; the service
    // was registered before it, so the first call cannot come too early.
    let published = capabilities.recv_timeout(WAIT).unwrap();
    let available: Vec<&str> = availability(&published)
        .into_iter()
        .filter(|(_, a)| *a == Availability::Available)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(available, ["audio_out", "bus", "net", "storage"]);
    let list = udevctl(&socket, &["list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let list = String::from_utf8(list.stdout).unwrap();
    assert!(list.contains("pci:ad:00.0  pci:8086:57b0  [working]\n    driver: gen-ixgbe 1.0.0\n"));
    let refused = udevctl(&socket, &["switch", NVME0, "nvme-turbo", "1.0.0"]);
    assert_eq!(refused.status.code(), Some(1));
    let refused = String::from_utf8_lossy(&refused.stdout).into_owned();
    assert!(refused.starts_with("Refused: Device pci:01:00.0 cannot be switched"));
    let switched = udevctl(&socket, &["switch", NVME0, "nvme", "1.0.0"]);
    assert_eq!(switched.status.code(), Some(0));
    assert_eq!(udevctl(&socket, &["bogus"]).status.code(), Some(2));
    assert_eq!(udevctl(&socket, &["shutdown"]).status.code(), Some(0));
    let output = daemon.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "ultar-devd: Device service running: 14 devices, 7 with a working driver.\n\
         ultar-devd: The device service was stopped.\n"
    );
    // The switch made through the tool is on disk.
    use ultar_devd::ChoiceStore;
    let mut store = ultar_devd::store::file::FileStore::new(&rig.state);
    let state = store.load().unwrap();
    let choice = state.choices.get("pci:01:00.0=pci:c0a9:560b").unwrap();
    assert_eq!(choice.to_string(), "nvme 1.0.0");
}

#[test]
fn once_mode_prints_the_device_list_without_a_bus() {
    let rig = switch_rig("once");
    let output = daemon_command(&rig)
        .args(["--once", "--lang", "de"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let list = String::from_utf8(output.stdout).unwrap();
    let nvme = "pci:01:00.0  pci:c0a9:560b  [läuft]\n    Treiber: nvme 1.1.0\n";
    assert!(list.contains(nvme), "{list}");
    assert!(list.contains("usb:0-1  usb:1c7a:0577  [kein Treiber]"));
}

#[test]
fn daemon_binary_reports_start_and_usage_errors() {
    // No kernel known and none given: a clear message and exit code 1.
    let empty = Rig::new("once-empty");
    let output = daemon_command(&empty)
        .args(["--once", "--lang", "en"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "ultar-devd: The running kernel could not be determined. Use --kernel-abi.\n"
    );
    let usage = Command::new(env!("CARGO_BIN_EXE_ultar-devd"))
        .args(["--bogus", "--lang", "en"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(!usage.stderr.is_empty() && usage.stdout.is_empty());
}
