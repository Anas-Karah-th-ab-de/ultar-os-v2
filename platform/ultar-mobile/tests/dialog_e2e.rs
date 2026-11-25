//! End to end on Linux with the real pieces in one test process: the compositor (headless
//! phone-sized output, phone mode switched on by a stand-in shell), the bus broker, the
//! phone's dialog program (`ultar-mobile-dialog`) on its own thread, and a stand-in mesh that
//! calls `sysdialog.handoff`. A finger (pointer input injected into the compositor) taps
//! Accept or Decline on the sheet; the mesh gets the person's answer.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{DeviceKind, Stage};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, PointerButton};
use ultar_mobile::dialog::DialogClick;
use ultar_mobile::program::{dialog::DialogProgram, Options};
use ultar_surface::proto::{Request, Rights};
use ultar_ui::geom::Rect;

const W: u32 = 360;
const H: u32 = 760;

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

/// Where the program shows its buttons (surface points) and how many questions wait.
#[derive(Default)]
struct Seen {
    buttons: Vec<(DialogClick, Rect)>,
    waiting: usize,
}

struct World {
    comp: Compositor,
    bus: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl World {
    fn new(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("umdialog-{}-{tag}", std::process::id()));
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
        World {
            comp,
            bus,
            socket,
            stop: Arc::new(AtomicBool::new(false)),
            seen: Arc::new(Mutex::new(Seen::default())),
            threads: Vec::new(),
        }
    }

    /// A stand-in shell that switches the compositor to phone mode and stays connected.
    fn spawn_phone_mode(&mut self) {
        let (socket, stop) = (self.socket.clone(), self.stop.clone());
        self.threads.push(std::thread::spawn(move || {
            let mut shell = ultar_surface::client::Client::connect(&socket, "shell").unwrap();
            shell.send(&Request::SetPhoneMode { on: true }).unwrap();
            while !stop.load(Ordering::SeqCst) {
                if shell.next_event(Some(Duration::from_millis(50))).is_err() {
                    break;
                }
            }
        }));
        self.until("phone mode", |w| w.comp.state.session.shell.phone.on);
    }

    fn spawn_dialog(&mut self) {
        let options = Options {
            compositor: self.socket.clone(),
            bus: Some(self.bus.clone()),
            lang: ultar_ui::Lang::En,
            light: false,
            user: None,
        };
        let (stop, seen) = (self.stop.clone(), self.seen.clone());
        self.threads.push(std::thread::spawn(move || {
            let mut app = DialogProgram::start(&options).unwrap();
            while !stop.load(Ordering::SeqCst) {
                if app.turn().is_err() {
                    break;
                }
                let mut s = seen.lock().unwrap();
                s.buttons = [DialogClick::Accept, DialogClick::Safe]
                    .into_iter()
                    .filter_map(|c| app.locate(c).map(|r| (c, r)))
                    .collect();
                s.waiting = app.dialogs.queue.len();
            }
        }));
    }

    /// A stand-in mesh asks the question; the thread ends with the answer.
    fn ask(&self) -> std::thread::JoinHandle<Option<bool>> {
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            let client = Client::connect(&bus, ClientOptions::named("mesh")).unwrap();
            let offer = AskHandoff {
                from_name: "ultar-pc".into(),
                from_kind: DeviceKind::Desktop,
                title: "notes.txt".into(),
                app: "editor".into(),
                size: Some(1_258_291),
                stage: Stage::State,
            };
            client
                .call(&offer, Duration::from_secs(30))
                .ok()
                .map(|r| r.accept)
        })
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

    /// The dialog's window: its content on the screen.
    fn dialog_content(&self) -> Option<ultar_compositor::geom::Rect> {
        let w = self.comp.state.windows.iter().find(|w| w.secure)?;
        w.mapped.then(|| w.content())
    }

    /// Taps `click` on the sheet (surface points + the window's place on the screen).
    fn tap(&mut self, click: DialogClick) {
        // Shown, and laid out for the window's size (not the first guess of the screen's).
        self.until(&format!("{click:?} shown"), |w| {
            let Some(content) = w.dialog_content() else {
                return false;
            };
            let s = w.seen.lock().unwrap();
            s.buttons
                .iter()
                .any(|(c, r)| *c == click && r.y + r.h <= content.h as f32)
        });
        let content = self.dialog_content().unwrap();
        let r = {
            let s = self.seen.lock().unwrap();
            s.buttons.iter().find(|(c, _)| *c == click).unwrap().1
        };
        let x = content.x as f32 + r.x + r.w / 2.0;
        let y = content.y as f32 + r.y + r.h / 2.0;
        self.tap_at(x, y);
    }

    fn answer(&mut self, caller: std::thread::JoinHandle<Option<bool>>) -> Option<bool> {
        let start = Instant::now();
        while !caller.is_finished() {
            assert!(start.elapsed() < Duration::from_secs(20), "no answer");
            self.pump(20);
        }
        caller.join().unwrap()
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

fn offered_and_tapped(tag: &str, click: DialogClick) -> Option<bool> {
    let mut w = World::new(tag);
    w.spawn_phone_mode();
    w.spawn_dialog();
    w.pump(200);
    let caller = w.ask();
    w.until("the sheet", |w| !w.seen.lock().unwrap().buttons.is_empty());
    // The sheet covers the work area under the trusted title bar.
    let title = ultar_compositor::decor::TITLEBAR;
    w.until("the sheet over the work area", |w| {
        w.dialog_content()
            .is_some_and(|c| (c.w, c.h) == (W as i32, H as i32 - title))
    });
    w.tap(click);
    let answer = w.answer(caller);
    w.until("the question closed", |w| {
        w.seen.lock().unwrap().waiting == 0 && w.dialog_content().is_none()
    });
    answer
}

#[test]
fn accept_on_the_sheet_takes_the_offer() {
    assert_eq!(
        offered_and_tapped("accept", DialogClick::Accept),
        Some(true)
    );
}

#[test]
fn decline_on_the_sheet_refuses_the_offer() {
    assert_eq!(
        offered_and_tapped("decline", DialogClick::Safe),
        Some(false)
    );
}
