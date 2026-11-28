//! Two real `ultar-mesh` services as on two devices (real sockets, real encrypted
//! channel, real bus): the phone offers a call, the desk rings and answers, voice goes
//! both ways over `mesh.call_audio`, and the desk hangs up.

use std::net::TcpListener;
use std::time::Duration;
use ultar_bus::messages::launcher::{AppStatus, List, ListReply};
use ultar_bus::messages::mesh::{
    Done, MeshState, PairBegin, PairJoin, Right, SetEnabled, SetRights,
};
use ultar_bus::messages::mesh_calls::{
    CallAnswer, CallAudio, CallAudioIn, CallEnd, CallIncoming, CallOffer, CallPhase, CallProgress,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, TypedSubscription};
use ultar_mesh::{audio_link, device};
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
identity = "system.shell"
call = ["mesh.*"]
subscribe = ["mesh.*"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Device {
    _world: World,
    _daemon: Daemon,
    shell: Client,
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

fn device(tag: &str, name: &str, port: u16, peer: Option<u16>) -> Device {
    let identities = ["system.mesh", "system.launcher", "system.shell"];
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
    let daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-mesh"), "system.mesh", &args);
    let shell = world.client("system.shell");
    Device {
        _world: world,
        _daemon: daemon,
        shell,
    }
}

/// Switches both on and pairs them (the desk shows the code, the phone types it); returns
/// the ids as each side sees the other, and grants `Right::Call` both ways.
fn switch_on_and_pair(a: &Device, b: &Device) -> (String, String) {
    let state_a = a.shell.subscribe::<MeshState>().unwrap();
    let state_b = b.shell.subscribe::<MeshState>().unwrap();
    assert!(!state_b.recv_timeout(WAIT).unwrap().enabled);
    let on = SetEnabled { enabled: true };
    assert_eq!(call(&b.shell, &on).unwrap(), Done { ok: true });
    assert_eq!(call(&a.shell, &on).unwrap(), Done { ok: true });
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
    let grants = vec![Right::Handoff, Right::Files, Right::Clipboard, Right::Call];
    let grant_b = SetRights {
        device: phone.clone(),
        rights: grants.clone(),
    };
    let grant_a = SetRights {
        device: desk.clone(),
        rights: grants,
    };
    assert!(wait_until(WAIT, || {
        call(&b.shell, &grant_b).is_ok_and(|r| r == Done { ok: true })
            && call(&a.shell, &grant_a).is_ok_and(|r| r == Done { ok: true })
    }));
    (phone, desk)
}

/// The phone offers, the desk answers; returns the call id. Both sides saw `Accepted`.
fn offer_accepted(
    a: &Device,
    b: &Device,
    phone: &str,
    desk: &str,
    incoming_b: &TypedSubscription<CallIncoming>,
    progress_a: &TypedSubscription<CallProgress>,
) -> u64 {
    let offer = CallOffer {
        device: desk.to_owned(),
        caller: "+963 000".into(),
        video: false,
    };
    let offered = call(&a.shell, &offer).unwrap();
    assert_eq!(offered.device, desk);
    assert!(offered.id != 0);
    let incoming = incoming_b.recv_timeout(WAIT).unwrap();
    assert_eq!(incoming.from, phone);
    assert_eq!(incoming.caller, "+963 000");
    let answer = CallAnswer {
        id: incoming.id,
        accept: true,
    };
    assert_eq!(call(&b.shell, &answer).unwrap(), Done { ok: true });
    let mut accepted = false;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            accepted |= p.phase == CallPhase::Accepted;
        }
        accepted
    }));
    incoming.id
}

/// One 20 ms frame each way (`frame` is base64 of a full PCM frame).
fn voice(a: &Device, b: &Device, phone: &str, desk: &str, id: u64, frame: &str) {
    let audio_a = a.shell.subscribe::<CallAudio>().unwrap();
    let audio_b = b.shell.subscribe::<CallAudio>().unwrap();
    let out = CallAudioIn {
        device: desk.to_owned(),
        id,
        seq: 0,
        pcm: frame.to_owned(),
    };
    assert_eq!(call(&a.shell, &out).unwrap(), Done { ok: true });
    let got = audio_b.recv_timeout(WAIT).unwrap();
    assert_eq!((got.id, got.seq), (id, 0));
    let samples = audio_link::decode_frame(&device::decode(&got.pcm).unwrap()).unwrap();
    assert_eq!(samples[0], 7);
    let back = CallAudioIn {
        device: phone.to_owned(),
        id,
        seq: 1,
        pcm: frame.to_owned(),
    };
    assert_eq!(call(&b.shell, &back).unwrap(), Done { ok: true });
    let got = audio_a.recv_timeout(WAIT).unwrap();
    assert_eq!((got.id, got.seq), (id, 1));
}

/// The desk hangs up; the phone sees it ended.
fn hang_up(b: &Device, id: u64, progress_a: &TypedSubscription<CallProgress>) {
    assert_eq!(call(&b.shell, &CallEnd { id }).unwrap(), Done { ok: true });
    let mut ended = false;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            ended |= p.phase == CallPhase::Ended;
        }
        ended
    }));
}

#[test]
fn a_call_rings_answers_talks_and_hangs_up_over_two_services() {
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("calls-proc-b", "Desk", port_b, None);
    let a = device("calls-proc-a", "Phone", port_a, Some(port_b));
    let (phone, desk) = switch_on_and_pair(&a, &b);
    let incoming_b = b.shell.subscribe::<CallIncoming>().unwrap();
    let progress_a = a.shell.subscribe::<CallProgress>().unwrap();
    let id = offer_accepted(&a, &b, &phone, &desk, &incoming_b, &progress_a);
    let frame = device::encode(&audio_link::encode_frame(&vec![
        7i16;
        audio_link::FRAME_SAMPLES
    ]));
    voice(&a, &b, &phone, &desk, id, &frame);
    hang_up(&b, id, &progress_a);
}
