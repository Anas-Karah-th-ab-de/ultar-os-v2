//! Regression (owner, VMware 1024x768, real mouse): in the quick settings sheet the Wi-Fi
//! symbol and tile, the tiles with an arrow (wired network, VPN, power mode), the gear, the
//! lock and the power button opened nothing, and the sliders said "0%" with the knob at the
//! far end. REAL-INPUT: the real compositor (headless output of 1024x768), the real shell
//! program on its own thread, a stand-in launcher and lock service on a real bus; every
//! click is an absolute pointer position and a button press injected into the compositor's
//! input, as VMware's absolute mouse delivers them. No service for Wi-Fi, VPN, audio or the
//! settings exists here – exactly the situation of the owner's virtual machine.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{
    InstanceState, List, ListReply, Start, StartReply, SERVICE as LAUNCHER,
};
use ultar_bus::messages::lock::{Lock, LockReply};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_compositor::backend::Headless;
use ultar_compositor::keymap::Layout;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_desktop::app::Options;
use ultar_desktop::model::quick::TileId;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::view::Target;
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState, PointerButton};
use ultar_surface::proto::{Rights, Role};
use ultar_ui::geom::Rect;

const W: u32 = 1024;
const H: u32 = 768;

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

/// What the stand-in services were asked.
#[derive(Default)]
struct Asked {
    starts: Vec<(String, Option<String>)>,
    locks: usize,
}

/// What the shell shows, after each turn of its loop.
#[derive(Default)]
struct Seen {
    open: Open,
    rects: Vec<(Target, bool, Rect)>,
}

const WATCHED: [Target; 17] = [
    Target::Status,
    Target::Tile(TileId::Wifi),
    Target::TileMore(TileId::Wifi),
    Target::Tile(TileId::Wired),
    Target::Tile(TileId::Vpn),
    Target::TileMore(TileId::Vpn),
    Target::Tile(TileId::PowerMode),
    Target::TileMore(TileId::PowerMode),
    Target::Tile(TileId::Dark),
    Target::Settings,
    Target::Lock,
    Target::Power,
    Target::QuickBack,
    Target::WifiSettings,
    Target::VpnSettings,
    Target::Volume,
    Target::Brightness,
];

struct World {
    comp: Compositor,
    asked: Arc<Mutex<Asked>>,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    shell: Option<std::thread::JoinHandle<()>>,
}

