//! The test bench of the real `ultar-voiced`: a bus with the real sound server (`ultar-audiod`
//! on a fake card: two files) and a stand-in for the helper, and stand-in programs for the
//! speech recogniser (a small HTTP server) and the speech output. The audio is synthetic
//! (tones shaped like words): it is written into the fake card's capture file and comes out
//! of the sound server as `audio.mic`; what voiced says is read from the card's play file.

use std::f32::consts::PI;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use ultar_bus::messages::agent::{Answer, AskVoice, Brain};
use ultar_bus::messages::audio::MicState;
use ultar_bus::messages::voice::{Enroll, EnrollKind, Phase, SetEnabled, VoiceOutcome, VoiceState};
use ultar_bus::{BusError, Call, Client, ErrorCode, Text, TypedSubscription};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.voiced"
register = ["voice"]
publish = ["voice.state"]
subscribe = ["audio.mic"]
call = ["audio.capture", "audio.play", "agent.ask_voice"]

[[rule]]
identity = "system.audiod"
register = ["audio"]
publish = ["audio.state", "audio.mic", "audio.mic_state"]
subscribe = ["settings.changed"]
call = ["settings.get"]

[[rule]]
identity = "system.stand_in"
register = ["agent"]

[[rule]]
identity = "system.shell"
call = ["voice.*"]
subscribe = ["voice.state", "audio.mic_state"]
"#;

const WHISPER: &str = r#"#!/usr/bin/env python3
import json, os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
port = int(args[args.index("--port") + 1])
with open(os.path.join(here, "started.log"), "a") as f:
    f.write(" ".join(args) + "\n")
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        with open(os.path.join(here, "requests.log"), "a") as f:
            f.write("%s %d %s\n" % (self.path, len(body), b"RIFF" in body))
        with open(os.path.join(here, "text.txt"), encoding="utf-8") as f:
            text = f.read().strip()
        out = json.dumps({"text": " " + text}, ensure_ascii=False).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", port), H).serve_forever()
"#;

const ESPEAK: &str = r#"#!/usr/bin/env python3
import os, struct, sys, math
here = os.path.dirname(os.path.abspath(__file__))
text = sys.stdin.buffer.read().decode("utf-8")
with open(os.path.join(here, "said.log"), "a", encoding="utf-8") as f:
    f.write(text.replace("\n", " ") + "\n")
rate = 22050
pcm = b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / rate))) for i in range(rate // 5))
# like the real espeak-ng into a pipe: the length is unknown, the largest sizes are written
header = b"RIFF" + struct.pack("<I", 0x7FFFF024) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16) + b"data" + struct.pack("<I", 0x7FFFF000)
sys.stdout.buffer.write(header + pcm)
"#;

/// A synthetic word: bursts around `base` Hz, `speed` stretches it in time.
pub fn word(base: f32, speed: f32, secs: f32) -> Vec<f32> {
    let len = (secs * 16_000.0 * speed) as usize;
    (0..len)
        .map(|i| {
            let t = i as f32 / 16_000.0 / speed;
            let burst = 0.55 + 0.45 * (2.0 * PI * 3.0 * t).sin();
            let glide = base * (1.0 + 0.3 * (2.0 * PI * 1.5 * t).sin());
            (2.0 * PI * glide * t).sin() * 0.25 * burst
        })
        .collect()
}

pub fn quiet(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * 16_000.0) as usize]
}

/// The word, a pause, then the request, then the pause that ends it.
pub fn word_then_request(base: f32, request: &[f32]) -> Vec<f32> {
    let mut s = quiet(0.5);
    s.extend(word(base, 1.02, 0.75));
    s.extend(quiet(1.0));
    s.extend(request);
    s.extend(quiet(1.6));
    s
}

/// What was seen: the microphone state the sound server published (every change after the
/// start, `true` for in use), and what the helper was asked.
#[derive(Default)]
pub struct Seen {
    pub capture: Mutex<Vec<bool>>,
    pub asked: Mutex<Vec<(String, bool)>>,
}

pub struct Bench {
    _world: World,
    _daemon: Daemon,
    _audiod: Daemon,
    shell: Client,
    pub seen: Arc<Seen>,
    play_file: PathBuf,
    capture_file: PathBuf,
    states: TypedSubscription<VoiceState>,
    last: Mutex<Option<VoiceState>>,
    dir: PathBuf,
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(30));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// The real `ultar-audiod`, built once (the test binary of another package cannot name it).
fn audiod_binary() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = std::process::Command::new(cargo)
            .args(["build", "-q", "-p", "ultar-audiod", "--bin", "ultar-audiod"])
            .status()
            .unwrap();
        assert!(status.success(), "ultar-audiod did not build");
        let exe = std::env::current_exe().unwrap();
        // target/debug/deps/<test> -> target/debug/ultar-audiod
        exe.parent().unwrap().parent().unwrap().join("ultar-audiod")
    })
    .clone()
}

