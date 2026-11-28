//! "Play there" with two real `ultar-mediad` processes, one per device, each on its own bus.
//! A stand-in for the mesh service on each side hands what its mediad sends to the other
//! bus (the real mesh transport is tested in `ultar-mesh`). The speaker of the playing
//! device is a file.

use std::path::{Path, PathBuf};
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
fn state_where(
    sub: &TypedSubscription<ThereState>,
    ok: impl Fn(&ThereState) -> bool,
) -> ThereState {
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

/// A stand-in for the sound server: it takes every media stream of the daemon (hello, reply)
/// and writes the PCM it receives to `out`.
fn sound_server(socket: &Path, out: &Path) {
    use std::io::{Read, Write};
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    let out = out.to_owned();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { return };
            let mut hello = [0u8; ultar_audio_stream::HELLO_LEN];
            if s.read_exact(&mut hello).is_err() {
                continue;
            }
            let ok = ultar_audio_stream::Hello::decode(&hello).is_some_and(|h| {
                h.class == ultar_audio_stream::Class::Media && h.rate == 48_000 && h.channels == 1
            });
            let status = if ok {
                ultar_audio_stream::Status::Ok
            } else {
                ultar_audio_stream::Status::BadHello
            };
            let _ = s.write_all(&status.encode());
            let out = out.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        return;
                    }
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&out) {
                        let _ = f.write_all(&buf[..n]);
                    }
                }
            });
        }
    });
}

