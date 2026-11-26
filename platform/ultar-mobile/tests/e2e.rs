//! End to end on Linux with the real pieces in one test process: the compositor (headless
//! phone-sized output), the bus broker, a launcher and an authentication service standing in
//! for the real ones, and the phone's two programs as clients on their own threads over
//! real sockets. A finger (touch-like pointer input injected into the compositor) taps the
//! status bar, the quick settings, the navigation bar and an app; the lock screen lifts the
//! lock only for the right password.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::auth::{
    Outcome, UserInfo, Users, UsersReply, Verify, VerifyReply, SERVICE as AUTH,
};
use ultar_bus::messages::launcher::{AppStatus, List, ListReply, Start, SERVICE as LAUNCHER};
use ultar_bus::messages::lock::Lock;
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState, PointerButton};
use ultar_mobile::click::Click;
use ultar_mobile::model::quick::Toggle;
use ultar_mobile::model::Screen;
use ultar_mobile::program::{lock::LockProgram, shell::ShellProgram, Options};
use ultar_surface::proto::{Rights, Role};
use ultar_ui::geom::Rect;

const W: u32 = 360;
const H: u32 = 760;
/// "falsch" and "geheim1" as key codes (US layout).
const FALSCH: [u16; 6] = [33, 30, 38, 31, 46, 35];
const GEHEIM1: [u16; 7] = [34, 18, 35, 18, 23, 50, 2];

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

/// What the test sees of the programs' threads.
#[derive(Default)]
struct Seen {
    screen: Option<Screen>,
    dnd: bool,
    shell: Vec<(Click, Rect)>,
    lock: Vec<(Click, Rect)>,
    field: Option<Rect>,
    lock_state: String,
    recents: Vec<String>,
}

const SHELL_CLICKS: [Click; 8] = [
    Click::Status,
    Click::Toggle(Toggle::Dnd),
    Click::Toggle(Toggle::Torch),
    Click::Toggle(Toggle::MobileData),
    Click::Home,
    Click::Recents,
    Click::Back,
    Click::Search,
];

