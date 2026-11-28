//! "Play there" with two real `ultar-mediad` processes, one per device, each on its own bus.
//! A stand-in for the mesh service on each side hands what its mediad sends to the other
//! bus (the real mesh transport is tested in `ultar-mesh`). The speaker of the playing
//! device is a file.

use std::path::Path;
use std::time::{Duration, Instant};
use ultar_bus::messages::media::{Button, Command, Update};
use ultar_bus::messages::mesh::{Capability, DeviceKind, Done, MeshState, Peer, Right};
use ultar_bus::messages::mesh_media::{
    MediaControl, MediaCtlOut, MediaIncoming, MediaOpen, MediaOpened, MediaPcmIn, MediaPcmOut,
    SetThere, TherePause, TherePush, ThereReply, ThereState, ThereVolume,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, TypedSubscription};
use ultar_mediad::there::{encode, FRAME};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.mediad"
register = ["media"]
publish = ["media.state", "media.command", "media.there"]
subscribe = ["mesh.state", "mesh.media_incoming", "mesh.media_pcm_in", "mesh.media_control", "mesh.media_progress"]
call = ["mesh.media_open", "mesh.media_pcm", "mesh.media_ctl"]

[[rule]]
identity = "system.mesh"
register = ["mesh"]
publish = ["mesh.state", "mesh.media_incoming", "mesh.media_pcm_in", "mesh.media_control", "mesh.media_progress"]

[[rule]]
identity = "system.shell"
call = ["media.set_output", "media.there_volume", "media.there_pause", "media.system"]
subscribe = ["media.there"]

