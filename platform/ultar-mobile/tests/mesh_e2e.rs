//! The mesh on the phone end to end: real headless compositor (360 × 760), real bus, a
//! stand-in for the mesh service with two connected devices. A finger opens the recent apps,
//! taps "continue on another device", gets the choice (two devices), picks the second and
//! sends; the mesh gets the activity and the chosen device; its progress "opened" becomes the
//! notice in the status bar.

mod common;

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{
    Done, HandoffProgress, HandoffSend, OfferState, SendOutcome, SetActivity, SERVICE,
};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, PointerButton};
use ultar_mobile::click::Click;
use ultar_mobile::model::Screen;
use ultar_mobile::program::{shell::ShellProgram, Options};
use ultar_surface::proto::Rights;
use ultar_ui::geom::Rect;

const W: u32 = 360;
const H: u32 = 760;

#[derive(Default)]
struct Seen {
    screen: Option<Screen>,
    notice: Option<&'static str>,
    rects: Vec<(Click, Rect)>,
}

#[derive(Default)]
struct Mesh {
    activity: Option<String>,
    targets: Vec<Option<String>>,
}

/// Compositor, stop flag, what the shell shows, what the mesh got, the shell's thread.
type World = (
    Compositor,
    Arc<AtomicBool>,
    Arc<Mutex<Seen>>,
    Arc<Mutex<Mesh>>,
    std::thread::JoinHandle<()>,
);

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

/// The mesh stand-in: state with two connected devices; sending without a target offers the
/// choice, with one sends there and reports "opened".
fn mesh(bus: std::path::PathBuf, stop: Arc<AtomicBool>, log: Arc<Mutex<Mesh>>) {
    std::thread::spawn(move || {
        let client = Client::connect(&bus, ClientOptions::named("mesh")).unwrap();
        let endpoint = client.register(SERVICE).unwrap();
        client.publish(&common::mesh_state(true)).unwrap();
        while !stop.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if let Ok(set) = request.decode::<SetActivity>() {
                log.lock().unwrap().activity = set.activity.map(|a| a.app);
                let _ = request.reply(&Done { ok: true });
            } else if let Ok(send) = request.decode::<HandoffSend>() {
                log.lock().unwrap().targets.push(send.target.clone());
                let reply = match send.target {
                    None => SendOutcome::Choose(common::targets()),
                    Some(device) => {
                        let target = common::targets()
                            .into_iter()
                            .find(|t| t.device == device)
                            .unwrap();
                        SendOutcome::Offered { offer: 7, target }
                    }
                };
                let offered = matches!(reply, SendOutcome::Offered { .. });
                let _ = request.reply(&reply);
                if offered {
                    std::thread::sleep(Duration::from_millis(300));
                    let p = HandoffProgress {
                        offer: 7,
                        device: "beelink".into(),
                        outgoing: true,
                        state: OfferState::Opened,
                    };
                    let _ = client.publish(&p);
                }
            }
        }
    });
}

/// A headless compositor listening at `socket`.
fn compositor(socket: &std::path::Path) -> Compositor {
    let mut rights = RightsTable::default();
    rights.grant(my_uid(), Rights::ALL);
    let config = Config {
        rights,
        inline_only: true,
        cursor: false,
        lang: ultar_ui::Lang::Ar,
        ..Config::default()
    };
    let mut comp = Compositor::new(
        config,
        Box::new(Headless::new(&[(W, H)])),
        Box::new(MonotonicClock::default()),
    );
    comp.set_listener(UnixListener::bind(socket).unwrap())
        .unwrap();
    comp
}