struct World {
    comp: Compositor,
    bus: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    starts: Arc<Mutex<Vec<String>>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl World {
    fn new(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("umobile-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut rights = RightsTable::default();
        rights.grant(my_uid(), Rights::ALL);
        let config = Config {
            rights,
            inline_only: true,
            cursor: false,
            layouts: vec![ultar_compositor::keymap::Layout::English],
            lang: ultar_ui::Lang::En,
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
        let starts = Arc::new(Mutex::new(Vec::new()));
        services(&bus, &stop, &starts, tag != "noaccount");
        World {
            comp,
            bus,
            socket,
            stop,
            seen: Arc::new(Mutex::new(Seen::default())),
            starts,
            threads: Vec::new(),
        }
    }

    fn options(&self) -> Options {
        Options {
            compositor: self.socket.clone(),
            bus: Some(self.bus.clone()),
            lang: ultar_ui::Lang::En,
            light: false,
            user: Some("anas".into()),
        }
    }

    fn spawn_shell(&mut self) {
        let (options, stop, seen) = (self.options(), self.stop.clone(), self.seen.clone());
        self.threads.push(std::thread::spawn(move || {
            let mut app = ShellProgram::start(&options).unwrap();
            while !stop.load(Ordering::SeqCst) {
                if app.turn().is_err() {
                    break;
                }
                let mut s = seen.lock().unwrap();
                s.screen = Some(app.model.screen);
                s.dnd = app.model.quick.is_on(Toggle::Dnd);
                s.recents = app.model.recents.clone();
                s.shell = SHELL_CLICKS
                    .iter()
                    .chain(
                        [
                            Click::App("org.ultar.files".into()),
                            Click::App("org.ultar.settings".into()),
                        ]
                        .iter(),
                    )
                    .filter_map(|c| app.locate(c).map(|r| (c.clone(), r)))
                    .collect();
            }
        }));
    }

    fn spawn_lock(&mut self) {
        let (options, stop, seen) = (self.options(), self.stop.clone(), self.seen.clone());
        self.threads.push(std::thread::spawn(move || {
            let mut app = LockProgram::start(&options).unwrap();
            while !stop.load(Ordering::SeqCst) {
                if app.turn().is_err() {
                    break;
                }
                let mut s = seen.lock().unwrap();
                s.lock = [Click::Unlock, Click::Submit]
                    .iter()
                    .filter_map(|c| app.locate(c).map(|r| (c.clone(), r)))
                    .collect();
                s.field = app.field();
                s.lock_state = format!(
                    "{:?} {:?} typed {}",
                    app.state.stage,
                    app.state.error,
                    app.typed()
                );
            }
        }));
    }

    fn pump(&mut self, ms: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            self.comp.dispatch(Some(Duration::from_millis(5))).unwrap();
        }
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&mut World) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "timed out waiting for: {what}"
            );
            self.comp.dispatch(Some(Duration::from_millis(10))).unwrap();
        }
        self.pump(60);
    }

    fn input(&mut self, kind: InputEventKind) {
        self.comp.inject(&[InputEvent {
            device: InputDeviceId(0),
            time_us: 0,
            kind,
        }]);
    }

    fn tap_at(&mut self, x: f32, y: f32) {
        let abs = |v: f32, size: u32| ((v / size as f32) * 65536.0).clamp(0.0, 65535.0) as u16;
        self.input(InputEventKind::PointerAbsolute {
            x: abs(x, W),
            y: abs(y, H),
        });
        self.pump(40);
        for pressed in [true, false] {
            self.input(InputEventKind::PointerButton {
                button: PointerButton::Primary,
                pressed,
            });
            self.pump(40);
        }
    }

    fn rect(&self, click: &Click, lock: bool) -> Option<Rect> {
        let s = self.seen.lock().unwrap();
        let list = if lock { &s.lock } else { &s.shell };
        list.iter().find(|(c, _)| c == click).map(|(_, r)| *r)
    }

    fn tap(&mut self, click: &Click, lock: bool) {
        self.until(&format!("{click:?} shown"), |w| {
            w.rect(click, lock).is_some()
        });
        let r = self.rect(click, lock).unwrap();
        self.tap_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
    }

    fn screen(&self) -> Option<Screen> {
        self.seen.lock().unwrap().screen
    }

    fn key(&mut self, code: u16) {
        for state in [KeyState::Pressed, KeyState::Released] {
            self.input(InputEventKind::Key { code, state });
        }
        self.pump(30);
    }

    fn roles(&self) -> Vec<Role> {
        let c = &self.comp;
        c.state
            .layers
            .iter()
            .filter_map(|k| c.state.surface(*k).map(|s| s.role))
            .collect()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let start = Instant::now();
            while !t.is_finished() && start.elapsed() < Duration::from_secs(5) {
                let _ = self.comp.dispatch(Some(Duration::from_millis(10)));
            }
            let _ = t.join();
        }
    }
}

/// A launcher (lists four apps, records starts) and an authentication service (account
/// "anas", password "geheim1") on the bus.
fn services(
    bus: &std::path::Path,
    stop: &Arc<AtomicBool>,
    starts: &Arc<Mutex<Vec<String>>>,
    accounts: bool,
) {
    let (bus2, stop2, starts2) = (bus.to_owned(), stop.clone(), starts.clone());
    std::thread::spawn(move || {
        let client = Client::connect(&bus2, ClientOptions::named("launcher")).unwrap();
        let endpoint = client.register(LAUNCHER).unwrap();
        while !stop2.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if request.decode::<List>().is_ok() {
                let apps = ["settings", "files", "phone", "store", "browser"]
                    .iter()
                    .map(|a| AppStatus {
                        app: (*a).into(),
                        uid: 0,
                        profile: String::new(),
                        version: String::new(),
                        instances: Vec::new(),
                    })
                    .collect();
                let _ = request.reply(&ListReply { apps });
            } else if let Ok(s) = request.decode::<Start>() {
                starts2.lock().unwrap().push(s.app);
                let reply = ultar_bus::messages::launcher::StartReply {
                    state: ultar_bus::messages::launcher::InstanceState::Running,
                    pid: 1,
                    already: false,
                    opened: false,
                };
                let _ = request.reply(&reply);
            }
        }
    });
    auth(bus, stop, accounts);
    hardware(bus, stop, starts);
}

