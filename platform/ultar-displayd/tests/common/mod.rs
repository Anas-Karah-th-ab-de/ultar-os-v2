//! Shared fixture: an in-process broker (token identities), the real
//! settings service and the real compositor in process (both with clocks
//! the test moves), and the real `ultar-displayd` binary as a child.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::{Broker, Client, ClientOptions, Identity, Policy, TokenIdentity};
use ultar_compositor::backend::{Backend, BackendOutput, Hotplug};
use ultar_compositor::geom::Rect;
use ultar_compositor::pixels::Image;
use ultar_compositor::{Clock, Compositor, Config, RightsTable};
use ultar_hal::{HalError, InputEvent};
use ultar_settingsd::access::PolicyAccess;
use ultar_settingsd::apply::Appliers;
use ultar_settingsd::clock::ManualClock;
use ultar_settingsd::core::{Core, CoreConfig};
use ultar_settingsd::schema::NoDefaults;
use ultar_settingsd::seed;
use ultar_settingsd::server::{Server, ServerParts};
use ultar_settingsd::store::StoreConfig;
use ultar_surface::proto::{OutputIdentity, OutputMode, Rights, SwitchShortcut};

pub const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-displayd");
pub const WAIT: Duration = Duration::from_secs(20);

pub const POLICY: &str = r#"
[[rule]]
identity  = "system.settingsd"
register  = ["settings"]
publish   = ["settings.*"]

[[rule]]
identity  = "system.displayd"
register  = ["display"]
publish   = ["display.outputs"]
subscribe = ["settings.changed", "settings.pending", "settings.pending_resolved"]
call      = ["settings.set_displays", "settings.admin", "settings.list", "settings.set", "settings.confirm", "settings.revert"]

[[rule]]
identity  = "app.settings"
call      = ["display.*", "settings.get", "settings.list", "settings.set"]
subscribe = ["display.outputs", "settings.*"]

[[rule]]
identity  = "app.evil"
call      = ["settings.get"]
"#;

const IDENTITIES: [&str; 4] = [
    "system.settingsd",
    "system.displayd",
    "app.settings",
    "app.evil",
];

pub fn me() -> u32 {
    ultar_service::effective_uid()
}

pub fn connect(socket: &Path, identity: &str) -> Client {
    let options = ClientOptions {
        token: Some(identity.to_owned()),
        ..ClientOptions::named(identity)
    };
    Client::connect(socket, options).unwrap()
}

fn mode(w: u32, h: u32) -> OutputMode {
    OutputMode {
        width: w,
        height: h,
        refresh_mhz: 60_000,
    }
}

/// A display as the test's backend reports it.
pub fn display(hal: u32, internal: bool) -> BackendOutput {
    BackendOutput {
        hal,
        name: if internal {
            "eDP-1".into()
        } else {
            format!("HDMI-A-{hal}")
        },
        width: 1280,
        height: 800,
        refresh_millihertz: 60_000,
        width_mm: 300,
        height_mm: 190,
        internal,
        identity: OutputIdentity {
            manufacturer: if internal { "BOE".into() } else { "DEL".into() },
            product: 0x10 + hal as u16,
            serial: 100 + hal,
            name: String::new(),
        },
        modes: vec![mode(1280, 800), mode(1024, 768)],
    }
}

#[derive(Default)]
pub struct Bench {
    pub outputs: Vec<BackendOutput>,
    pub plugs: Vec<Hotplug>,
}

struct Shared(Arc<Mutex<Bench>>);

impl Backend for Shared {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.0.lock().unwrap().outputs.clone()
    }
    fn present(&mut self, _: u32, _: &Image, _: &[Rect]) -> Result<(), HalError> {
        Ok(())
    }
    fn input_fds(&self) -> Vec<std::os::fd::BorrowedFd<'_>> {
        Vec::new()
    }
    fn read_input(&mut self) -> Vec<InputEvent> {
        Vec::new()
    }
    fn hotplug(&mut self) -> Vec<Hotplug> {
        std::mem::take(&mut self.0.lock().unwrap().plugs)
    }
    fn set_mode(&mut self, hal: u32, m: OutputMode) -> Result<(), HalError> {
        let mut b = self.0.lock().unwrap();
        let o = b
            .outputs
            .iter_mut()
            .find(|o| o.hal == hal)
            .ok_or(HalError::NotFound)?;
        (o.width, o.height, o.refresh_millihertz) = (m.width, m.height, m.refresh_mhz);
        Ok(())
    }
}

