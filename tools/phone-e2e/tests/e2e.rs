//! Incoming call → answer → hold → end, through the real bus: the phone app's clicks become
//! `call.*` requests, the call service (fake line) changes the call, `call.state` comes back
//! and the app shows it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use ultar_bus::messages::call::{CallState, Cellular, End, Output, Phase, Ultar, Way};
use ultar_bus::{
    Broker, Client, ClientOptions, FixedIdentity, Identity, Policy, TypedSubscription,
};
use ultar_call::fake::{FakeAudio, FakeControl, FakeLine, ManualClock};
use ultar_call::service::Service;
use ultar_call::session::Session;
use ultar_phone::port::Host;
use ultar_phone::{BusPort, Click, Look, PhoneApp, Screen, WindowOp};
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::{Lang, Size};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct NoHost;
impl Host for NoHost {
    fn copy_text(&mut self, _t: &str) -> bool {
        true
    }
    fn window(&mut self, _op: WindowOp) {}
    fn open_settings(&mut self) {}
    fn create_contact(&mut self, _n: &str) {}
}

type App = PhoneApp<BusPort, NoHost>;

fn click(app: &mut App, want: Click) {
    let id = app
        .clicks()
        .iter()
        .find(|(_, c)| *c == want)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("{want:?} shown"));
    app.ui().layout();
    let pos = app.ui().rect(id).unwrap().center();
    app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
}

/// Feeds states to the app until `f` holds.
fn until(app: &mut App, states: &TypedSubscription<CallState>, f: impl Fn(&CallState) -> bool) {
    loop {
        let st = states.recv_timeout(WAIT).unwrap();
        let done = f(&st);
        app.set_state(st, 0).unwrap();
        if done {
            return;
        }
    }
}

struct Rig {
    _broker: Broker,
    control: FakeControl,
    app: App,
    states: TypedSubscription<CallState>,
    stop: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

/// Broker, call service with the fake line in a thread, and the phone app on the bus.
fn start() -> Rig {
    let path = std::env::temp_dir().join(format!("phone-e2e-{}.sock", std::process::id()));
    let auth = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start()
        .unwrap();
    let (line, control) = FakeLine::new(&[Way::Ultar, Way::Cellular(1)]);
    control.set_ultar(Ultar::Ready, Some("UOS-1111-2222"));
    control.set_cellular(Cellular::Ready, &[1]);
    let session = Session::new(
        vec![Box::new(line)],
        Box::new(FakeAudio::new(&[Output::Earpiece, Output::Speaker])),
        Box::new(ManualClock::default()),
    );
    let bus = Client::connect(&path, ClientOptions::named("calld")).unwrap();
    let mut service = Service::new(bus, session, Duration::from_millis(5)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            service.step().unwrap();
        }
    });

    let client = Client::connect(&path, ClientOptions::named("phone")).unwrap();
    let port = BusPort::new(client, WAIT);
    let states = port.states().unwrap();
    let look = Look {
        lang: Lang::Ar,
        theme: ThemeKind::Dark,
        mode: InputMode::Touch,
        size: Size { w: 360.0, h: 720.0 },
    };
    let mut app = PhoneApp::new(look, CallState::default(), port, NoHost).unwrap();
    until(&mut app, &states, |s| {
        s.lines.ultar == Ultar::Ready && s.lines.cellular == Cellular::Ready
    });
    assert_eq!(app.model().screen(), Screen::Dial);

    Rig {
        _broker: broker,
        control,
        app,
        states,
        stop,
        worker,
    }
}

#[test]
fn incoming_answer_hold_end() {
    let Rig {
        _broker,
        control,
        mut app,
        states,
        stop,
        worker,
    } = start();
    control.ring(Way::Cellular(1), "+963 11 555 0142", Some("ليلى حداد"));
    until(&mut app, &states, |s| s.calls.len() == 1);
    let id = app.model().state.calls[0].id;
    assert_eq!(app.model().screen(), Screen::Incoming(id));

    click(&mut app, Click::Accept(id));
    until(&mut app, &states, |s| {
        s.calls.first().is_some_and(|c| c.phase == Phase::Active)
    });
    assert_eq!(app.model().screen(), Screen::InCall(id));

    click(&mut app, Click::Hold(id));
    until(&mut app, &states, |s| {
        s.calls.first().is_some_and(|c| c.phase == Phase::Held)
    });

    click(&mut app, Click::Hangup(id));
    until(&mut app, &states, |s| s.calls.is_empty());
    assert_eq!(app.model().screen(), Screen::Dial);
    let ended = app.model().state.ended.clone().unwrap();
    assert_eq!(ended.phase, Phase::Ended(End::Local));
    assert_eq!(
        app.model().log.len(),
        1,
        "history reloaded from the service"
    );
    assert!(app.clicks().iter().any(|(_, c)| *c == Click::Again));

    stop.store(true, Ordering::SeqCst);
    worker.join().unwrap();
}
