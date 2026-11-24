//! End-to-end run of the Bluetooth service on the Ultar kernel with a USB controller
//! (QEMU: `usb-redir` and `tools/kern-test/bt-usb.py`, see `tools/kern-test/bt-e2e.sh`):
//! `ultar-init` (not PID 1) starts the **shipped** definitions of logd, bus and btd
//! (paths moved below a work directory), btd as its own user with only its rights.
//! Checked: btd reaches the controller through `dev/ultar/hciN` and sets it up
//! (`bluetooth.state`: present, devices supported), it holds the node (a second opener
//! is refused), and its input sink creates a keyboard through `dev/ultar/uinput` whose
//! keys the seat reads from the new event node. Last line: `BT: ALLES OK` or
//! `BT: FEHLER <what>`.
//!
//! ```text
//! btd-e2e --bin DIR --work DIR
//! ```

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use ultar_btd::runner::Uinput;
use ultar_btd::stack::hid::{InputEvent, InputKind, InputSink};
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::{Client, ClientOptions};

macro_rules! shipped {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../rootfs/etc/ultar/services/",
            $name,
            ".toml"
        ))
    };
}

const SERVICES: [(&str, &str); 3] = [
    ("logd", shipped!("logd")),
    ("bus", shipped!("bus")),
    ("btd", shipped!("btd")),
];
const POLICY: &str = include_str!("../../../../rootfs/etc/ultar/bus-policy.toml");
const IDS: &str = include_str!("../../../../rootfs/etc/ultar/ids.toml");
const WAIT: Duration = Duration::from_secs(120);

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if Instant::now() > deadline {
            return Err(what.to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

struct Run {
    bin: PathBuf,
    work: PathBuf,
}

impl Run {
    fn at(&self, path: &str) -> PathBuf {
        self.work.join(path)
    }

    fn moved(&self, text: &str) -> String {
        let work = self.work.display().to_string();
        text.replace("/run/ultar/", &format!("{work}/run/"))
            .replace("/data/ultar/", &format!("{work}/data/"))
            .replace("/etc/ultar/", &format!("{work}/etc/"))
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
    }

    /// Writes the service files, the policy and the init configuration.
    fn layout(&self) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        for (name, text) in SERVICES {
            let body = if name == "bus" {
                // the test plays the shell: a socket for its identity, root only
                text.replacen(
                    "    \"--policy\"",
                    "    \"--listen\", \"/run/ultar/bus/system.shell.sock=system.shell\", \"--mode\", \"0600\",\n    \"--policy\"",
                    1,
                )
            } else {
                text.to_owned()
            };
            std::fs::write(services.join(format!("{name}.toml")), self.moved(&body))?;
        }
        std::fs::write(self.at("etc/bus-policy.toml"), POLICY)?;
        std::fs::write(self.at("etc/ids.toml"), IDS)?;
        let text = format!(
            "shutdown_grace_ms = 5000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\n\
             bus_socket = \"{}\"\nbus_policy = \"{}\"\ncontrol_socket = \"{}\"\nlauncher = \"{}\"\n",
            services.display(),
            self.at("run/log/init.sock").display(),
            self.at("run/bus/init.sock").display(),
            self.at("etc/bus-policy.toml").display(),
            self.at("ctl.sock").display(),
            self.bin.join("ultar-init").display(),
        );
        let config = self.at("etc/init.toml");
        std::fs::write(&config, text)?;
        Ok(config)
    }

    fn client(&self, socket: &str) -> Result<Client, String> {
        let options = ClientOptions {
            connect_retry_for: WAIT,
            ..ClientOptions::named("btd-e2e")
        };
        Client::connect(&self.at(socket), options).map_err(|e| format!("bus {socket}: {e}"))
    }
}

fn running(admin: &Client) -> Vec<(String, u32)> {
    admin
        .call(&Status {}, Duration::from_secs(5))
        .map(|r| {
            r.services
                .into_iter()
                .filter(|s| s.state == ServiceState::Running)
                .map(|s| (s.name, s.uid))
                .collect()
        })
        .unwrap_or_default()
}

/// The event node whose device name is `name` (sysfs), as a path below `root`.
fn event_node(root: &Path, name: &str) -> Option<PathBuf> {
    use ultar_hal_system::seat_nodes::{input_class, input_name, node_path};
    std::fs::read_dir(input_class(root, ""))
        .ok()?
        .flatten()
        .find_map(|e| {
            let event = e.file_name().to_str()?.to_owned();
            let named = std::fs::read_to_string(input_name(&input_class(root, &event))).ok()?;
            (named.trim_end() == name).then(|| node_path(root, &format!("input/{event}")))
        })
}

/// btd's input sink on this kernel: a keyboard, one key press, read back by the seat.
fn input_through_btd_sink(root: &Path) -> Result<(), String> {
    let mut sink = Uinput::new(root.to_path_buf());
    sink.added(7, InputKind::Keyboard, "BT E2E Tastatur");
    if sink.is_empty() {
        return Err("uinput keyboard not created".into());
    }
    let node = event_node(root, "BT E2E Tastatur").ok_or("no event node for the keyboard")?;
    let mut seat = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000) // O_NONBLOCK
        .open(&node)
        .map_err(|e| format!("open {node:?}: {e}"))?;
    let ev = |kind, code, value| InputEvent { kind, code, value };
    sink.events(7, &[ev(1, 30, 1), ev(0, 0, 0), ev(1, 30, 0), ev(0, 0, 0)]);
    let mut got = Vec::new();
    wait("key events at the seat", || {
        let mut buf = [0u8; 24 * 8];
        if let Ok(n) = seat.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
        (got.len() >= 24 * 4).then_some(())
    })?;
    let field = |i: usize, o: usize| u16::from_le_bytes([got[i * 24 + o], got[i * 24 + o + 1]]);
    if (field(0, 16), field(0, 18), got[20], field(2, 18), got[68]) != (1, 30, 1, 30, 0) {
        return Err("key events differ".into());
    }
    sink.removed(7);
    if event_node(root, "BT E2E Tastatur").is_some() {
        return Err("keyboard node stays after removal".into());
    }
    println!("BT: btd's input sink: keyboard {node:?}, key A down/up read by the seat, gone after removal");
    Ok(())
}