struct TestClock(Arc<AtomicU64>);

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// The pointer configuration the compositor was told (`Request::ConfigurePointer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerSnap {
    pub speed: i8,
    pub natural_scroll: bool,
    pub swap_buttons: bool,
    pub acceleration: bool,
    pub size: ultar_surface::proto::PointerSize,
    pub double_click_ms: u16,
}

impl Default for PointerSnap {
    /// The schema's defaults.
    fn default() -> Self {
        PointerSnap {
            speed: 0,
            natural_scroll: false,
            swap_buttons: false,
            acceleration: true,
            size: ultar_surface::proto::PointerSize::Normal,
            double_click_ms: 400,
        }
    }
}

/// What the compositor shows, read by the test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snap {
    /// (hal, width, enabled, primary, x) per output, by hal.
    pub outputs: Vec<(u32, u32, bool, bool, i32)>,
    /// A setup waits for confirmation.
    pub provisional: bool,
    /// Display numbers are shown.
    pub identify: usize,
    /// Deadline of the waiting setup (compositor clock, ms).
    pub deadline: u64,
    /// The compositor's clock.
    pub now: u64,
    /// The pointer configuration.
    pub pointer: PointerSnap,
    /// Key repeat: on, delay in milliseconds, per second.
    pub key_repeat: (bool, u16, u8),
    /// Keyboard helpers: sticky, slow, bounce, mouse, and the compose key.
    pub key_helpers: (bool, bool, bool, bool, ultar_surface::proto::ComposeKey),
    /// The switch combination (`None`: unconfigured) and per-window languages.
    pub keyboard: (Option<SwitchShortcut>, bool),
}

impl Snap {
    pub fn enabled(&self) -> Vec<u32> {
        self.outputs.iter().filter(|o| o.2).map(|o| o.0).collect()
    }
}

pub struct Rig {
    broker: Option<Broker>,
    pub socket: PathBuf,
    pub dir: PathBuf,
    pub clock: ManualClock,
    pub server: Option<Server>,
    pub comp_clock: Arc<AtomicU64>,
    pub bench: Arc<Mutex<Bench>>,
    pub snap: Arc<Mutex<Snap>>,
    rescan: Sender<()>,
    stop: Arc<AtomicBool>,
}