/// What the speaker was given (mono), one entry per 20 ms frame.
fn played(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let left: Vec<i16> = bytes
        .chunks_exact(2)
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

/// The two devices: a phone that plays and a desk with a (file) speaker.
struct Rig {
    _src: World,
    _snk: World,
    shell: Client,
    music: Client,
    desk_shell: Client,
    into_phone: Client,
    phone_state: TypedSubscription<ThereState>,
    desk_state: TypedSubscription<ThereState>,
    commands: TypedSubscription<Command>,
    speaker: PathBuf,
    phone_daemon: Daemon,
    desk_daemon: Daemon,
}

impl Rig {
    /// The music app hands over one 20 ms frame.
    fn push(&self, seq: u64, pcm: String) -> bool {
        call(&self.music, &TherePush { seq, pcm }).unwrap().ok
    }
}

fn up(playing: bool) -> Update {
    Update {
        title: "Song".into(),
        artist: "Artist".into(),
        playing,
        can_next: false,
        can_previous: false,
    }
}

/// The stand-in meshes: each hands what its mediad sends to the other bus. Returns the
/// phone's mesh connection (the mesh's reports).
fn stand_in_meshes(src: &World, snk: &World) -> Client {
    let into_desk = snk.client("system.mesh");
    let into_phone = src.client("system.mesh");
    let (to_desk, to_phone) = (into_desk, into_phone.clone());
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
    into_phone
}

fn rig() -> Rig {
    let src = World::new(
        "there-src",
        POLICY,
        &["system.mediad", "system.mesh", "system.shell", "app.music"],
    );
    let snk = World::new(
        "there-snk",
        POLICY,
        &["system.mediad", "system.mesh", "system.shell"],
    );
    let speaker = snk.subdir("card").join("pcm");
    std::fs::write(&speaker, []).unwrap();
    let socket = snk.subdir("card").join("mediad.sock");
    sound_server(&socket, &speaker);
    let desk_daemon = start(&snk, Some(&socket));
    let phone_daemon = start(&src, None);
    let into_phone = stand_in_meshes(&src, &snk);
    let shell = src.client("system.shell");
    let music = src.client("app.music");
    let desk_shell = snk.client("system.shell");
    let phone_state = shell.subscribe::<ThereState>().unwrap();
    let desk_state = desk_shell.subscribe::<ThereState>().unwrap();
    let commands = music.subscribe::<Command>().unwrap();
    call(&music, &up(true)).unwrap();
    Rig {
        _src: src,
        _snk: snk,
        shell,
        music,
        desk_shell,
        into_phone,
        phone_state,
        desk_state,
        commands,
        speaker,
        phone_daemon,
        desk_daemon,
    }
}

/// The desk is offered once the mesh shows it paired, reachable and with a speaker; an app
/// cannot send the sound there, an unknown device is refused; then the sound goes there.
fn the_desk_is_offered_and_chosen(r: &Rig) {
    wait_until(WAIT, || r.into_phone.publish(&desk()).is_ok());
    let offered = state_where(&r.phone_state, |s| s.outputs.len() == 1);
    assert_eq!(offered.outputs[0].device, "desk");
    let denied = r.music.call(
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
    assert_eq!(call(&r.shell, &none).unwrap(), ThereReply { ok: false });
    let there = SetThere {
        device: Some("desk".into()),
    };
    assert_eq!(call(&r.shell, &there).unwrap(), ThereReply { ok: true });
    state_where(&r.phone_state, |s| s.active.as_deref() == Some("desk"));
    let on_desk = state_where(&r.desk_state, |s| s.playing_for.is_some());
    assert_eq!(on_desk.playing_for.as_deref(), Some("Phone"));
}

/// Sound in order; a wrong size and a repeated counter are refused. Volume from the phone
/// reaches the desk's speaker; from the desk, the phone's state.
fn sound_and_volume(r: &Rig) {
    for (seq, m) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
        assert!(r.push(seq, mark(m)));
    }
    assert!(!r.push(4, encode(&[1, 2, 3])), "a frame has exactly 20 ms");
    assert!(!r.push(3, mark(9)), "a counter only goes up");
    assert!(wait_until(WAIT, || played(&r.speaker) == [1, 2, 3, 4]));
    assert!(call(&r.shell, &ThereVolume { percent: 25 }).unwrap().ok);
    state_where(&r.phone_state, |s| s.volume == 25);
    for seq in 4..7 {
        assert!(r.push(seq, mark(100)));
    }
    assert!(wait_until(WAIT, || played(&r.speaker).ends_with(&[25, 25, 25])));
    assert!(
        call(&r.desk_shell, &ThereVolume { percent: 60 })
            .unwrap()
            .ok
    );
    state_where(&r.phone_state, |s| s.volume == 60);
}

/// Pause from the phone: its player is told, nothing is accepted, the desk is silent.
/// Resume from the desk: the phone follows and its player is told again.
fn pause_and_resume(r: &Rig) {
    assert!(call(&r.shell, &TherePause { on: true }).unwrap().ok);
    let told = r.commands.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (told.player.as_str(), told.button),
        ("app.music", Button::PlayPause)
    );
    state_where(&r.phone_state, |s| s.paused);
    assert!(!r.push(7, mark(50)), "paused: no sound is sent");
    call(&r.music, &up(false)).unwrap();
    assert!(call(&r.desk_shell, &TherePause { on: false }).unwrap().ok);
    state_where(&r.phone_state, |s| !s.paused);
    let told = r.commands.recv_timeout(WAIT).unwrap();
    assert_eq!(told.button, Button::PlayPause);
    call(&r.music, &up(true)).unwrap();
    // A session starts playing once 60 ms are buffered.
    for seq in 8..11 {
        assert!(r.push(seq, mark(100)));
    }
    assert!(wait_until(WAIT, || played(&r.speaker).ends_with(&[60])));
}

#[test]
fn the_sound_goes_to_the_desk_and_both_sides_steer_it() {
    let mut r = rig();
    the_desk_is_offered_and_chosen(&r);
    sound_and_volume(&r);
    pause_and_resume(&r);

    // Back to this device: the desk stops playing.
    assert!(call(&r.shell, &SetThere { device: None }).unwrap().ok);
    state_where(&r.phone_state, |s| s.active.is_none());
    state_where(&r.desk_state, |s| s.playing_for.is_none());
    assert!(!r.push(9, mark(1)), "no session: nothing is sent");
    assert_eq!(r.phone_daemon.terminate(), Some(0));
    assert_eq!(r.desk_daemon.terminate(), Some(0));
}

#[test]
fn a_device_without_a_speaker_turns_a_session_away() {
    let src = World::new(
        "there-nospk",
        POLICY,
        &["system.mediad", "system.mesh", "system.shell"],
    );
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
        (
            "phone".to_owned(),
            3,
            ultar_bus::messages::mesh_media::MediaCtl::Stop
        )
    );
    assert_eq!(daemon.terminate(), Some(0));
}