fn test(run: &Run) -> Result<(), String> {
    let root = Path::new("/");
    let config = run.layout().map_err(|e| format!("layout {e}"))?;
    let mut init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let checked = (|| {
        let admin = run.client("run/bus/system.sock")?;
        wait("services running", || {
            (running(&admin).len() == SERVICES.len()).then_some(())
        })?;
        let btd_uid = running(&admin)
            .into_iter()
            .find(|s| s.0 == "btd")
            .map(|s| s.1);
        println!("BT: services running, btd as uid {btd_uid:?}");
        let shell = run.client("run/bus/system.shell.sock")?;
        let sub = shell
            .subscribe::<BluetoothState>()
            .map_err(|e| format!("subscribe {e}"))?;
        let state = wait("controller set up by btd", || {
            let mut last = None;
            while let Ok(Some(v)) = sub.try_recv() {
                last = Some(v);
            }
            last.filter(|s| s.present && s.devices_supported)
        })?;
        println!(
            "BT: bluetooth.state present={} on={} devices_supported={}",
            state.present, state.on, state.devices_supported
        );
        let nodes = ultar_hal_linux::bluetooth::ultar_nodes(root);
        let node = nodes.first().ok_or("no controller node")?;
        match ultar_hal_linux::bluetooth::RawNode::open(node) {
            Err(e) if e.raw_os_error() == Some(16) => {
                println!("BT: {node:?} held by btd (second opener: EBUSY)")
            }
            other => return Err(format!("second opener of {node:?}: {:?}", other.err())),
        }
        input_through_btd_sink(root)
    })();
    let pid = rustix::process::Pid::from_raw(init.id() as i32).ok_or("pid")?;
    let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
    let ended = wait("init ended", || init.try_wait().ok().flatten());
    checked.and(ended.and_then(|s| s.success().then_some(()).ok_or(format!("init ended {s}"))))
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!("BT: FEHLER usage: btd-e2e --bin DIR --work DIR");
        return;
    };
    match test(&Run { bin, work }) {
        Ok(()) => println!("BT: ALLES OK"),
        Err(what) => println!("BT: FEHLER {what}"),
    }
}