/// The helper of the bench.
fn stand_ins(world: &World, seen: &Arc<Seen>) {
    let s = seen.clone();
    world.serve("system.stand_in", "agent", move |request| {
        if let Ok(ask) = request.decode::<AskVoice>() {
            s.asked.lock().unwrap().push((ask.text, ask.locked));
            let _ = request.reply(&Answer {
                text: Text::key("agent.done"),
                brain: Brain::Minimal,
                steps: Vec::new(),
            });
        }
    });
}

/// The models and programs; `install`: a model is on the device.
fn programs(world: &World, install: bool) -> (PathBuf, PathBuf) {
    let dir = world.subdir("voice-programs");
    let models = dir.join("models");
    fs::create_dir_all(&models).unwrap();
    if install {
        fs::write(models.join("ggml-small.bin"), b"model").unwrap();
    }
    script(&dir, "whisper-server", WHISPER);
    script(&dir, "espeak-ng", ESPEAK);
    fs::write(dir.join("text.txt"), "").unwrap();
    (dir, models)
}

/// The sound server on a fake card (two files): the daemon, the play file, the capture file.
fn sound_server(world: &World) -> (Daemon, PathBuf, PathBuf) {
    let card = world.subdir("card");
    let (play_file, capture_file) = (card.join("play.pcm"), card.join("capture.pcm"));
    fs::write(&play_file, b"").unwrap();
    fs::write(&capture_file, b"").unwrap();
    let audiod = world.spawn(
        audiod_binary().to_str().unwrap(),
        "system.audiod",
        &[
            "--state",
            world.subdir("audio-state").to_str().unwrap(),
            "--root",
            world.subdir("audio-root").to_str().unwrap(),
            "--play-file",
            play_file.to_str().unwrap(),
            "--capture-file",
            capture_file.to_str().unwrap(),
        ],
    );
    (audiod, play_file, capture_file)
}

/// Writes every change of "microphone in use" into `seen.capture`.
fn watch_microphone(mic_states: TypedSubscription<MicState>, seen: Arc<Seen>) {
    std::thread::spawn(move || {
        let mut before = false;
        loop {
            match mic_states.recv_timeout(Duration::from_millis(100)) {
                Ok(s) if s.in_use != before => {
                    before = s.in_use;
                    seen.capture.lock().unwrap().push(s.in_use);
                }
                Ok(_) | Err(BusError::Timeout) => {}
                Err(_) => return,
            }
        }
    });
}

/// The service with a bench around it; `install`: the speech model exists.
pub fn bench(tag: &str, install: bool) -> Bench {
    let world = World::new(
        tag,
        POLICY,
        &[
            "system.voiced",
            "system.audiod",
            "system.stand_in",
            "system.shell",
        ],
    );
    let seen = Arc::new(Seen::default());
    stand_ins(&world, &seen);
    let (audiod, play_file, capture_file) = sound_server(&world);
    let (dir, models) = programs(&world, install);
    let state = world.subdir("voice-state");
    let port = free_port().to_string();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-voiced"),
        "system.voiced",
        &[
            "--state",
            state.to_str().unwrap(),
            "--models",
            models.to_str().unwrap(),
            "--whisper",
            &path("whisper-server"),
            "--espeak",
            &path("espeak-ng"),
            "--port",
            &port,
        ],
    );
    let shell = world.client("system.shell");
    let states = shell.subscribe::<VoiceState>().unwrap();
    let mic_states = shell.subscribe::<MicState>().unwrap();
    watch_microphone(mic_states, seen.clone());
    Bench {
        _world: world,
        _daemon: daemon,
        _audiod: audiod,
        shell,
        seen,
        play_file,
        capture_file,
        states,
        last: Mutex::new(None),
        dir,
    }
}

impl Bench {
    /// Waits until the published state satisfies `f`; returns it.
    pub fn until(&self, what: &str, f: impl Fn(&VoiceState) -> bool) -> VoiceState {
        let mut found = None;
        let ok = wait_until(Duration::from_secs(40), || {
            while let Ok(Some(state)) = self.states.try_recv() {
                *self.last.lock().unwrap() = Some(state);
            }
            let last = self.last.lock().unwrap().clone();
            if let Some(state) = last.filter(|s| f(s)) {
                found = Some(state);
                return true;
            }
            false
        });
        assert!(
            ok,
            "never: {what}; last state {:?}",
            self.last.lock().unwrap()
        );
        found.unwrap()
    }

    pub fn ask<C: Call>(&self, c: &C) -> C::Reply {
        call(&self.shell, c).unwrap()
    }

    /// Waits until the sound server says the microphone is in use.
    fn wait_for_microphone(&self) {
        let on = wait_until(WAIT, || {
            self.seen.capture.lock().unwrap().last() == Some(&true)
        });
        assert!(on, "the sound server never opened the microphone");
        // the server throws away what was recorded before it began to listen: let it begin
        std::thread::sleep(Duration::from_millis(150));
    }

