//! A call between two devices from end to end: two real `ultar-mesh` services (real sockets,
//! real encrypted channel), and on each device the real call service with a fake line and a
//! fake sound device. The phone rings, the desk answers, the voice flows both ways in order,
//! and the call is hung up from each side.

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use ultar_bus::messages::call::{Hangup, Phase, Snapshot, Ultar, Way};
use ultar_bus::messages::launcher::{AppStatus, List, ListReply};
use ultar_bus::messages::mesh::{
    Done, MeshState, PairBegin, PairJoin, Right, SetEnabled, SetRights,
};
use ultar_bus::messages::mesh_calls::{CallAnswer, CallEnd, CallIncoming, CallPhase, CallProgress};
use ultar_bus::{BusError, Call, Client, ErrorCode, TypedSubscription};
use ultar_call::fake::{Command, FakeAudio, FakeControl, FakeLine, ManualClock};
use ultar_call::mesh_relay::{MeshRelay, Taps};
use ultar_call::mesh_tap::{FakeTap, FakeTapControl, FRAME};
use ultar_call::service::Service;
use ultar_call::session::Session;
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.mesh"
register = ["mesh"]
publish = ["mesh.state", "mesh.call_incoming", "mesh.call_progress", "mesh.call_audio", "mesh.note"]
call = ["launcher.list"]
subscribe = ["llm.state"]

[[rule]]
identity = "system.launcher"
register = ["launcher"]

[[rule]]
identity = "system.calld"
register = ["call"]
publish = ["call.state", "call.picture", "call.mesh"]
subscribe = ["mesh.state", "mesh.call_incoming", "mesh.call_progress", "mesh.call_audio", "settings.changed"]
call = ["mesh.call_offer", "mesh.call_end", "mesh.call_audio"]

[[rule]]
identity = "system.shell"
call = ["mesh.*", "call.*"]
subscribe = ["mesh.*", "call.mesh"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// The mesh service's program, built if needed (a test of another package has no
/// `CARGO_BIN_EXE_` for it).
fn mesh_binary() -> String {
    static BUILT: std::sync::Once = std::sync::Once::new();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    BUILT.call_once(|| {
        let status = std::process::Command::new(&cargo)
            .args(["build", "-q", "-p", "ultar-mesh"])
            .status()
            .expect("cargo runs");
        assert!(status.success(), "ultar-mesh builds");
    });
    let exe = std::env::current_exe().unwrap();
    let debug = exe.parent().and_then(|deps| deps.parent()).unwrap();
    debug.join("ultar-mesh").to_str().unwrap().to_owned()
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// One device: the mesh service, the call service, and what a test needs of them.
struct Device {
    _world: World,
    _daemon: Daemon,
    shell: Client,
    line: FakeControl,
    tap: FakeTapControl,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn launcher(world: &World) {
    world.serve("system.launcher", "launcher", move |request| {
        if request.method() == List::METHOD {
            let apps = ["editor"]
                .iter()
                .map(|app| AppStatus {
                    app: (*app).to_owned(),
                    uid: 0,
                    profile: String::new(),
                    version: String::new(),
                    instances: Vec::new(),
                })
                .collect();
            let _ = request.reply(&ListReply { apps });
        }
    });
}

/// The call service of a device (`host`: it keeps the line and offers calls; otherwise it
/// answers calls of another device on its sound device).
fn calld(
    world: &World,
    host: bool,
) -> (FakeControl, FakeTapControl, Arc<AtomicBool>, JoinHandle<()>) {
    let (fake, line) = FakeLine::new(&[Way::Ultar]);
    line.set_ultar(Ultar::Ready, Some("UOS-1111-2222"));
    let session = Session::new(
        vec![Box::new(fake)],
        Box::new(FakeAudio::new(&[
            ultar_bus::messages::call::Output::Earpiece,
        ])),
        Box::new(ManualClock::default()),
    );
    let (tap, control) = FakeTap::new();
    let taps = if host {
        Taps {
            ultar: Some(Box::new(tap)),
            ..Taps::default()
        }
    } else {
        Taps {
            guest: Some(Box::new(tap)),
            ..Taps::default()
        }
    };
    let relay = MeshRelay::new(world.client("system.calld"), taps);
    let bus = world.client("system.calld");
    let mut service = Service::new(bus, session, Duration::from_millis(5))
        .unwrap()
        .with_mesh_relay(relay);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            service.step().unwrap();
        }
    });
    (line, control, stop, worker)
}

