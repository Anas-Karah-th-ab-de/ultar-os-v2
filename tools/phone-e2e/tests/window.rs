//! The phone in a real compositor (headless, in a thread): the app draws its own header
//! (`ClientHeader`, no compositor title bar), and a ringing call adds the call screen above
//! the windows (`Role::CallScreen`), which goes when the call is answered.

use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use ultar_bus::messages::call::{
    CallInfo, CallState, Cellular, Direction, Lines, LogEntry, Media, Phase, Ultar, Way,
};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_phone::port::{CallPort, PortError, Request};
use ultar_phone::{Look, PhoneApp, PhoneWindow, QueuedHost, Step};
use ultar_surface::client::Client;
use ultar_surface::proto::{Rights, Role};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::{Lang, Size};

struct NoPort;
impl CallPort for NoPort {
    fn send(&mut self, _r: Request) -> Result<Option<u32>, PortError> {
        Ok(None)
    }
    fn log(&mut self, _m: bool) -> Result<Vec<LogEntry>, PortError> {
        Ok(Vec::new())
    }
}

/// What the compositor shows: (window has a compositor title bar, call screens in the scene).
type Probe = (bool, usize);

fn compositor(
    stream: UnixStream,
    probes: mpsc::Receiver<mpsc::Sender<Probe>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut rights = RightsTable::default();
        rights.grant(
            ultar_compositor::server::peer_of(&stream).uid,
            Rights::CLIENT_HEADER.union(Rights::CALL_SCREEN),
        );
        let config = Config {
            rights,
            inline_only: true,
            cursor: false,
            ..Config::default()
        };
        let mut comp = Compositor::new(
            config,
            Box::new(Headless::new(&[(1280, 800)])),
            Box::new(MonotonicClock::default()),
        );
        comp.add_stream(stream);
        loop {
            comp.dispatch(Some(Duration::from_millis(5))).unwrap();
            match probes.try_recv() {
                Ok(reply) => {
                    let s = &comp.state;
                    let decorated = s.windows.iter().any(|w| w.decorated());
                    let screens = s
                        .scene()
                        .iter()
                        .filter(|p| s.surface(p.key).is_some_and(|x| x.role == Role::CallScreen))
                        .count();
                    reply.send((decorated, screens)).unwrap();
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    })
}

fn ready() -> CallState {
    CallState {
        lines: Lines {
            cellular: Cellular::Ready,
            sims: vec![1],
            ultar: Ultar::Ready,
            own_number: Some("UOS-1111-2222".into()),
        },
        ..CallState::default()
    }
}

fn ringing(phase: Phase) -> CallState {
    let call = CallInfo {
        id: 1,
        way: Way::Ultar,
        direction: Direction::Incoming,
        number: "UOS-4821-7730".into(),
        name: None,
        phase,
        media: Media::default(),
        encrypted: true,
        active_since_ms: Some(0),
        safety: None,
        key_changed: false,
    };
    CallState {
        calls: vec![call],
        ..ready()
    }
}

fn run(
    win: &mut PhoneWindow<NoPort>,
    probe: &mpsc::Sender<mpsc::Sender<Probe>>,
    f: impl Fn(Probe) -> bool,
) -> Probe {
    for _ in 0..400 {
        assert_eq!(
            win.step(Duration::from_millis(5), 0).unwrap(),
            Step::Running
        );
        let (tx, rx) = mpsc::channel();
        probe.send(tx).unwrap();
        let p = rx.recv().unwrap();
        if f(p) {
            return p;
        }
    }
    panic!("compositor never showed the expected state");
}

#[test]
fn client_header_and_call_screen() {
    let (a, b) = UnixStream::pair().unwrap();
    let (probe, probes) = mpsc::channel();
    let worker = compositor(a, probes);
    let client = Client::with_stream(b, "phone").unwrap();
    let host = QueuedHost::default();
    let look = Look {
        lang: Lang::Ar,
        theme: ThemeKind::Dark,
        mode: InputMode::Pointer,
        size: Size {
            w: 1280.0,
            h: 760.0,
        },
    };
    let app = PhoneApp::new(look, ready(), NoPort, host.clone()).unwrap();
    let mut win = PhoneWindow::open(client, app, host, "phone").unwrap();
    let p = run(&mut win, &probe, |(decorated, _)| !decorated);
    assert_eq!(
        p,
        (false, 0),
        "own header, no compositor title bar, no call screen"
    );
    win.app().set_state(ringing(Phase::Ringing), 0).unwrap();
    run(&mut win, &probe, |(_, screens)| screens == 1);
    win.app().set_state(ringing(Phase::Active), 0).unwrap();
    run(&mut win, &probe, |(_, screens)| screens == 0);
    drop(probe);
    worker.join().unwrap();
}

/// Settings through the launcher (`launcher.start` v2 with `settings://apps/org.ultar.phone`):
/// a fake launcher on a bus answers; without a launcher the app says it could not open them.
#[test]
fn settings_open_through_the_launcher() {
    use ultar_bus::{Broker, ClientOptions, FixedIdentity, Identity, Policy};
    let path = std::env::temp_dir().join(format!("phone-launch-{}.sock", std::process::id()));
    let auth = std::sync::Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start()
        .unwrap();
    let (seen, fake) = fake_launcher(&path);
    let (a, b) = UnixStream::pair().unwrap();
    let (probe, probes) = mpsc::channel();
    let worker = compositor(a, probes);
    let host = QueuedHost::default();
    let look = wide_look();
    let app = PhoneApp::new(look, ready(), NoPort, host.clone()).unwrap();
    let client = Client::with_stream(b, "phone").unwrap();
    let bus = ultar_bus::Client::connect(&path, ClientOptions::named("phone")).unwrap();
    let mut win = PhoneWindow::open(client, app, host, "phone")
        .unwrap()
        .with_launcher(bus);
    let gear = win
        .app()
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::Settings)
        .unwrap()
        .0;
    win.app().ui().layout();
    let pos = win.app().ui().rect(gear).unwrap().center();
    win.app()
        .handle(ultar_ui::event::Event::PointerDown {
            pos,
            button: ultar_ui::event::PointerButton::Primary,
        })
        .unwrap();
    win.app()
        .handle(ultar_ui::event::Event::PointerUp {
            pos,
            button: ultar_ui::event::PointerButton::Primary,
        })
        .unwrap();
    assert_eq!(
        win.step(Duration::from_millis(5), 0).unwrap(),
        Step::Running
    );
    let start = seen.recv_timeout(Duration::from_secs(20)).unwrap();
    // The phone asks for the Settings manifest id, without a URI (Settings declares no URI
    // scheme yet, R2-4).
    assert_eq!(start.app, ultar_phone::launch::SETTINGS_APP);
    assert_eq!(start.uri, None);
    fake.join().unwrap();
    assert_eq!(win.app().toast_key(), None);
    drop(win);
    drop(probe);
    worker.join().unwrap();
    drop(broker);
}