impl World {
    fn new() -> World {
        let dir = std::env::temp_dir().join(format!("udesk-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut rights = RightsTable::default();
        rights.grant(my_uid(), Rights::ALL);
        let config = Config {
            rights,
            inline_only: true,
            cursor: false,
            layouts: vec![Layout::Arabic, Layout::English],
            lang: ultar_ui::Lang::Ar,
            ..Config::default()
        };
        let mut comp = Compositor::new(
            config,
            Box::new(Headless::new(&[(W, H)])),
            Box::new(MonotonicClock::default()),
        );
        let socket = dir.join("c.sock");
        comp.set_listener(UnixListener::bind(&socket).unwrap())
            .unwrap();
        let bus = dir.join("bus.sock");
        let identity = Arc::new(FixedIdentity::new(Identity::new("ui.test").unwrap()));
        let broker = Broker::builder(Policy::allow_all())
            .listen(&bus, identity)
            .start()
            .unwrap();
        std::mem::forget(broker);
        let stop = Arc::new(AtomicBool::new(false));
        let asked = Arc::new(Mutex::new(Asked::default()));
        services(&bus, &stop, &asked);
        let seen = Arc::new(Mutex::new(Seen::default()));
        let options = Options {
            compositor: socket,
            bus: Some(bus),
            lang: ultar_i18n::Lang::Ar,
            light: false,
            form: Some("desktop".into()),
            bar: None,
            state: None,
        };
        let (stop2, seen2) = (stop.clone(), seen.clone());
        let shell = std::thread::spawn(move || {
            let mut app = ultar_desktop::app::desktop::DesktopApp::start(
                &options,
                "anas",
                Arc::new(ultar_desktop::model::time::SystemClock::default()),
            )
            .unwrap();
            while !stop2.load(Ordering::SeqCst) {
                if app.turn().is_err() {
                    break;
                }
                let mut s = seen2.lock().unwrap();
                s.open = app.shell().d.open;
                s.rects = WATCHED
                    .iter()
                    .filter_map(|t| app.locate(t).map(|(p, r)| (t.clone(), p, r)))
                    .collect();
            }
        });
        World {
            comp,
            asked,
            seen,
            stop,
            shell: Some(shell),
        }
    }

    fn pump(&mut self, ms: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            self.comp.dispatch(Some(Duration::from_millis(5))).unwrap();
        }
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&World) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "timed out waiting for: {what} (open: {:?})",
                self.open()
            );
            self.comp.dispatch(Some(Duration::from_millis(10))).unwrap();
        }
        self.pump(80);
    }

    fn open(&self) -> Open {
        self.seen.lock().unwrap().open
    }

    fn layer(&self, role_popup: bool) -> Option<Rect> {
        let c = &self.comp;
        let key = *c.state.layers.iter().find(|k| {
            let role = c.state.surface(**k).map(|s| s.role);
            if role_popup {
                matches!(role, Some(Role::Popup { .. }))
            } else {
                matches!(role, Some(Role::Panel { .. }))
            }
        })?;
        let r = c.state.layer_rect(key)?;
        Some(Rect::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32))
    }

    /// The screen rectangle of a target (its surface's place plus its place inside).
    fn find(&self, target: &Target) -> Option<Rect> {
        let seen = self.seen.lock().unwrap();
        let (_, popup, r) = seen.rects.iter().find(|(t, ..)| t == target)?.clone();
        drop(seen);
        let at = self.layer(popup)?;
        Some(Rect::new(at.x + r.x, at.y + r.y, r.w, r.h))
    }

    fn input(&mut self, kind: InputEventKind) {
        self.comp.inject(&[InputEvent {
            device: InputDeviceId(0),
            time_us: 0,
            kind,
        }]);
    }

    /// A click as VMware's absolute pointer sends it: position, press, release.
    fn click_at(&mut self, x: f32, y: f32) {
        let abs = |v: f32, size: u32| ((v / size as f32) * 65536.0).clamp(0.0, 65535.0) as u16;
        let (x, y) = (abs(x, W), abs(y, H));
        self.input(InputEventKind::PointerAbsolute { x, y });
        self.pump(60);
        for pressed in [true, false] {
            self.input(InputEventKind::PointerButton {
                button: PointerButton::Primary,
                pressed,
            });
            self.pump(60);
        }
    }

    fn click(&mut self, target: &Target) {
        self.until(&format!("{target:?} shown"), |w| w.find(target).is_some());
        let r = self.find(target).unwrap();
        self.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
    }

    fn key(&mut self, code: u16) {
        for state in [KeyState::Pressed, KeyState::Released] {
            self.input(InputEventKind::Key { code, state });
        }
        self.pump(60);
    }

    fn starts(&self) -> Vec<(String, Option<String>)> {
        self.asked.lock().unwrap().starts.clone()
    }

    /// Opens the tiles with a click beside the status icons (not on the network icon).
    fn open_tiles(&mut self) {
        if self.open() == Open::Quick(QuickPage::Tiles) {
            return;
        }
        if self.open() != Open::None {
            self.key(1);
            self.until("closed", |w| w.open() == Open::None);
        }
        self.until("the bar", |w| w.find(&Target::Status).is_some());
        let r = self.find(&Target::Status).unwrap();
        self.click_at(r.x + 2.0, r.y + r.h / 2.0);
        self.until("the tiles", |w| w.open() == Open::Quick(QuickPage::Tiles));
    }

    fn expect_start(&mut self, uri: Option<&str>) {
        let before = self.starts().len();
        let _ = before;
        let want = ("settings".to_owned(), uri.map(str::to_owned));
        self.until(&format!("launcher.start {want:?}"), |w| {
            w.starts().last() == Some(&want)
        });
        self.asked.lock().unwrap().starts.clear();
        self.until("the sheet closed", |w| w.open() == Open::None);
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(shell) = self.shell.take() {
            let start = Instant::now();
            while !shell.is_finished() && start.elapsed() < Duration::from_secs(5) {
                let _ = self.comp.dispatch(Some(Duration::from_millis(10)));
            }
            let _ = shell.join();
        }
    }
}