/// The power service's torch and the radio service's mobile data: every switch is recorded
/// in `asked` as `torch=on` / `data=on`.
fn hardware(bus: &std::path::Path, stop: &Arc<AtomicBool>, asked: &Arc<Mutex<Vec<String>>>) {
    use ultar_bus::messages::modem::{ModemData, ModemSetData};
    use ultar_bus::messages::power::{Torch, TorchReply};
    for service in ["power", "modem"] {
        let (bus2, stop2, asked2) = (bus.to_owned(), stop.clone(), asked.clone());
        std::thread::spawn(move || {
            let client = Client::connect(&bus2, ClientOptions::named(service)).unwrap();
            let endpoint = client.register(service).unwrap();
            while !stop2.load(Ordering::SeqCst) {
                let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                if let Ok(t) = request.decode::<Torch>() {
                    if let Some(on) = t.on {
                        asked2.lock().unwrap().push(format!("torch={on}"));
                    }
                    let _ = request.reply(&TorchReply {
                        present: true,
                        on: t.on.unwrap_or(false),
                    });
                } else if let Ok(d) = request.decode::<ModemSetData>() {
                    if let Some(on) = d.on {
                        asked2.lock().unwrap().push(format!("data={on}"));
                    }
                    let _ = request.reply(&ModemData {
                        on: d.on.unwrap_or(false),
                    });
                }
            }
        });
    }
}

/// The authentication service: account "anas", password "geheim1" (none with `accounts` off).
fn auth(bus: &std::path::Path, stop: &Arc<AtomicBool>, accounts: bool) {
    let (bus3, stop3) = (bus.to_owned(), stop.clone());
    std::thread::spawn(move || {
        let client = Client::connect(&bus3, ClientOptions::named("authd")).unwrap();
        let endpoint = client.register(AUTH).unwrap();
        while !stop3.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if request.decode::<Users>().is_ok() {
                let anas = UserInfo {
                    name: "anas".into(),
                    display_name: "Anas".into(),
                    has_pin: false,
                    admin: true,
                };
                let users = if accounts { vec![anas] } else { Vec::new() };
                let _ = request.reply(&UsersReply { users });
            } else if let Ok(v) = request.decode::<Verify>() {
                let outcome = if v.user == "anas" && v.secret.expose() == "geheim1" {
                    Outcome::Granted
                } else {
                    Outcome::Denied {
                        retry_after_ms: 300,
                    }
                };
                let _ = request.reply(&VerifyReply { outcome });
            }
        }
    });
}

#[test]
fn the_shell_shows_its_bars_and_home_and_every_tap_works() {
    let mut w = World::new("shell");
    w.spawn_shell();
    w.until("bars and home", |w| {
        let r = w.roles();
        r.iter().filter(|r| matches!(r, Role::Panel { .. })).count() == 2
            && r.contains(&Role::Background)
    });
    w.until("home", |w| w.screen() == Some(Screen::Home));
    // Status bar → quick settings as a sheet between the bars.
    w.tap(&Click::Status, false);
    w.until("quick settings", |w| w.screen() == Some(Screen::Quick));
    w.until("the sheet", |w| {
        w.roles().iter().any(|r| matches!(r, Role::Popup { .. }))
    });
    w.tap(&Click::Toggle(Toggle::Dnd), false);
    w.until("do not disturb on", |w| w.seen.lock().unwrap().dnd);
    // Torch (power service) and mobile data (radio service), as on the first Ultar phone.
    w.tap(&Click::Toggle(Toggle::Torch), false);
    w.until("torch on", |w| {
        w.starts.lock().unwrap().contains(&"torch=true".to_owned())
    });
    w.tap(&Click::Toggle(Toggle::MobileData), false);
    w.until("mobile data on", |w| {
        w.starts.lock().unwrap().contains(&"data=true".to_owned())
    });
    // Navigation bar: home closes the sheet.
    w.tap(&Click::Home, false);
    w.until("home again", |w| w.screen() == Some(Screen::Home));
    w.until("the sheet closed", |w| {
        !w.roles().iter().any(|r| matches!(r, Role::Popup { .. }))
    });
    // Recent apps and back.
    w.tap(&Click::Recents, false);
    w.until("recents", |w| w.screen() == Some(Screen::Recents));
    w.tap(&Click::Back, false);
    w.until("home after back", |w| w.screen() == Some(Screen::Home));
    an_apps_window_follows_the_shell(&mut w);
}

