//! The real `ultar-audiod` as the sound server on a fake card (two files): a call, the media
//! player and voice control at the same time, each with the right the bus policy gives it.
//! Real sockets, real bus, real process; no real sound card (that cannot be tested here).

use std::f64::consts::PI;
use std::path::Path;
use std::time::Duration;
use ultar_audio_stream::{Class, ConnectError, Direction, Hello, Status, Stream};
use ultar_bus::messages::audio::MicState;
use ultar_bus::messages::voice::{AudioDone, Capture, MicChunk};
use ultar_bus::{BusError, Client, ErrorCode, TypedSubscription};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.audiod"
register = ["audio"]
publish = ["audio.state", "audio.mic", "audio.mic_state"]
subscribe = ["settings.changed"]
call = ["settings.get"]

[[rule]]
identity = "system.calld"
call = ["audio.call", "audio.capture"]

[[rule]]
identity = "system.mediad"
call = ["audio.play"]

[[rule]]
identity = "system.voiced"
call = ["audio.capture", "audio.play"]
subscribe = ["audio.mic"]

[[rule]]
identity = "app.nobody"
call = []

[[rule]]
identity = "system.shell"
subscribe = ["audio.mic_state"]
"#;

const IDENTITIES: [&str; 7] = [
    "system.audiod",
    "system.calld",
    "system.mediad",
    "system.voiced",
    "app.nobody",
    "system.shell",
    "system.stand_in",
];

/// Magnitude of the tone `freq` in `samples` (48 kHz): its amplitude when it is a pure sine.
fn amplitude(samples: &[f64], freq: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, s) in samples.iter().enumerate() {
        let a = 2.0 * PI * freq * i as f64 / 48_000.0;
        re += s * a.cos();
        im += s * a.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len() as f64
}

/// The left channel of the card's play file between two byte offsets, as -1..1.
fn left(path: &Path, from: usize, to: usize) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let (from, to) = (from.min(bytes.len()) / 4 * 4, to.min(bytes.len()) / 4 * 4);
    bytes[from..to]
        .chunks_exact(4)
        .map(|f| f64::from(i16::from_le_bytes([f[0], f[1]])) / 32768.0)
        .collect()
}

fn size(path: &Path) -> usize {
    std::fs::metadata(path)
        .map(|m| m.len() as usize)
        .unwrap_or(0)
}

/// 20 ms of a sine at `rate`, `channels`, continuing at `phase` (in samples).
fn tone(freq: f64, amp: f64, rate: u32, channels: usize, phase: &mut usize) -> Vec<i16> {
    let n = rate as usize / 50;
    let mut out = Vec::with_capacity(n * channels);
    for _ in 0..n {
        let s = (amp * (2.0 * PI * freq * *phase as f64 / f64::from(rate)).sin() * 32767.0) as i16;
        for _ in 0..channels {
            out.push(s);
        }
        *phase += 1;
    }
    out
}

fn connect(
    socket: &Path,
    class: Class,
    direction: Direction,
    rate: u32,
    ch: u8,
) -> Result<Stream, ConnectError> {
    Stream::connect(socket, Hello::new(class, direction, rate, ch))
}

/// The daemon on a fake card, its stream sockets and the card's two files.
struct Bench {
    world: World,
    sockets: std::path::PathBuf,
    play: std::path::PathBuf,
    capture: std::path::PathBuf,
    _daemon: ultar_svckit::testkit::Daemon,
}

impl Bench {
    fn start(tag: &str) -> Bench {
        let world = World::new(tag, POLICY, &IDENTITIES);
        let (sockets, card) = (world.subdir("sock"), world.subdir("card"));
        let (play, capture) = (card.join("play.pcm"), card.join("capture.pcm"));
        std::fs::write(&play, b"").unwrap();
        std::fs::write(&capture, b"").unwrap();
        let args = [
            ("--state", world.subdir("state")),
            ("--root", world.subdir("root")),
            ("--play-file", play.clone()),
            ("--capture-file", capture.clone()),
            ("--policy", world.policy_file()),
            ("--stream-dir", sockets.clone()),
        ];
        let mut argv: Vec<&str> = Vec::new();
        for (k, v) in &args {
            argv.push(k);
            argv.push(v.to_str().unwrap());
        }
        argv.extend(["--streams", "system.calld,system.mediad,app.nobody"]);
        let daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-audiod"), "system.audiod", &argv);
        let bench = Bench {
            world,
            sockets,
            play,
            capture,
            _daemon: daemon,
        };
        for who in ["system.calld", "system.mediad", "app.nobody"] {
            assert!(
                wait_until(WAIT, || bench.sock(who).exists()),
                "no socket for {who}"
            );
        }
        bench
    }

    fn sock(&self, who: &str) -> std::path::PathBuf {
        self.sockets.join(format!("{who}.sock"))
    }

    fn open(&self, who: &str, class: Class, dir: Direction, rate: u32, ch: u8) -> Stream {
        connect(&self.sock(who), class, dir, rate, ch).unwrap()
    }

