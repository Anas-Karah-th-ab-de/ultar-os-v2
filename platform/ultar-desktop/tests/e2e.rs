//! End to end on Linux with the real pieces in one test process: the compositor (headless
//! output), the bus broker, the authentication service and the shell's programs as clients
//! on their own threads over real sockets. The lock screen's security properties
//! (DESKTOP-SPEC §5.3; COMPOSITOR.md §8a): no input reaches an app while locked, the lock
//! survives a dying shell and a dying lock screen, wrong passwords wait longer, a typed
//! password is not kept; the bar appears as a panel and opens its sheets.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::auth::{
    Outcome as AuthOutcome, UserInfo, Users, UsersReply, Verify, VerifyReply, SERVICE as AUTH,
};
use ultar_bus::messages::lock::Lock;
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_compositor::backend::Headless;
use ultar_compositor::keymap::Layout;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_desktop::app::Options;
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState};
use ultar_surface::client::{Client as Surface, Window, WindowConfig};
use ultar_surface::proto::{Event, Rights, Role};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("udesk-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

struct World {
    comp: Compositor,
    socket: PathBuf,
    bus: PathBuf,
    _broker: Broker,
    stop: Arc<AtomicBool>,
    /// The authentication service's clock (its waits pass only when the test says so).
    auth_clock: Arc<AtomicU64>,
}

fn world(tag: &str) -> World {
    let dir = tmp(tag);
    let mut rights = RightsTable::default();
    rights.grant(my_uid(), Rights::ALL);
    let config = Config {
        rights,
        inline_only: true,
        cursor: false,
        layouts: vec![Layout::English, Layout::Arabic],
        lang: ultar_ui::Lang::En,
        ..Config::default()
    };
    let mut comp = Compositor::new(
        config,
        Box::new(Headless::new(&[(1280, 800)])),
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
    // The authentication service (its crate has its own tests in system/services/ultar-auth;
    // a platform crate may not depend on it): one account, one second of waiting after a
    // wrong password, on a clock the test moves.
    let stop = Arc::new(AtomicBool::new(false));
    let auth_clock = Arc::new(AtomicU64::new(1_000_000));
    let (bus2, stop2, clock2) = (bus.clone(), stop.clone(), auth_clock.clone());
    std::thread::spawn(move || fake_auth(&bus2, &stop2, &clock2));
    World {
        comp,
        socket,
        bus,
        _broker: broker,
        stop,
        auth_clock,
    }
}

/// A stand-in for `ultar-authd` on the bus: account "anas" with the password "geheim1".
fn fake_auth(bus: &std::path::Path, stop: &AtomicBool, clock: &AtomicU64) {
    let client = Client::connect(bus, ClientOptions::named("authd")).unwrap();
    let endpoint = client.register(AUTH).unwrap();
    let mut wait_until = 0;
    while !stop.load(Ordering::SeqCst) {
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
            let _ = request.reply(&UsersReply { users: vec![anas] });
        } else if let Ok(v) = request.decode::<Verify>() {
            let now = clock.load(Ordering::SeqCst);
            let outcome = if now < wait_until {
                AuthOutcome::Throttled {
                    retry_after_ms: wait_until - now,
                }
            } else if v.user == "anas" && v.secret.expose() == "geheim1" {
                AuthOutcome::Granted
            } else {
                wait_until = now + 1_000;
                AuthOutcome::Denied {
                    retry_after_ms: 1_000,
                }
            };
            let _ = request.reply(&VerifyReply { outcome });
        }
    }
}

impl World {
    fn options(&self) -> Options {
        Options {
            compositor: self.socket.clone(),
            bus: Some(self.bus.clone()),
            lang: ultar_i18n::Lang::En,
            light: false,
        }
    }

    /// Runs the compositor until `done` or 20 s.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Compositor) -> bool) {
        let start = Instant::now();
        while !done(&mut self.comp) {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "timed out waiting for: {what}"
            );
            self.comp.dispatch(Some(Duration::from_millis(10))).unwrap();
        }
    }

    fn pump(&mut self, ms: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            self.comp.dispatch(Some(Duration::from_millis(5))).unwrap();
        }
    }

    /// Joins a client thread while the compositor keeps serving it (a client may be in the
    /// middle of sending a frame).
    fn join<T>(&mut self, thread: std::thread::JoinHandle<T>) -> T {
        let start = Instant::now();
        while !thread.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "a client thread does not end"
            );
            self.pump(10);
        }
        thread.join().unwrap()
    }

    fn key(&mut self, code: u16) {
        for state in [KeyState::Pressed, KeyState::Released] {
            self.comp.inject(&[InputEvent {
                device: InputDeviceId(0),
                time_us: 0,
                kind: InputEventKind::Key { code, state },
            }]);
        }
        self.pump(30);
    }

    fn type_text(&mut self, codes: &[u16]) {
        for c in codes {
            self.key(*c);
        }
    }
}