/// An app's window: it shows in the recent apps; home puts it away; its icon brings it back.
fn an_apps_window_follows_the_shell(w: &mut World) {
    let socket = w.socket.clone();
    let stop = w.stop.clone();
    w.threads.push(std::thread::spawn(move || {
        let mut c = ultar_surface::client::Client::connect(&socket, "org.ultar.files").unwrap();
        let config = ultar_surface::client::WindowConfig::toplevel("notes.txt");
        let mut win = ultar_surface::client::Window::create(&mut c, &config).unwrap();
        while !stop.load(Ordering::SeqCst) {
            if let Ok(Some(e)) = c.next_event(Some(Duration::from_millis(20))) {
                if win.handle(&e) {
                    if let (ultar_surface::proto::Event::Configure(cf), true) = (&e, win.can_draw())
                    {
                        let px = vec![0x80u8; (cf.width * cf.height * 4) as usize];
                        let _ = win.present(&mut c, &px, (cf.width, cf.height), &[]);
                    }
                }
            }
        }
    }));
    w.until("the window in the recent apps", |w| {
        w.seen.lock().unwrap().recents.first().map(String::as_str) == Some("org.ultar.files")
    });
    w.tap(&Click::Home, false);
    w.until("the window put away", |w| {
        w.comp
            .state
            .windows
            .iter()
            .all(|x| x.state == ultar_surface::proto::WindowState::Minimized)
    });
    // Its icon brings the window back instead of starting the app again.
    w.tap(&Click::App("org.ultar.files".into()), false);
    w.until("the window in front again", |w| {
        w.comp
            .state
            .windows
            .iter()
            .any(|x| x.state != ultar_surface::proto::WindowState::Minimized)
    });
    assert!(!w.starts.lock().unwrap().contains(&"files".to_owned()));
    // An app without a window starts through the launcher (its one-part id).
    w.tap(&Click::Home, false);
    w.tap(&Click::App("org.ultar.settings".into()), false);
    w.until("settings started", |w| {
        w.starts.lock().unwrap().contains(&"settings".to_owned())
    });
}

#[test]
fn the_lock_screen_lifts_the_lock_only_for_the_right_password() {
    let mut w = World::new("lock");
    w.spawn_lock();
    w.pump(300);
    let bus = w.bus.clone();
    let caller = std::thread::spawn(move || {
        Client::connect(&bus, ClientOptions::named("shell"))
            .unwrap()
            .call(&Lock {}, Duration::from_secs(10))
            .map(|r| r.locked)
    });
    w.until("locked", |w| w.comp.state.locked());
    while !caller.is_finished() {
        w.pump(20);
    }
    assert_eq!(caller.join().unwrap().ok(), Some(true));
    w.tap(&Click::Unlock, true);
    w.until("the sign-in", |w| w.seen.lock().unwrap().field.is_some());
    // Wrong: still locked.
    for c in FALSCH {
        w.key(c);
    }
    w.key(28);
    w.pump(800);
    assert!(w.comp.state.locked(), "a wrong password does not unlock");
    // Right, after the wait.
    w.pump(500);
    for c in GEHEIM1 {
        w.key(c);
    }
    w.key(28);
    let start = Instant::now();
    while w.comp.state.locked() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "not unlocked: {}",
            w.seen.lock().unwrap().lock_state
        );
        w.pump(20);
    }
}

/// A phone without any account (no installer ran): the lock opens by itself.
#[test]
fn a_phone_without_accounts_is_not_locked_out() {
    let mut w = World::new("noaccount");
    w.spawn_lock();
    w.pump(300);
    let bus = w.bus.clone();
    std::thread::spawn(move || {
        let _ = Client::connect(&bus, ClientOptions::named("shell"))
            .unwrap()
            .call(&Lock {}, Duration::from_secs(10));
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) && !w.comp.state.locked() {
        w.pump(20);
    }
    let start = Instant::now();
    while w.comp.state.locked() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "still locked: {}",
            w.seen.lock().unwrap().lock_state
        );
        w.pump(20);
    }
}