pub struct Daemon(pub Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(s) = self.server.take() {
            s.stop();
        }
        std::thread::sleep(Duration::from_millis(100));
        // The broker needs its socket file to shut down: first the broker.
        drop(self.broker.take());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn snapshot(comp: &Compositor) -> Snap {
    let mut outputs: Vec<_> = comp
        .state
        .layout
        .outputs()
        .iter()
        .map(|o| (o.hal, o.width, o.enabled, o.primary, o.x))
        .collect();
    outputs.sort();
    Snap {
        outputs,
        provisional: comp.state.session.manage.provisional.is_some(),
        deadline: comp
            .state
            .session
            .manage
            .provisional
            .as_ref()
            .map_or(0, |p| p.deadline),
        now: comp.state.now,
        pointer: {
            let p = &comp.state.pointer;
            PointerSnap {
                speed: p.speed,
                natural_scroll: p.natural_scroll,
                swap_buttons: p.swap_buttons,
                acceleration: p.accel.on,
                size: p.size,
                double_click_ms: p.double_click_ms,
            }
        },
        key_repeat: {
            let r = comp.state.keyboard.repeat;
            (r.enabled, r.delay_ms, r.per_second)
        },
        key_helpers: {
            let h = &comp.state.keyboard.helpers;
            (
                h.sticky.on,
                h.slow.on,
                h.bounce.on,
                h.mouse.on,
                h.compose.key,
            )
        },
        keyboard: (comp.state.keyboard.switch, comp.state.keyboard.per_window),
        identify: comp
            .state
            .session
            .manage
            .identify
            .as_ref()
            .map_or(0, |i| i.numbers.len()),
    }
}

/// What the compositor thread shares with the test.
type Parts = (
    Arc<Mutex<Bench>>,
    Arc<AtomicU64>,
    Arc<Mutex<Snap>>,
    Arc<AtomicBool>,
);

/// The compositor on its own thread: dispatches, rescans when asked, and
/// leaves a snapshot for the test after every turn.
fn spawn_compositor((b, c, s, st): Parts, path: PathBuf, rescans: std::sync::mpsc::Receiver<()>) {
    std::thread::spawn(move || {
        let mut rights = RightsTable::default();
        rights.grant(me(), Rights::OUTPUT_CONFIG);
        let config = Config {
            rights,
            cursor: false,
            ..Config::default()
        };
        let mut comp = Compositor::new(config, Box::new(Shared(b)), Box::new(TestClock(c)));
        comp.set_listener(UnixListener::bind(&path).unwrap())
            .unwrap();
        while !st.load(Ordering::SeqCst) {
            while rescans.try_recv().is_ok() {
                comp.rescan_outputs();
            }
            let _ = comp.dispatch(Some(Duration::from_millis(10)));
            *s.lock().unwrap() = snapshot(&comp);
        }
    });
}

impl Rig {
    pub fn new(tag: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("ultar-dispd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.join("bus.sock");
        let mut auth = TokenIdentity::new();
        for name in IDENTITIES {
            auth = auth.with_token(name, Identity::new(name).unwrap());
        }
        let broker = Broker::builder(Policy::from_toml_str(POLICY).unwrap())
            .listen(&socket, Arc::new(auth))
            .start()
            .unwrap();
        let token = dir.join("token");
        std::fs::write(&token, "system.displayd").unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let bench = Arc::new(Mutex::new(Bench {
            outputs: vec![display(0, true), display(1, false)],
            plugs: Vec::new(),
        }));
        let comp_clock = Arc::new(AtomicU64::new(1_000));
        let snap = Arc::new(Mutex::new(Snap::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (rescan, rescans) = channel::<()>();
        spawn_compositor(
            (
                bench.clone(),
                comp_clock.clone(),
                snap.clone(),
                stop.clone(),
            ),
            dir.join("comp.sock"),
            rescans,
        );
        let mut rig = Rig {
            broker: Some(broker),
            socket,
            dir,
            clock: ManualClock::new(),
            server: None,
            comp_clock,
            bench,
            snap,
            rescan,
            stop,
        };
        rig.start_settings();
        rig
    }

    pub fn start_settings(&mut self) {
        let clock = Arc::new(self.clock.clone());
        let config = CoreConfig {
            store: StoreConfig {
                dir: self.dir.join("settings"),
                user: "anas".into(),
                write_delay: Duration::from_millis(50),
            },
            registry: Arc::new(seed::registry().unwrap()),
            clock: clock.clone(),
            appliers: Appliers::new(),
            defaults: Box::new(NoDefaults),
            capabilities: {
                // A keyboard and a mouse: the panels whose values the compositor follows.
                let mut c = ultar_profile::Capabilities::default();
                c.flags.insert(ultar_profile::Capability::Keyboard);
                c.flags.insert(ultar_profile::Capability::Pointer);
                c
            },
        };
        let (core, notices) = Core::open(config).unwrap();
        let server = Server::start(ServerParts {
            bus: connect(&self.socket, "system.settingsd"),
            core,
            access: Box::new(PolicyAccess(Policy::from_toml_str(POLICY).unwrap())),
            clock,
            notices,
            on_error: Box::new(|_| {}),
            register_wait: WAIT,
        })
        .unwrap();
        self.server = Some(server);
    }

    pub fn stop_settings(&mut self) {
        if let Some(s) = self.server.take() {
            s.stop();
        }
    }

    pub fn start_displayd(&self) -> Daemon {
        let mut cmd = Command::new(DAEMON);
        cmd.arg("--bus")
            .arg(&self.socket)
            .arg("--compositor")
            .arg(self.dir.join("comp.sock"))
            .arg("--token-file")
            .arg(self.dir.join("token"));
        if me() == 0 {
            cmd.arg("--allow-root");
        }
        Daemon(cmd.spawn().unwrap())
    }

    pub fn client(&self, identity: &str) -> Client {
        connect(&self.socket, identity)
    }

    pub fn snap(&self) -> Snap {
        self.snap.lock().unwrap().clone()
    }

    /// Waits until the compositor shows what `ok` wants.
    pub fn wait_for(&self, what: &str, ok: impl Fn(&Snap) -> bool) -> Snap {
        let start = Instant::now();
        loop {
            let s = self.snap();
            if ok(&s) {
                return s;
            }
            assert!(start.elapsed() < WAIT, "{what}: {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Moves the settings service's clock and lets it act.
    pub fn advance_settings(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
        if let Some(s) = &self.server {
            s.tick();
        }
    }

    pub fn advance_compositor(&self, ms: u64) {
        self.comp_clock.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn plug(&self, o: BackendOutput) {
        {
            let mut b = self.bench.lock().unwrap();
            b.outputs.push(o.clone());
            b.plugs.push(Hotplug::Added(o));
        }
        let _ = self.rescan.send(());
    }
}

/// Waits for a condition outside the compositor.
pub fn eventually(what: &str, ok: impl Fn() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < WAIT, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
