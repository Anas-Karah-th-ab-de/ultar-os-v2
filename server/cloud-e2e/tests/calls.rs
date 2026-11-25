//! A call between two devices of two accounts through the real server, end to end: the device
//! client's backend (`ultar-call`: signalling, handshake with the device keys, media sealed per
//! call) against `ultraos-server`. Checked: both hear each other and see the picture, both show
//! the same safety number, and the server's database is the same before and after the call –
//! it keeps nothing about it.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_call::e2e::audio::{self, FRAME};
use ultar_call::e2e::engine::Signer;
use ultar_call::e2e::media::{AudioIo, VideoIo};
use ultar_call::e2e::net::TurnConfig;
use ultar_call::e2e::pins::Pins;
use ultar_call::e2e::video::{self, Picture};
use ultar_call::{CallBackend, CallEvent, Device, E2eCall, SignalAuth};
use ultar_cloud::{DeviceHome, FileKeyStore, KeyStore, SignalProof};
use ultar_cloud_e2e::Stack;

struct Proof(SignalProof);

impl SignalAuth for Proof {
    fn hello(&self) -> Result<String, String> {
        self.0.hello().map_err(|e| format!("{e:?}"))
    }
    fn answer(&self, c: &str) -> Result<String, String> {
        self.0.answer(c).map_err(|e| format!("{e:?}"))
    }
}

struct Key(FileKeyStore);

impl Signer for Key {
    fn public(&self) -> Option<[u8; 32]> {
        self.0.public_key().ok()
    }
    fn sign(&self, m: &[u8]) -> Option<[u8; 64]> {
        self.0.sign(m).ok()
    }
}

/// A microphone saying one tone, a speaker that records; a camera with bars, a screen that counts.
#[derive(Clone)]
struct Room {
    hz: f32,
    heard: Arc<Mutex<Vec<i16>>>,
    seen: Arc<AtomicU32>,
}

struct Mic(Room, usize);

impl AudioIo for Mic {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let f = (0..FRAME)
            .map(|i| {
                let t = (self.1 + i) as f32 / audio::RATE as f32;
                (6000.0 * (t * self.0.hz * std::f32::consts::TAU).sin()) as i16
            })
            .collect();
        self.1 += FRAME;
        Some(f)
    }
    fn play(&mut self, frame: &[i16]) {
        self.0.heard.lock().unwrap().extend_from_slice(frame);
    }
}

struct Cam(Room, u32);

impl VideoIo for Cam {
    fn capture(&mut self) -> Option<Picture> {
        self.1 += 1;
        Some(video::bars(self.1))
    }
    fn show(&mut self, _p: Picture) {
        self.0.seen.fetch_add(1, Ordering::Relaxed);
    }
}

struct TestDevice {
    base: String,
    home: DeviceHome,
    room: Room,
}

impl Device for TestDevice {
    fn server(&self) -> Option<String> {
        Some(self.base.clone())
    }
    fn auth(&self) -> Option<Box<dyn SignalAuth>> {
        Some(Box::new(Proof(self.home.signal_proof(&self.base).ok()?)))
    }
    fn signer(&self) -> Box<dyn Signer> {
        Box::new(Key(self.home.key_store().unwrap()))
    }
    fn turn(&self) -> Option<TurnConfig> {
        None
    }
    fn audio(&self) -> Box<dyn AudioIo> {
        Box::new(Mic(self.room.clone(), 0))
    }
    fn video(&self) -> Option<Box<dyn VideoIo>> {
        Some(Box::new(Cam(self.room.clone(), 0)))
    }
    fn pins(&self) -> Pins {
        Pins::default()
    }
}

fn device(s: &Stack, user: &str, hz: f32) -> (E2eCall, String, Room) {
    let client = s.signed_in(user);
    let home = s.device_home(user);
    let number = home.register_number(&client, user, None).unwrap();
    let room = Room {
        hz,
        heard: Arc::default(),
        seen: Arc::default(),
    };
    let dev = TestDevice {
        base: s.base(),
        home,
        room: room.clone(),
    };
    (E2eCall::new(Arc::new(dev)), number, room)
}

fn until(b: &mut E2eCall, f: impl Fn(&CallEvent) -> bool, events: &mut Vec<CallEvent>) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        let got = b.poll();
        let hit = got.iter().any(&f);
        events.extend(got);
        if hit {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn tables(s: &Stack) -> Vec<(String, i64)> {
    let db = rusqlite::Connection::open(s.dir.path().join("server.sqlite")).unwrap();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
        .into_iter()
        .map(|t| {
            let n = db
                .query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))
                .unwrap();
            (t, n)
        })
        .collect()
}

/// Strength of a tone (Goertzel).
fn power(s: &[i16], hz: f32) -> f64 {
    let k = 2.0 * (std::f64::consts::TAU * f64::from(hz) / f64::from(audio::RATE)).cos();
    let (mut a, mut b) = (0f64, 0f64);
    for x in s {
        let c = f64::from(*x) + k * a - b;
        b = a;
        a = c;
    }
    (a * a + b * b - k * a * b) / s.len().max(1) as f64
}

/// The safety number of a first call (key not changed), if the call was secured.
fn safety(ev: &[CallEvent]) -> Option<String> {
    ev.iter().find_map(|e| match e {
        CallEvent::Secured {
            safety,
            changed: false,
        } => Some(safety.clone()),
        _ => None,
    })
}

/// Both rooms heard the other side's tone and saw at least ten pictures.
fn both_heard_and_saw(anna_room: &Room, ben_room: &Room) {
    let heard = ben_room.heard.lock().unwrap().clone();
    let tail = &heard[heard.len() / 3..];
    assert!(
        power(tail, 440.0) > 50.0 * power(tail, 300.0),
        "ben hears anna"
    );
    let heard = anna_room.heard.lock().unwrap().clone();
    let tail = &heard[heard.len() / 3..];
    assert!(
        power(tail, 300.0) > 50.0 * power(tail, 440.0),
        "anna hears ben"
    );
    assert!(
        ben_room.seen.load(Ordering::Relaxed) >= 10,
        "ben sees anna's picture"
    );
    assert!(
        anna_room.seen.load(Ordering::Relaxed) >= 10,
        "anna sees ben's picture"
    );
}

#[test]
fn a_video_call_end_to_end_and_the_server_keeps_nothing() {
    let s = Stack::start();
    let (mut anna, _, anna_room) = device(&s, "anna", 440.0);
    let (mut ben, ben_number, ben_room) = device(&s, "ben", 300.0);
    let start = Instant::now();
    while !(anna.ready() && ben.ready()) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        anna.ready() && ben.ready(),
        "both signed in at the signalling socket"
    );
    let before = tables(&s);
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    anna.place(&ben_number, true);
    assert!(
        until(
            &mut ben,
            |e| matches!(e, CallEvent::IncomingCall { video: true, .. }),
            &mut eb
        ),
        "{eb:?}"
    );
    ben.answer();
    assert!(
        until(&mut anna, |e| matches!(e, CallEvent::Answered), &mut ea),
        "{ea:?}"
    );
    assert!(safety(&ea).is_some());
    assert_eq!(
        safety(&ea),
        safety(&eb),
        "both phones show the same safety number"
    );
    std::thread::sleep(Duration::from_secs(3));
    anna.hangup();
    assert!(until(&mut ben, |e| matches!(e, CallEvent::Ended), &mut eb));
    both_heard_and_saw(&anna_room, &ben_room);
    assert_eq!(
        tables(&s),
        before,
        "the call left nothing in the server's database"
    );
}
