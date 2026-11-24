//! The mobile network through the modem service on a real bus: the call service's
//! `ModemLine` against the fake modem (`modem.*`): incoming → answer → hold → tone → mute →
//! hang up, an outgoing call that the other side answers, and a call ended by the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use ultar_bus::messages::call::{
    Answer, CallState, Cellular, Dial, Dtmf, End, Hangup, Hold, Mute, Others, Phase, Way,
};
use ultar_bus::messages::modem::ModemCallStatus;
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_call::fake::ManualClock;
use ultar_call::fake_modem::{FakeModem, FakeModemControl};
use ultar_call::modem_line::{ModemAudio, ModemLine};
use ultar_call::service::Service;
use ultar_call::session::Session;

const WAIT: Duration = Duration::from_secs(10);
const SIM: Way = Way::Cellular(1);

fn wait(app: &Client, f: impl Fn(&CallState) -> bool) -> CallState {
    let sub = app.subscribe::<CallState>().unwrap();
    loop {
        let st = sub.recv_timeout(WAIT).unwrap();
        if f(&st) {
            return st;
        }
    }
}

fn eventually(f: impl Fn() -> bool) {
    for _ in 0..1000 {
        if f() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("never happened");
}

struct Rig {
    _broker: Broker,
    app: Client,
    modem: FakeModemControl,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

/// Broker, fake modem and call service (each in a thread), and a client.
fn start() -> Rig {
    let path = std::env::temp_dir().join(format!("ucall-modem-{}.sock", std::process::id()));
    let auth = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start()
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let modem = FakeModemControl::default();
    modem.set_cellular(Cellular::Ready, &[1]);
    let mut fake = FakeModem::new(
        Client::connect(&path, ClientOptions::named("ril")).unwrap(),
        modem.clone(),
    )
    .unwrap();
    let flag = stop.clone();
    let m = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            fake.step(Duration::from_millis(5)).unwrap();
        }
    });
    let bus = Client::connect(&path, ClientOptions::named("calld")).unwrap();
    let line = ModemLine::new(bus.clone(), 1, WAIT).unwrap();
    let audio = ModemAudio::new(bus.clone(), WAIT);
    let session = Session::new(
        vec![Box::new(line)],
        Box::new(audio),
        Box::new(ManualClock::default()),
    );
    let mut service = Service::new(bus, session, Duration::from_millis(5)).unwrap();
    let flag = stop.clone();
    let s = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            service.step().unwrap();
        }
    });
    let app = Client::connect(&path, ClientOptions::named("phone")).unwrap();
    wait(&app, |s| {
        s.lines.cellular == Cellular::Ready && s.lines.sims == vec![1]
    });

    Rig {
        _broker: broker,
        app,
        modem,
        stop,
        threads: vec![m, s],
    }
}

#[test]
fn mobile_calls_through_the_modem_service() {
    let Rig {
        _broker,
        app,
        modem,
        stop,
        threads,
    } = start();
    let index = modem.ring("0933 111 222").unwrap();
    let st = wait(&app, |s| s.calls.len() == 1);
    let id = st.calls[0].id;
    assert_eq!(st.calls[0].phase, Phase::Ringing);
    app.call(
        &Answer {
            id,
            others: Others::Hold,
        },
        WAIT,
    )
    .unwrap();
    eventually(|| {
        modem
            .state()
            .calls
            .iter()
            .any(|c| c.index == index && c.status == ModemCallStatus::Active)
    });
    app.call(&Hold { id, on: true }, WAIT).unwrap();
    eventually(|| {
        modem
            .state()
            .calls
            .iter()
            .any(|c| c.status == ModemCallStatus::Held)
    });
    app.call(&Hold { id, on: false }, WAIT).unwrap();
    app.call(&Dtmf { id, key: b'7' }, WAIT).unwrap();
    app.call(&Mute { on: true }, WAIT).unwrap();
    assert_eq!(modem.tones_and_mute(), (vec![b'7'], true));
    app.call(&Hangup { id }, WAIT).unwrap();
    eventually(|| modem.state().calls.is_empty());

    outgoing_answered_then_ended_by_the_network(&app, &modem);
    stop.store(true, Ordering::SeqCst);
    for t in threads {
        t.join().unwrap();
    }
}

fn outgoing_answered_then_ended_by_the_network(app: &Client, modem: &FakeModemControl) {
    let id = app
        .call(
            &Dial {
                way: SIM,
                number: "0944".into(),
                video: false,
            },
            WAIT,
        )
        .unwrap()
        .id;
    wait(app, |s| {
        s.calls
            .iter()
            .any(|c| c.id == id && c.phase == Phase::Alerting)
    });
    let index = modem.state().calls[0].index;
    modem.set_status(index, ModemCallStatus::Active);
    wait(app, |s| {
        s.calls
            .iter()
            .any(|c| c.id == id && c.phase == Phase::Active)
    });
    modem.end(index);
    let st = wait(app, |s| s.calls.is_empty());
    assert_eq!(st.ended.unwrap().phase, Phase::Ended(End::Remote));
}