fn device(tag: &str, name: &str, port: u16, peer: Option<u16>, host: bool) -> Device {
    let identities = [
        "system.mesh",
        "system.launcher",
        "system.shell",
        "system.calld",
    ];
    let world = World::new(tag, POLICY, &identities);
    launcher(&world);
    let state = world.subdir("state");
    let inbox = world.subdir("inbox");
    let home = world.subdir("home");
    let port = port.to_string();
    let peer = peer.map(|p| format!("127.0.0.1:{p}"));
    let mut args = vec!["--state", state.to_str().unwrap()];
    args.extend(["--share", home.to_str().unwrap()]);
    args.extend(["--inbox", inbox.to_str().unwrap(), "--name", name]);
    args.extend(["--kind", "desktop", "--port", &port, "--announce", "off"]);
    if let Some(peer) = &peer {
        args.extend(["--peer", peer]);
    }
    let daemon = world.spawn(&mesh_binary(), "system.mesh", &args);
    let shell = world.client("system.shell");
    let (line, tap, stop, worker) = calld(&world, host);
    Device {
        _world: world,
        _daemon: daemon,
        shell,
        line,
        tap,
        stop,
        worker: Some(worker),
    }
}

/// Switches both on, pairs them and grants "Anrufe und Ton" both ways.
fn switch_on_and_pair(a: &Device, b: &Device) {
    let state_a = a.shell.subscribe::<MeshState>().unwrap();
    let state_b = b.shell.subscribe::<MeshState>().unwrap();
    assert!(!state_b.recv_timeout(WAIT).unwrap().enabled);
    let on = SetEnabled { enabled: true };
    for device in [b, a] {
        assert!(wait_until(WAIT, || call(&device.shell, &on)
            .is_ok_and(|r| r == Done { ok: true })));
    }
    let (mut phone, mut desk) = (None, None);
    assert!(wait_until(WAIT, || {
        while let Ok(Some(s)) = state_a.try_recv() {
            desk = s
                .peers
                .iter()
                .find(|p| p.name == "Desk")
                .map(|p| p.id.clone());
        }
        while let Ok(Some(s)) = state_b.try_recv() {
            phone = s
                .peers
                .iter()
                .find(|p| p.name == "Phone")
                .map(|p| p.id.clone());
        }
        phone.is_some() && desk.is_some()
    }));
    let (phone, desk) = (phone.unwrap(), desk.unwrap());
    let code = call(&b.shell, &PairBegin).unwrap().code;
    let join = PairJoin {
        device: desk.clone(),
        code,
    };
    assert_eq!(call(&a.shell, &join).unwrap(), Done { ok: true });
    let rights = vec![Right::Handoff, Right::Files, Right::Clipboard, Right::Call];
    let grant_b = SetRights {
        device: phone,
        rights: rights.clone(),
    };
    let grant_a = SetRights {
        device: desk,
        rights,
    };
    assert!(wait_until(WAIT, || {
        call(&b.shell, &grant_b).is_ok_and(|r| r == Done { ok: true })
            && call(&a.shell, &grant_a).is_ok_and(|r| r == Done { ok: true })
    }));
}

/// Marks that no halving of another mark can be mistaken for (a concealed frame is half).
const MARKS: [i16; 6] = [1001, 3007, 9011, 20009, 25013, 31019];

/// The marks `control` heard, in the order heard.
fn heard(control: &FakeTapControl) -> Vec<i16> {
    control
        .heard()
        .iter()
        .map(|f| f[0])
        .filter(|s| MARKS.contains(s))
        .collect()
}