/// A launcher that answers one `launcher.start` v2 and hands the request to the test.
fn fake_launcher(
    path: &std::path::Path,
) -> (
    mpsc::Receiver<ultar_bus::messages::launcher::Start>,
    thread::JoinHandle<()>,
) {
    use ultar_bus::messages::launcher::{InstanceState, Start, StartReply};
    use ultar_bus::ClientOptions;
    let launcher = ultar_bus::Client::connect(path, ClientOptions::named("launcher")).unwrap();
    let inbox = launcher.register("launcher").unwrap();
    let (seen_tx, seen) = mpsc::channel();
    let fake = thread::spawn(move || {
        let _keep = launcher;
        let request = inbox.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!((request.method(), request.version()), ("start", 2));
        request
            .respond::<Start>(|s| {
                seen_tx.send(s).unwrap();
                Ok(StartReply {
                    state: InstanceState::Running,
                    pid: 9,
                    already: false,
                    opened: true,
                })
            })
            .unwrap();
    });
    (seen, fake)
}

/// Arabic, dark, pointer, the boards' window content.
fn wide_look() -> Look {
    let size = Size {
        w: 1280.0,
        h: 760.0,
    };
    Look {
        lang: Lang::Ar,
        theme: ThemeKind::Dark,
        mode: InputMode::Pointer,
        size,
    }
}