fn start() -> World {
    let dir = std::env::temp_dir().join(format!("umobile-mesh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("c.sock");
    let comp = compositor(&socket);
    let bus = dir.join("bus.sock");
    let identity = Arc::new(FixedIdentity::new(Identity::new("ui.test").unwrap()));
    std::mem::forget(
        Broker::builder(Policy::allow_all())
            .listen(&bus, identity)
            .start()
            .unwrap(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Mesh::default()));
    mesh(bus.clone(), stop.clone(), log.clone());
    let seen = Arc::new(Mutex::new(Seen::default()));
    let options = Options {
        compositor: socket,
        bus: Some(bus),
        lang: ultar_ui::Lang::Ar,
        light: false,
        user: None,
    };
    let (stop2, seen2) = (stop.clone(), seen.clone());
    let shell = std::thread::spawn(move || {
        let mut app = ShellProgram::start(&options).unwrap();
        app.model.recents = vec!["org.ultar.files".into()];
        let watched = [
            Click::Recents,
            Click::MeshSend("org.ultar.files".into()),
            Click::MeshChoose(1),
            Click::MeshConfirm,
        ];
        while !stop2.load(Ordering::SeqCst) {
            if app.turn().is_err() {
                break;
            }
            let mut s = seen2.lock().unwrap();
            s.screen = Some(app.model.screen);
            s.notice = app.model.mesh.notice_at(app.model.now_ms);
            s.rects = watched
                .iter()
                .filter_map(|c| app.locate(c).map(|r| (c.clone(), r)))
                .collect();
        }
    });
    (comp, stop, seen, log, shell)
}

fn pump(comp: &mut Compositor, ms: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        comp.dispatch(Some(Duration::from_millis(5))).unwrap();
    }
}

fn until(comp: &mut Compositor, what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for: {what}"
        );
        comp.dispatch(Some(Duration::from_millis(10))).unwrap();
    }
    pump(comp, 80);
}

fn tap(comp: &mut Compositor, seen: &Arc<Mutex<Seen>>, click: &Click) {
    let find = || {
        seen.lock()
            .unwrap()
            .rects
            .iter()
            .find(|(c, _)| c == click)
            .map(|(_, r)| *r)
    };
    until(comp, &format!("{click:?} shown"), || find().is_some());
    let r = find().unwrap();
    let abs = |v: f32, size: u32| ((v / size as f32) * 65536.0).clamp(0.0, 65535.0) as u16;
    let input = |comp: &mut Compositor, kind| {
        comp.inject(&[InputEvent {
            device: InputDeviceId(0),
            time_us: 0,
            kind,
        }]);
    };
    input(
        comp,
        InputEventKind::PointerAbsolute {
            x: abs(r.x + r.w / 2.0, W),
            y: abs(r.y + r.h / 2.0, H),
        },
    );
    pump(comp, 40);
    for pressed in [true, false] {
        input(
            comp,
            InputEventKind::PointerButton {
                button: PointerButton::Primary,
                pressed,
            },
        );
        pump(comp, 40);
    }
}

#[test]
fn continue_an_app_on_the_chosen_device() {
    let (mut comp, stop, seen, log, shell) = start();
    until(&mut comp, "home", || {
        seen.lock().unwrap().screen == Some(Screen::Home)
    });
    tap(&mut comp, &seen, &Click::Recents);
    until(&mut comp, "recents", || {
        seen.lock().unwrap().screen == Some(Screen::Recents)
    });
    tap(&mut comp, &seen, &Click::MeshSend("org.ultar.files".into()));
    until(&mut comp, "the choice", || {
        seen.lock().unwrap().screen == Some(Screen::Mesh)
    });
    assert_eq!(
        log.lock().unwrap().activity.as_deref(),
        Some("org.ultar.files")
    );
    tap(&mut comp, &seen, &Click::MeshChoose(1));
    tap(&mut comp, &seen, &Click::MeshConfirm);
    until(&mut comp, "sent to beelink", || {
        log.lock().unwrap().targets == vec![None, Some("beelink".to_owned())]
    });
    until(&mut comp, "home after sending", || {
        seen.lock().unwrap().screen == Some(Screen::Home)
    });
    until(&mut comp, "the notice", || {
        seen.lock().unwrap().notice == Some("mobile.mesh.opened")
    });
    stop.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while !shell.is_finished() && start.elapsed() < Duration::from_secs(5) {
        let _ = comp.dispatch(Some(Duration::from_millis(10)));
    }
}