    /// One second of a 700 Hz tone, stereo, appended to the fake card's microphone.
    fn speak_into_the_microphone(&self) {
        let (mut phase, mut mic) = (0, Vec::new());
        for _ in 0..50 {
            mic.extend(tone(700.0, 0.3, 48_000, 2, &mut phase));
        }
        let bytes: Vec<u8> = mic.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&self.capture)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes))
            .unwrap();
    }
}

#[test]
fn only_the_bus_policy_decides_who_plays_and_who_records() {
    let b = Bench::start("sound-rights");
    let denied =
        |r: Result<Stream, ConnectError>| matches!(r, Err(ConnectError::Refused(Status::Denied)));
    let nobody = b.sock("app.nobody");
    let mediad = b.sock("system.mediad");
    assert!(denied(connect(
        &nobody,
        Class::Media,
        Direction::Play,
        48_000,
        2
    )));
    assert!(denied(connect(
        &nobody,
        Class::Media,
        Direction::Capture,
        16_000,
        1
    )));
    assert!(denied(connect(
        &mediad,
        Class::Media,
        Direction::Duplex,
        48_000,
        1
    )));
    assert!(denied(connect(
        &mediad,
        Class::Call,
        Direction::Play,
        48_000,
        1
    )));
    let nobody = b.world.client("app.nobody");
    assert!(matches!(
        nobody.call(&Capture { on: true }, Duration::from_secs(5)),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
}

/// The microphone reached the call and voice control, each at its own rate; then voice control
/// stops listening: the state says so.
fn microphone_reaches_both_and_voice_control_lets_go(
    heard: &[i16],
    chunks: &TypedSubscription<MicChunk>,
    states: &TypedSubscription<MicState>,
    voiced: &Client,
) {
    assert!(!heard.is_empty(), "the call hears the microphone");
    assert!(
        wait_until(WAIT, || chunks
            .try_recv()
            .ok()
            .flatten()
            .is_some_and(|c| c.rate == 16_000 && !c.samples.is_empty())),
        "voice control gets audio.mic"
    );
    let state = loop {
        match states.try_recv() {
            Ok(Some(s)) if s.in_use => break s,
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    assert!(
        state.users.iter().any(|u| u == "system.voiced"),
        "{state:?}"
    );

    // voice control stops listening: the state says so, the call still hears
    let off = voiced.call(&Capture { on: false }, Duration::from_secs(5));
    assert_eq!(off.unwrap(), AudioDone { ok: true });
    assert!(wait_until(WAIT, || {
        let mut off = false;
        while let Ok(Some(s)) = states.try_recv() {
            off = !s.users.iter().any(|u| u == "system.voiced");
        }
        off
    }));
}

#[test]
fn a_call_the_player_and_voice_control_share_one_card_each_with_its_own_right() {
    let b = Bench::start("sound-mix");
    let shell = b.world.client("system.shell");
    let states = shell.subscribe::<MicState>().unwrap();
    let voiced = b.world.client("system.voiced");
    let chunks = voiced.subscribe::<MicChunk>().unwrap();

    // the player alone
    let mut media = b.open("system.mediad", Class::Media, Direction::Play, 48_000, 2);
    let mut media_phase = 0;
    for _ in 0..50 {
        let t = tone(440.0, 0.5, 48_000, 2, &mut media_phase);
        media.push(&t).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let end_alone = size(&b.play);
    let alone = amplitude(
        &left(&b.play, end_alone.saturating_sub(96_000), end_alone),
        440.0,
    );
    assert!(alone > 0.4, "the player's tone reaches the card: {alone}");

    // the call joins, voice control listens: all at the same time
    let mut call = b.open("system.calld", Class::Call, Direction::Duplex, 16_000, 1);
    let reply = voiced.call(&Capture { on: true }, Duration::from_secs(5));
    assert_eq!(reply.unwrap(), AudioDone { ok: true });
    assert!(wait_until(WAIT, || states
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s.in_use)));
    std::thread::sleep(Duration::from_millis(200));
    b.speak_into_the_microphone();
    let (mut call_phase, mut heard) = (0, Vec::new());
    for _ in 0..75 {
        call.push(&tone(1_000.0, 0.5, 16_000, 1, &mut call_phase))
            .unwrap();
        media
            .push(&tone(440.0, 0.5, 48_000, 2, &mut media_phase))
            .unwrap();
        call.pull(&mut heard).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let end_call = size(&b.play);
    let window = left(&b.play, end_call.saturating_sub(48_000 * 3), end_call);
    let (under_call, call_level) = (amplitude(&window, 440.0), amplitude(&window, 1_000.0));
    let ducked_db = 20.0 * (under_call / alone).log10();
    println!("media under the call: {ducked_db:.1} dB; call tone {call_level:.2}");
    assert!(
        call_level > 0.3,
        "the call is heard at full level: {call_level}"
    );
    assert!(
        (-19.0..=-11.0).contains(&ducked_db),
        "media ducked by {ducked_db} dB"
    );

    microphone_reaches_both_and_voice_control_lets_go(&heard, &chunks, &states, &voiced);
}