/// Rings the phone and lets the desk answer; returns the phone's call id and the offer.
fn ring_and_answer(
    a: &Device,
    b: &Device,
    incoming: &TypedSubscription<CallIncoming>,
) -> (u32, u64) {
    a.line.ring(Way::Ultar, "UOS-5555-6666", Some("Samer"));
    let seen = incoming.recv_timeout(WAIT).unwrap();
    assert_eq!(seen.caller, "Samer", "only what the phone shows");
    let answer = CallAnswer {
        id: seen.id,
        accept: true,
    };
    assert_eq!(call(&b.shell, &answer).unwrap(), Done { ok: true });
    let mut id = 0;
    assert!(wait_until(WAIT, || {
        let state = a.shell.call(&Snapshot, WAIT).unwrap();
        match state.calls.first() {
            Some(c) if c.phase == Phase::Active => {
                id = c.id;
                true
            }
            _ => false,
        }
    }));
    (id, seen.id)
}

/// Voice both ways, in order and without loss.
fn voice_both_ways(a: &Device, b: &Device) {
    for mark in MARKS {
        a.tap.speak(vec![mark; FRAME]);
        b.tap.speak(vec![mark; FRAME]);
    }
    assert!(wait_until(WAIT, || heard(&b.tap).len() >= MARKS.len()
        && heard(&a.tap).len() >= MARKS.len()));
    assert_eq!(heard(&b.tap), MARKS, "phone to desk");
    assert_eq!(heard(&a.tap), MARKS, "desk to phone");
}

/// A second call; this time the phone hangs up and the desk is told.
fn the_phone_hangs_up(a: &Device, b: &Device, incoming: &TypedSubscription<CallIncoming>) {
    let progress_b = b.shell.subscribe::<CallProgress>().unwrap();
    let (call_id, _) = ring_and_answer(a, b, incoming);
    assert!(call(&a.shell, &Hangup { id: call_id }).is_ok());
    let mut told = false;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_b.try_recv() {
            told |= p.phase == CallPhase::Ended;
        }
        told && b.tap.calls() == (2, 2) && a.tap.calls() == (2, 2)
    }));
}

/// The desk hangs up: the phone's call ends and both sound devices are let go.
fn the_desk_hangs_up(a: &Device, b: &Device, offer: u64) {
    let progress_a = a.shell.subscribe::<CallProgress>().unwrap();
    assert_eq!(
        call(&b.shell, &CallEnd { id: offer }).unwrap(),
        Done { ok: true }
    );
    assert!(wait_until(WAIT, || a
        .shell
        .call(&Snapshot, WAIT)
        .unwrap()
        .calls
        .is_empty()));
    assert!(a
        .line
        .commands()
        .iter()
        .any(|c| matches!(c, Command::Release(_))));
    let mut ended = false;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            ended |= p.phase == CallPhase::Ended;
        }
        ended && a.tap.calls() == (1, 1) && b.tap.calls() == (1, 1)
    }));
}

#[test]
fn a_call_rings_is_answered_on_the_desk_talks_both_ways_and_hangs_up_from_each_side() {
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("calls-e2e-b", "Desk", port_b, None, false);
    let a = device("calls-e2e-a", "Phone", port_a, Some(port_b), true);
    switch_on_and_pair(&a, &b);
    let incoming = b.shell.subscribe::<CallIncoming>().unwrap();

    // Ring -> answer on the desk: the phone's call is answered on its line.
    let (_, offer) = ring_and_answer(&a, &b, &incoming);
    assert!(a
        .line
        .commands()
        .iter()
        .any(|c| matches!(c, Command::Answer(_))));
    assert_eq!(a.tap.calls().0, 1, "the phone's voice is bridged");
    assert_eq!(b.tap.calls().0, 1, "the desk's sound device is taken");

    voice_both_ways(&a, &b);
    the_desk_hangs_up(&a, &b, offer);
    the_phone_hangs_up(&a, &b, &incoming);
}