[[rule]]
identity = "app.*"
call = ["media.update", "media.there_push", "media.set_output"]
subscribe = ["media.command", "media.there"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn start(world: &World, sink: Option<&Path>) -> Daemon {
    let policy = world.policy_file();
    let mut args = vec!["--policy", policy.to_str().unwrap()];
    if let Some(sink) = sink {
        args.extend(["--there-sink", sink.to_str().unwrap()]);
    }
    world.spawn(env!("CARGO_BIN_EXE_ultar-mediad"), "system.mediad", &args)
}

/// Waits for a published state that satisfies `ok`.
fn state_where(sub: &TypedSubscription<ThereState>, ok: impl Fn(&ThereState) -> bool) -> ThereState {
    let start = Instant::now();
    loop {
        let left = WAIT.saturating_sub(start.elapsed());
        match sub.recv_timeout(left) {
            Ok(s) if ok(&s) => return s,
            Ok(_) => {}
            Err(e) => panic!("no such state: {e:?}"),
        }
    }
}

fn mark(m: i16) -> String {
    encode(&vec![m; FRAME])
}

/// What the file speaker was given, as the left channel, one entry per 20 ms frame.
fn played(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let left: Vec<i16> = bytes
        .chunks_exact(4)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    left.chunks_exact(FRAME)
        .filter(|f| f.iter().any(|s| *s != 0))
        .map(|f| f[0])
        .collect()
}

fn desk() -> MeshState {
    MeshState {
        enabled: true,
        id: "phone".into(),
        name: "Phone".into(),
        peers: vec![Peer {
            id: "desk".into(),
            name: "Desk".into(),
            kind: DeviceKind::Desktop,
            capabilities: vec![Capability::AudioOut],
            paired: true,
            reachable: true,
            rights: vec![Right::Call],
        }],
    }
}

#[test]
fn the_sound_goes_to_the_desk_and_both_sides_steer_it() {
    let src = World::new(
        "there-src",
        POLICY,
        &["system.mediad", "system.mesh", "system.shell", "app.music"],
    );
    let snk = World::new("there-snk", POLICY, &["system.mediad", "system.mesh", "system.shell"]);
    let speaker = snk.subdir("card").join("pcm");
    std::fs::write(&speaker, []).unwrap();
    let mut desk_daemon = start(&snk, Some(&speaker));
    let mut phone_daemon = start(&src, None);

    // The stand-in meshes: each hands what its mediad sends to the other bus.
    let into_desk = snk.client("system.mesh");
    let into_phone = src.client("system.mesh");
    let (to_desk, to_phone) = (into_desk.clone(), into_phone.clone());
    src.serve("system.mesh", "mesh", move |r| match r.method() {
        "media_open" => {
            let _ = r.respond::<MediaOpen>(|c| {
                let _ = to_desk.publish(&MediaIncoming {
                    id: 7,
                    from: "phone".into(),
                    from_name: "Phone".into(),
                    title: c.title,
                    artist: c.artist,
                });
                Ok(MediaOpened { id: 7 })
            });
        }
        "media_pcm" => {
            let _ = r.respond::<MediaPcmOut>(|c| {
                let _ = to_desk.publish(&MediaPcmIn {
                    id: c.id,
                    from: "phone".into(),
                    seq: c.seq,
                    pcm: c.pcm,
                });
                Ok(Done { ok: true })
            });
        }
        "media_ctl" => {
            let _ = r.respond::<MediaCtlOut>(|c| {
                let _ = to_desk.publish(&MediaControl {
                    id: c.id,
                    from: "phone".into(),
                    ctl: c.ctl,
                });
                Ok(Done { ok: true })
            });
        }
        _ => {}
    });
    snk.serve("system.mesh", "mesh", move |r| {
        let _ = r.respond::<MediaCtlOut>(|c| {
            let _ = to_phone.publish(&MediaControl {
                id: c.id,
                from: "desk".into(),
                ctl: c.ctl,
            });
            Ok(Done { ok: true })
        });
    });

    let shell = src.client("system.shell");
    let music = src.client("app.music");
    let desk_shell = snk.client("system.shell");
    let phone_state = shell.subscribe::<ThereState>().unwrap();
    let desk_state = desk_shell.subscribe::<ThereState>().unwrap();
    let commands = music.subscribe::<Command>().unwrap();
    let up = |playing| Update {
        title: "Song".into(),
        artist: "Artist".into(),
        playing,
        can_next: false,
        can_previous: false,
    };
    call(&music, &up(true)).unwrap();

    // The desk is offered once the mesh shows it paired, reachable and with a speaker.
    wait_until(WAIT, || {
        into_phone.publish(&desk()).is_ok()
    });
    let offered = state_where(&phone_state, |s| s.outputs.len() == 1);
    assert_eq!(offered.outputs[0].device, "desk");
    // An app cannot send the sound anywhere; an unknown device is refused.
    let denied = music.call(
        &SetThere {
            device: Some("desk".into()),
        },
        WAIT,
    );
    assert!(matches!(
        denied,
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let none = SetThere {
        device: Some("nobody".into()),
    };
    assert_eq!(call(&shell, &none).unwrap(), ThereReply { ok: false });

    let there = SetThere {
        device: Some("desk".into()),
    };
    assert_eq!(call(&shell, &there).unwrap(), ThereReply { ok: true });
    state_where(&phone_state, |s| s.active.as_deref() == Some("desk"));
    let on_desk = state_where(&desk_state, |s| s.playing_for.is_some());
    assert_eq!(on_desk.playing_for.as_deref(), Some("Phone"));

    // Sound in order; a wrong size and a repeated counter are refused.
    let push = |seq, pcm: String| call(&music, &TherePush { seq, pcm }).unwrap().ok;
    for (seq, m) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
        assert!(push(seq, mark(m)));
    }
    assert!(!push(4, encode(&[1, 2, 3])), "a frame has exactly 20 ms");
    assert!(!push(3, mark(9)), "a counter only goes up");
    assert!(wait_until(WAIT, || played(&speaker) == [1, 2, 3, 4]));

    // Volume from the phone reaches the desk's speaker; from the desk, the phone's state.
    assert!(call(&shell, &ThereVolume { percent: 25 }).unwrap().ok);
    state_where(&phone_state, |s| s.volume == 25);
    assert!(push(4, mark(100)));
    assert!(push(5, mark(100)));
    assert!(push(6, mark(100)));
    assert!(wait_until(WAIT, || played(&speaker).ends_with(&[25, 25, 25])));
    assert!(call(&desk_shell, &ThereVolume { percent: 60 }).unwrap().ok);
    state_where(&phone_state, |s| s.volume == 60);

    // Pause from the phone: its player is told, nothing is accepted, the desk is silent.
    assert!(call(&shell, &TherePause { on: true }).unwrap().ok);
    let told = commands.recv_timeout(WAIT).unwrap();
    assert_eq!((told.player.as_str(), told.button), ("app.music", Button::PlayPause));
    state_where(&phone_state, |s| s.paused);
    assert!(!push(7, mark(50)), "paused: no sound is sent");
    call(&music, &up(false)).unwrap();
    // Resume from the desk: the phone follows and its player is told again.
    assert!(call(&desk_shell, &TherePause { on: false }).unwrap().ok);
    state_where(&phone_state, |s| !s.paused);
    let told = commands.recv_timeout(WAIT).unwrap();
    assert_eq!(told.button, Button::PlayPause);
    call(&music, &up(true)).unwrap();
    assert!(push(8, mark(100)));
    assert!(wait_until(WAIT, || played(&speaker).ends_with(&[60])));

    // Back to this device: the desk stops playing.
    assert!(call(&shell, &SetThere { device: None }).unwrap().ok);
    state_where(&phone_state, |s| s.active.is_none());
    state_where(&desk_state, |s| s.playing_for.is_none());
    assert!(!push(9, mark(1)), "no session: nothing is sent");
    assert_eq!(phone_daemon.terminate(), Some(0));
    assert_eq!(desk_daemon.terminate(), Some(0));
}

#[test]
fn a_device_without_a_speaker_turns_a_session_away() {
    let src = World::new("there-nospk", POLICY, &["system.mediad", "system.mesh", "system.shell"]);
    let mut daemon = start(&src, None);
    let stops = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = stops.clone();
    src.serve("system.mesh", "mesh", move |r| {
        let record = record.clone();
        let _ = r.respond::<MediaCtlOut>(|c| {
            record.lock().unwrap().push((c.device, c.id, c.ctl));
            Ok(Done { ok: true })
        });
    });
    let mesh = src.client("system.mesh");
    let shell = src.client("system.shell");
    let state = shell.subscribe::<ThereState>().unwrap();
    state_where(&state, |_| true);
    wait_until(WAIT, || {
        mesh.publish(&MediaIncoming {
            id: 3,
            from: "phone".into(),
            from_name: "Phone".into(),
            title: "x".into(),
            artist: "y".into(),
        })
        .is_ok()
            && !stops.lock().unwrap().is_empty()
    });
    let got = stops.lock().unwrap()[0].clone();
    assert_eq!(
        got,
        ("phone".to_owned(), 3, ultar_bus::messages::mesh_media::MediaCtl::Stop)
    );
    assert_eq!(daemon.terminate(), Some(0));
}
