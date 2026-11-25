//! Live check of a call through the release server and its TURN relay (not part of the normal
//! run): two devices of the check account call each other, the caller with `relay_only` – its
//! voice and picture go sealed through the relay, the server and the relay see ciphertext only.
//! (Both sides relay-only would need relay-to-relay on one server, which the relay refuses: it
//! denies its own address as a peer.)
//!
//! ```text
//! ULTAR_LIVE_USER=<account> ULTAR_LIVE_PASSWORD=<password> ULTAR_LIVE_HOME=<dir that persists> \
//!   cargo test --release --test live_call -- --ignored --nocapture
//! ```

use std::net::ToSocketAddrs;
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
use ultar_cloud::{Client, CloudError, DeviceHome, FileKeyStore, KeyStore, SignalProof};

const BASE: &str = "https://ultar.code-craft.tech";

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

#[derive(Clone, Default)]
struct Room {
    hz: f32,
    heard: Arc<Mutex<Vec<i16>>>,
    seen: Arc<AtomicU32>,
}

struct Mic(Room, usize);
impl AudioIo for Mic {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let hz = self.0.hz;
        let f = (0..FRAME)
            .map(|i| {
                (6000.0
                    * ((self.1 + i) as f32 / audio::RATE as f32 * hz * std::f32::consts::TAU).sin())
                    as i16
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

struct Live {
    home: DeviceHome,
    token: String,
    room: Room,
    relay_only: bool,
}

impl Device for Live {
    fn server(&self) -> Option<String> {
        Some(BASE.into())
    }
    fn auth(&self) -> Option<Box<dyn SignalAuth>> {
        Some(Box::new(Proof(self.home.signal_proof(BASE).ok()?)))
    }
    fn signer(&self) -> Box<dyn Signer> {
        Box::new(Key(self.home.key_store().unwrap()))
    }
    fn turn(&self) -> Option<TurnConfig> {
        let c = Client::new(BASE, Some(self.token.clone()))
            .turn_credentials()
            .ok()?;
        let server = c
            .udp_server()?
            .to_socket_addrs()
            .ok()?
            .find(|a| a.is_ipv4())?;
        Some(TurnConfig {
            server,
            user: c.username,
            password: c.credential,
        })
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
    fn relay_only(&self) -> bool {
        self.relay_only
    }
}

fn var(n: &str) -> String {
    std::env::var(n).unwrap_or_else(|_| panic!("{n} is not set"))
}

fn token() -> String {
    let anon = Client::new(BASE, None);
    match anon.login(&var("ULTAR_LIVE_USER"), &var("ULTAR_LIVE_PASSWORD")) {
        Ok(t) => t,
        Err(CloudError::InvalidCredentials) => anon
            .register(&var("ULTAR_LIVE_USER"), &var("ULTAR_LIVE_PASSWORD"))
            .unwrap(),
        Err(e) => panic!("{e:?}"),
    }
}

fn device(name: &str, hz: f32, token: &str, relay_only: bool) -> (E2eCall, String, Room) {
    let home = DeviceHome::new(std::path::Path::new(&var("ULTAR_LIVE_HOME")).join(name));
    let number = home
        .register_number(&Client::new(BASE, Some(token.into())), name, None)
        .unwrap();
    let room = Room {
        hz,
        ..Room::default()
    };
    let dev = Live {
        home,
        token: token.into(),
        room: room.clone(),
        relay_only,
    };
    (E2eCall::new(Arc::new(dev)), number, room)
}

fn until(b: &mut E2eCall, f: impl Fn(&CallEvent) -> bool, ev: &mut Vec<CallEvent>) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(25) {
        let got = b.poll();
        let hit = got.iter().any(&f);
        ev.extend(got);
        if hit {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

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

#[test]
#[ignore = "needs the network and an account on the live server"]
fn a_video_call_through_the_live_server_and_its_relay() {
    let t = token();
    let (mut a, _, room_a) = device("call-a", 440.0, &t, true);
    let (mut b, number_b, room_b) = device("call-b", 300.0, &t, false);
    let start = Instant::now();
    while !(a.ready() && b.ready()) && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(a.ready() && b.ready(), "both signed in");
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    a.place(&number_b, true);
    assert!(
        until(
            &mut b,
            |e| matches!(e, CallEvent::IncomingCall { video: true, .. }),
            &mut eb
        ),
        "{eb:?}"
    );
    b.answer();
    assert!(
        until(&mut a, |e| matches!(e, CallEvent::Answered), &mut ea),
        "{ea:?}"
    );
    println!("ok call connected through the relay: {ea:?}");
    std::thread::sleep(Duration::from_secs(6));
    a.hangup();
    assert!(until(&mut b, |e| matches!(e, CallEvent::Ended), &mut eb));
    let hb = room_b.heard.lock().unwrap().clone();
    let ha = room_a.heard.lock().unwrap().clone();
    let (tb, ta) = (&hb[hb.len() / 2..], &ha[ha.len() / 2..]);
    println!(
        "b hears 440: {:.0} vs 300: {:.0}; pictures a {} b {}",
        power(tb, 440.0),
        power(tb, 300.0),
        room_a.seen.load(Ordering::Relaxed),
        room_b.seen.load(Ordering::Relaxed)
    );
    assert!(power(tb, 440.0) > 20.0 * power(tb, 300.0), "b hears a");
    assert!(power(ta, 300.0) > 20.0 * power(ta, 440.0), "a hears b");
    assert!(
        room_a.seen.load(Ordering::Relaxed) >= 10 && room_b.seen.load(Ordering::Relaxed) >= 10,
        "pictures"
    );
}