fn spawn_lock(w: &World, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<String> {
    let options = w.options();
    std::thread::spawn(move || {
        let mut app = ultar_desktop::app::lock::LockApp::start(
            &options,
            Some("anas".into()),
            Arc::new(ultar_desktop::model::time::SystemClock::default()),
        )
        .unwrap();
        let r = app.run(&|| stop.load(Ordering::SeqCst));
        assert!(r.is_ok() || stop.load(Ordering::SeqCst), "{r:?}");
        format!("{app:?}")
    })
}

/// The shell asks to lock (`lock.lock`): the answer comes once the lock surface is up; a
/// shell client that comes and goes changes nothing.
fn lock_through_the_bus_and_survive_a_dying_shell(w: &mut World) {
    // The shell asks to lock (lock.lock).
    let bus = w.bus.clone();
    let caller = std::thread::spawn(move || {
        Client::connect(&bus, ClientOptions::named("shell"))
            .unwrap()
            .call(&Lock {}, Duration::from_secs(10))
            .map(|r| r.locked)
    });
    w.until("locked", |c| c.state.locked());
    w.pump(200);
    assert_eq!(caller.join().unwrap().ok(), Some(true));
    // A shell that dies does not unlock.
    let socket = w.socket.clone();
    let shell = std::thread::spawn(move || {
        Surface::connect(&socket, "org.ultar.desktop")
            .map(drop)
            .is_ok()
    });
    while !shell.is_finished() {
        w.pump(20);
    }
    assert!(
        shell.join().unwrap(),
        "a shell client connects (and goes away again)"
    );
}

/// Has the lock screen program said hello to the compositor?
fn lock_client(c: &mut Compositor) -> bool {
    c.state
        .clients
        .values()
        .any(|x| x.hello && x.app_id == ultar_desktop::app::lock::APP_ID)
}

/// A plain app window in its own thread; returns the keys it saw.
fn spawn_app(w: &World, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<u16>> {
    let socket = w.socket.clone();
    std::thread::spawn(move || {
        let mut c = Surface::connect(&socket, "org.test.app").unwrap();
        let mut win = Window::create(&mut c, &WindowConfig::toplevel("app")).unwrap();
        let mut keys = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            match c.next_event(Some(Duration::from_millis(20))) {
                Ok(Some(e)) => {
                    if win.handle(&e) {
                        if let (Event::Configure(cf), true) = (&e, win.can_draw()) {
                            let px = vec![0x80u8; (cf.width * cf.height * 4) as usize];
                            let _ = win.present(&mut c, &px, (cf.width, cf.height), &[]);
                        }
                    } else if let Event::Key { code, state: 1, .. } = e {
                        keys.push(code);
                    }
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
        keys
    })
}

// Key codes (English layout).
const ENTER: u16 = 28;
const FALSCH: [u16; 6] = [33, 30, 38, 31, 46, 35];
const GEHEIM1: [u16; 7] = [34, 18, 35, 18, 23, 50, 2];

#[test]
fn locked_session_keeps_input_from_apps_and_only_the_password_unlocks() {
    let mut w = world("lock");
    let app_stop = Arc::new(AtomicBool::new(false));
    let app = spawn_app(&w, app_stop.clone());
    w.until("app window", |c| c.state.windows.iter().any(|x| x.mapped));
    let lock_stop = Arc::new(AtomicBool::new(false));
    let lock = spawn_lock(&w, lock_stop.clone());
    // (All clients of this test share one uid and so all rights: wait for the lock screen
    // itself, which registers `lock` on the bus before it greets the compositor.)
    w.until("lock client", lock_client);
    lock_through_the_bus_and_survive_a_dying_shell(&mut w);
    w.pump(50);
    assert!(w.comp.state.locked());
    // Keys while locked: the cover, then a wrong password.
    w.key(57);
    w.type_text(&FALSCH);
    w.key(ENTER);
    w.pump(400);
    assert!(w.comp.state.locked(), "a wrong password does not unlock");
    // The lock screen dies while locked: still locked, nothing reaches the app.
    lock_stop.store(true, Ordering::SeqCst);
    let debug = w.join(lock);
    assert!(
        !debug.contains("falsch") && !debug.contains("geheim"),
        "{debug}"
    );
    w.pump(100);
    assert!(w.comp.state.locked());
    w.type_text(&GEHEIM1);
    // A new lock screen takes over; after the wait of the wrong try the right one unlocks.
    let lock_stop = Arc::new(AtomicBool::new(false));
    let lock = spawn_lock(&w, lock_stop.clone());
    w.until("new lock client", |c| {
        lock_client(c) && c.state.lock_surface().is_some()
    });
    // The wait after the wrong try (1 s) passes.
    w.auth_clock.fetch_add(1_500, Ordering::SeqCst);
    w.pump(1200);
    w.key(57);
    w.type_text(&GEHEIM1);
    w.key(ENTER);
    w.until("unlocked", |c| !c.state.locked());
    // Now the app gets keys again.
    w.until("app has the focus", |c| c.state.focus.is_some());
    w.key(30);
    w.pump(100);
    lock_stop.store(true, Ordering::SeqCst);
    app_stop.store(true, Ordering::SeqCst);
    let _ = w.join(lock);
    let keys = w.join(app);
    assert_eq!(
        keys,
        vec![30],
        "the app saw only the key after unlocking: {keys:?}"
    );
    w.stop.store(true, Ordering::SeqCst);
}

#[test]
fn the_bar_is_a_panel_and_super_opens_the_overview() {
    let mut w = world("bar");
    let stop = Arc::new(AtomicBool::new(false));
    let options = w.options();
    let stop2 = stop.clone();
    let shell = std::thread::spawn(move || {
        let mut app = ultar_desktop::app::desktop::DesktopApp::start(
            &options,
            "anas",
            Arc::new(ultar_desktop::model::time::SystemClock::default()),
        )
        .unwrap();
        let _ = app.run(&|| stop2.load(Ordering::SeqCst));
        app.shell().d.open
    });
    let panel = |c: &mut Compositor| {
        c.state.layers.iter().any(|k| {
            matches!(
                c.state.surface(*k).map(|s| s.role),
                Some(Role::Panel { size: 56, .. })
            ) && c.state.surface(*k).is_some_and(|s| s.image.is_some())
        })
    };
    w.until("the bar", panel);
    // The work area leaves the bar's 56 points free.
    let out = w.comp.state.layout.primary().unwrap().id;
    assert_eq!(w.comp.state.work_area(out).h, 800 - 56);
    // Super alone: the overview, a popup of the panel with content.
    w.key(125);
    let popup = |c: &mut Compositor| {
        c.state.layers.iter().any(|k| {
            matches!(
                c.state.surface(*k).map(|s| s.role),
                Some(Role::Popup { .. })
            ) && c.state.surface(*k).is_some_and(|s| s.image.is_some())
        })
    };
    w.until("the overview", popup);
    assert!(
        w.comp.state.session.shell.focus.is_some(),
        "the overview has the keyboard"
    );
    // Esc closes it.
    w.key(1);
    w.until("closed", |c| {
        !c.state.layers.iter().any(|k| {
            matches!(
                c.state.surface(*k).map(|s| s.role),
                Some(Role::Popup { .. })
            )
        })
    });
    stop.store(true, Ordering::SeqCst);
    w.pump(600);
    assert_eq!(shell.join().unwrap(), ultar_desktop::model::Open::None);
    w.stop.store(true, Ordering::SeqCst);
}