    /// Speaks into the fake microphone: the audio (16 kHz) goes into the card's capture file as
    /// 48 kHz stereo, and the sound server hands it to voiced as `audio.mic`.
    pub fn speak(&self, samples: &[f32]) {
        self.wait_for_microphone();
        self.write_audio(samples);
    }

    /// Speaks while the microphone is off: nobody asked for it, so the sound server never
    /// hands it on (what piled up in the card is thrown away when a listener begins).
    pub fn speak_unheard(&self, samples: &[f32]) {
        self.write_audio(samples);
    }

    fn write_audio(&self, samples: &[f32]) {
        let mut bytes = Vec::with_capacity(samples.len() * 12);
        for (i, s) in samples.iter().enumerate() {
            let next = samples.get(i + 1).copied().unwrap_or(*s);
            for k in 0..3 {
                let v = s + (next - s) * (k as f32 / 3.0);
                let q = (v.clamp(-1.0, 1.0) * 32_767.0) as i16;
                bytes.extend_from_slice(&q.to_le_bytes());
                bytes.extend_from_slice(&q.to_le_bytes());
            }
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&self.capture_file)
            .unwrap();
        for chunk in bytes.chunks(48_000 * 4 / 10) {
            file.write_all(chunk).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Bytes the sound server has written to the card's play file.
    pub fn played_bytes(&self) -> usize {
        fs::metadata(&self.play_file)
            .map(|m| m.len() as usize)
            .unwrap_or(0)
    }

    /// How loud the tone `hz` is in everything the sound server has played so far (the mean
    /// amplitude over the whole play file, 0..1): the listening tone is 880 Hz, the stand-in
    /// speech output 440 Hz, so each can be told from the other.
    pub fn tone_level(&self, hz: f64) -> f64 {
        let bytes = fs::read(&self.play_file).unwrap_or_default();
        let left: Vec<f64> = bytes
            .chunks_exact(4)
            .map(|f| f64::from(i16::from_le_bytes([f[0], f[1]])) / 32_768.0)
            .collect();
        if left.is_empty() {
            return 0.0;
        }
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in left.iter().enumerate() {
            let a = 2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0;
            re += s * a.cos();
            im += s * a.sin();
        }
        2.0 * (re * re + im * im).sqrt() / left.len() as f64
    }

    pub fn set_text(&self, text: &str) {
        fs::write(self.dir.join("text.txt"), text).unwrap();
    }

    pub fn log(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    /// How many requests the helper has heard.
    pub fn asked(&self) -> usize {
        self.seen.asked.lock().unwrap().len()
    }

    /// One enrolment take of `kind`: the call, then the audio.
    fn take(&self, kind: EnrollKind, audio: &[f32], clips: impl Fn(&VoiceState) -> u8, n: u8) {
        let reply = self.ask(&Enroll { kind });
        assert_eq!(reply.outcome, VoiceOutcome::Done);
        self.until("the take is awaited", |s| s.phase == Phase::Enrolling);
        self.speak(audio);
        self.until("the take counts", |s| clips(s) >= n);
    }

    /// The wake word or the secret word, three takes at `base` Hz.
    pub fn enrol_word(&self, kind: EnrollKind, base: f32) {
        let clips = move |s: &VoiceState| match kind {
            EnrollKind::Wake => s.wake.clips,
            _ => s.secret.clips,
        };
        for (n, speed) in [1.0f32, 1.08, 0.94].iter().enumerate() {
            let mut audio = quiet(0.4);
            audio.extend(word(base, *speed, 0.75));
            audio.extend(quiet(0.7));
            self.take(kind, &audio, clips, n as u8 + 1);
        }
    }

    fn enrol_voice(&self, base: f32) {
        for (n, speed) in [1.0f32, 1.1, 0.9].iter().enumerate() {
            let mut audio = quiet(0.4);
            audio.extend(word(base, *speed, 2.0));
            audio.extend(quiet(0.7));
            self.take(EnrollKind::Voice, &audio, |s| s.voice.clips, n as u8 + 1);
        }
    }
}

/// Trained (wake word 400 Hz, secret word 1500 Hz, voice 500 Hz), switched on and listening.
pub fn listening(tag: &str) -> Bench {
    let b = bench(tag, true);
    b.until("the service is up", |_| true);
    b.enrol_word(EnrollKind::Wake, 400.0);
    b.enrol_word(EnrollKind::Secret, 1_500.0);
    b.enrol_voice(500.0);
    b.until("trained", |s| {
        s.wake.enrolled && s.secret.enrolled && s.voice.enrolled
    });
    assert_eq!(b.ask(&SetEnabled { on: true }).outcome, VoiceOutcome::Done);
    b.until("it listens", |s| s.phase == Phase::Listening && s.ready.mic);
    b
}
