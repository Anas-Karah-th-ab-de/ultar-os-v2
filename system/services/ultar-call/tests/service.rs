//! The call service over a real bus: requests, typed errors, retained state, hostile payloads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use ultar_bus::messages::call::{
    Answer, CallError, CallState, Cellular, Dial, Emergency, Hangup, Hold, Log, Mute, Others,
    Phase, Snapshot, Ultar, Way,
};
use ultar_bus::messages::settings::{Changed, Get, GetReply, SettingValue};
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
    path: std::path::PathBuf,
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
    rig_with(tag, None)
}

/// The rig; `mono`: a file standing in for the kernel's mono switch.
fn rig_with(tag: &str, mono: Option<std::path::PathBuf>) -> Rig {
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
    if let Some(node) = mono {
        service = service.with_mono_node(node);
    }
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
        path,
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

/// A settings stand-in on the rig's bus: answers `settings.get` with `value`.
fn settings_stand_in(path: &std::path::Path, value: Arc<std::sync::Mutex<SettingValue>>) -> Client {
    let settings = Client::connect(path, ClientOptions::named("settingsd")).unwrap();
    let endpoint = settings.register("settings").unwrap();
    thread::spawn(move || loop {
        match endpoint.recv_timeout(Duration::from_millis(20)) {
            Ok(req) => {
                let _ = req.respond::<Get>(|_| {
                    Ok(GetReply {
                        value: Some(value.lock().unwrap().clone()),
                    })
                });
            }
            Err(BusError::Timeout) => {}
            Err(_) => return,
        }
    });
    settings
}

/// Places an emergency call and reads the number it dialed.
fn emergency_number(app: &Client) -> String {
    let reply = app.call(&Emergency {}, WAIT).unwrap();
    let st = app.call(&Snapshot, WAIT).unwrap();
    let number = st
        .calls
        .iter()
        .find(|c| c.id == reply.id)
        .unwrap()
        .number
        .clone();
    app.call(&Hangup { id: reply.id }, WAIT).unwrap();
    number
}

/// Waits until an emergency call dials `want`.
fn until_number(app: &Client, want: &str) {
    let start = std::time::Instant::now();
    loop {
        let n = emergency_number(app);
        if n == want {
            return;
        }
        assert!(start.elapsed() < WAIT, "emergency number stuck at {n}");
        thread::sleep(Duration::from_millis(50));
    }
}

/// call.emergency: the service dials the number it read from the settings itself
/// (the request carries none), follows changes at runtime, falls back to 112 for an
/// invalid value, and fails when the mobile way is not available.
#[test]
fn emergency_calls_the_configured_number_and_follows_changes() {
    let r = rig("emergency");
    wait_state(&r.app, |s| s.lines.cellular == Cellular::Ready);
    let value = Arc::new(std::sync::Mutex::new(SettingValue::Text("999".into())));
    let settings = settings_stand_in(&r.path, value.clone());
    let tell = |text: &str| {
        *value.lock().unwrap() = SettingValue::Text(text.into());
        settings
            .publish(&Changed {
                key: "emergency.number".into(),
                value: SettingValue::Text(text.into()),
            })
            .unwrap();
    };
    // The start read brings 999 (until then the default 112).
    until_number(&r.app, "999");
    // A runtime change is followed; an invalid value falls back to 112.
    tell("110");
    until_number(&r.app, "110");
    tell("not-a-number");
    until_number(&r.app, "112");
    // Without the mobile way the call fails (the lock screen shows the reason).
    r.line.set_cellular(Cellular::NoModem, &[]);
    wait_state(&r.app, |s| s.lines.cellular == Cellular::NoModem);
    let err = r.app.call(&Emergency {}, WAIT).unwrap_err();
    assert!(matches!(
        service_error(err),
        Some(CallError::Unavailable(_))
    ));
}

/// accessibility.mono_audio: read at start, followed at runtime, written to the kernel's
/// switch (`1` on, `0` off); a value of the wrong type changes nothing.
#[test]
fn mono_audio_reaches_the_kernels_switch() {
    let node = std::env::temp_dir().join(format!("ucall-mono-{}", std::process::id()));
    std::fs::write(&node, b"").unwrap();
    let r = rig_with("mono", Some(node.clone()));
    let value = Arc::new(std::sync::Mutex::new(SettingValue::Flag(true)));
    let settings = settings_stand_in(&r.path, value.clone());
    let switch = |want: &[u8]| {
        let start = std::time::Instant::now();
        while std::fs::read(&node).unwrap() != want {
            assert!(
                start.elapsed() < WAIT,
                "switch stuck at {:?}",
                std::fs::read(&node)
            );
            thread::sleep(Duration::from_millis(20));
        }
    };
    let tell = |v: SettingValue| {
        settings
            .publish(&Changed {
                key: "accessibility.mono_audio".into(),
                value: v,
            })
            .unwrap();
    };
    // The start read brings "on".
    switch(b"1");
    tell(SettingValue::Flag(false));
    switch(b"0");
    tell(SettingValue::Text("on".into()));
    tell(SettingValue::Flag(true));
    switch(b"1");
    drop(r);
    std::fs::remove_file(&node).unwrap();
}