/// The stand-in launcher (records starts) and lock service (records locks).
fn services(bus: &std::path::Path, stop: &Arc<AtomicBool>, asked: &Arc<Mutex<Asked>>) {
    let (bus2, stop2, asked2) = (bus.to_owned(), stop.clone(), asked.clone());
    std::thread::spawn(move || {
        let client = Client::connect(&bus2, ClientOptions::named("launcher")).unwrap();
        let endpoint = client.register(LAUNCHER).unwrap();
        while !stop2.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if request.decode::<List>().is_ok() {
                let _ = request.reply(&ListReply::default());
            } else if let Ok(s) = request.decode::<Start>() {
                asked2
                    .lock()
                    .unwrap()
                    .starts
                    .push((s.app.clone(), s.uri.clone()));
                let _ = request.reply(&StartReply {
                    state: InstanceState::Running,
                    pid: 4242,
                    already: false,
                    opened: s.uri.is_some(),
                });
            }
        }
    });
    let (bus3, stop3, asked3) = (bus.to_owned(), stop.clone(), asked.clone());
    std::thread::spawn(move || {
        let client = Client::connect(&bus3, ClientOptions::named("lock")).unwrap();
        let endpoint = client.register("lock").unwrap();
        while !stop3.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if request.decode::<Lock>().is_ok() {
                asked3.lock().unwrap().locks += 1;
                let _ = request.reply(&LockReply { locked: true });
            }
        }
    });
}

#[test]
fn every_part_of_the_quick_settings_answers_a_real_click() {
    let _ = PathBuf::new();
    let mut w = World::new();
    w.until("the bar", |w| w.find(&Target::Status).is_some());
    // The network symbol on the bar opens the Wi-Fi page directly.
    let r = w.find(&Target::Status).unwrap();
    w.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
    w.until("Wi-Fi page from the bar icon", |w| {
        w.open() == Open::Quick(QuickPage::Wifi)
    });
    // Its back button leads to the tiles.
    w.click(&Target::QuickBack);
    w.until("tiles", |w| w.open() == Open::Quick(QuickPage::Tiles));
    // The Wi-Fi tile (no network service here) opens its page, which says why.
    w.click(&Target::Tile(TileId::Wifi));
    w.until("Wi-Fi page from the tile", |w| {
        w.open() == Open::Quick(QuickPage::Wifi)
    });
    // "Wi-Fi settings": the Settings app on its page.
    w.click(&Target::WifiSettings);
    w.expect_start(Some("settings://wifi"));
    // The Wi-Fi arrow.
    w.open_tiles();
    w.click(&Target::TileMore(TileId::Wifi));
    w.until("Wi-Fi page from the arrow", |w| {
        w.open() == Open::Quick(QuickPage::Wifi)
    });
    // Wired network: Settings > Network.
    w.open_tiles();
    w.click(&Target::Tile(TileId::Wired));
    w.expect_start(Some("settings://network"));
    // VPN: arrow and tile open the VPN page; its settings button opens Settings.
    w.open_tiles();
    w.click(&Target::TileMore(TileId::Vpn));
    w.until("VPN page from the arrow", |w| {
        w.open() == Open::Quick(QuickPage::Vpn)
    });
    w.click(&Target::QuickBack);
    w.until("tiles", |w| w.open() == Open::Quick(QuickPage::Tiles));
    w.click(&Target::Tile(TileId::Vpn));
    w.until("VPN page from the tile", |w| {
        w.open() == Open::Quick(QuickPage::Vpn)
    });
    w.click(&Target::VpnSettings);
    w.expect_start(Some("settings://network/vpn"));
    // Power mode: the arrow shows the three modes.
    w.open_tiles();
    w.click(&Target::TileMore(TileId::PowerMode));
    w.until("power modes", |w| {
        w.open() == Open::Quick(QuickPage::PowerModes)
    });
    w.click(&Target::QuickBack);
    w.until("tiles", |w| w.open() == Open::Quick(QuickPage::Tiles));
    // Without the settings service a switch tile opens its place in Settings.
    w.click(&Target::Tile(TileId::PowerMode));
    w.expect_start(Some("settings://power"));
    w.open_tiles();
    w.click(&Target::Tile(TileId::Dark));
    w.expect_start(Some("settings://appearance"));
    // The gear, the lock and the power button.
    w.open_tiles();
    w.click(&Target::Settings);
    w.expect_start(None);
    w.open_tiles();
    w.click(&Target::Power);
    w.until("the power menu", |w| w.open() == Open::PowerMenu);
    w.key(1);
    w.until("closed", |w| w.open() == Open::None);
    w.open_tiles();
    w.click(&Target::Lock);
    w.until("the lock service asked", |w| {
        w.asked.lock().unwrap().locks == 1
    });
}
