//! The call service over a real bus: requests, typed errors, retained state, hostile payloads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use ultar_bus::messages::call::{
    Answer, CallError, CallState, Cellular, Dial, Hangup, Hold, Log, Mute, Others, Phase, Snapshot,
    Ultar, Way,
};
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, Policy,
};
use ultar_call::fake::{FakeAudio, FakeControl, FakeLine, ManualClock};
use ultar_call::service::Service;
use ultar_call::session::Session;

const WAIT: Duration = Duration::from_secs(10);

struct Rig {
    _broker: Broker,
    app: Client,
    line: FakeControl,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            w.join().unwrap();
        }
    }
}

fn rig(tag: &str) -> Rig {
    let path = std::env::temp_dir().join(format!("ucall-{}-{tag}.sock", std::process::id()));
    let auth = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start()
        .unwrap();
    let (fake, line) = FakeLine::new(&[Way::Ultar, Way::Cellular(1)]);
    line.set_ultar(Ultar::Ready, Some("UOS-1111-2222"));
    line.set_cellular(Cellular::Ready, &[1]);
    let session = Session::new(
        vec![Box::new(fake)],
        Box::new(FakeAudio::new(&[
            ultar_bus::messages::call::Output::Earpiece,
        ])),
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
    let app = Client::connect(&path, ClientOptions::named("phone")).unwrap();
    Rig {
        _broker: broker,
        app,
        line,
        stop,
        worker: Some(worker),
    }
}

fn wait_state(app: &Client, f: impl Fn(&CallState) -> bool) -> CallState {
    let sub = app.subscribe::<CallState>().unwrap();
    loop {
        let st = sub.recv_timeout(WAIT).unwrap();
        if f(&st) {
            return st;
        }
    }
}

fn service_error(e: BusError) -> Option<CallError> {
    match e {
        BusError::Remote(ErrorCode::Service(c)) => CallError::from_code(c),
        _ => None,
    }
}

#[test]
fn incoming_answer_hold_hangup_over_the_bus() {
    let r = rig("flow");
    wait_state(&r.app, |s| s.lines.ultar == Ultar::Ready);
    r.line.ring(Way::Cellular(1), "0933", Some("سامر"));
    let st = wait_state(&r.app, |s| s.calls.len() == 1);
    let id = st.calls[0].id;
    assert_eq!(st.calls[0].phase, Phase::Ringing);
    r.app
        .call(
            &Answer {
                id,
                others: Others::Hold,
            },
            WAIT,
        )
        .unwrap();
    r.app.call(&Mute { on: true }, WAIT).unwrap();
    r.app.call(&Hold { id, on: true }, WAIT).unwrap();
    let st = r.app.call(&Snapshot, WAIT).unwrap();
    assert_eq!(st.calls[0].phase, Phase::Held);
    assert!(st.audio.muted);
    r.app.call(&Hangup { id }, WAIT).unwrap();
    let st = wait_state(&r.app, |s| s.calls.is_empty());
    assert!(st.ended.is_some());
    let log = r.app.call(&Log { missed_only: false }, WAIT).unwrap();
    assert_eq!(log.entries.len(), 1);
}

#[test]
fn typed_errors_and_hostile_payloads() {
    let r = rig("errors");
    wait_state(&r.app, |s| s.lines.cellular == Cellular::Ready);
    let err = r.app.call(&Hangup { id: 42 }, WAIT).unwrap_err();
    assert_eq!(service_error(err), Some(CallError::UnknownCall));
    let dial = Dial {
        way: Way::Ultar,
        number: "no number".into(),
        video: false,
    };
    let err = r.app.call(&dial, WAIT).unwrap_err();
    assert_eq!(service_error(err), Some(CallError::BadNumber));
    // Garbage and unknown methods are answered, the service keeps working.
    let garbage: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(37)).collect();
    let res: Result<(), BusError> = r.app.request::<Vec<u8>, ()>("call", "dial", &garbage, WAIT);
    assert!(res.is_err());
    let res: Result<(), BusError> = r.app.request::<(), ()>("call", "nope", &(), WAIT);
    assert!(matches!(
        res,
        Err(BusError::Remote(ErrorCode::UnknownMethod))
    ));
    let ok = Dial {
        way: Way::Cellular(1),
        number: "112".into(),
        video: false,
    };
    assert!(r.app.call(&ok, WAIT).is_ok());
}
