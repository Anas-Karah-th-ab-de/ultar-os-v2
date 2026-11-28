//! Flow tests of the engine with real sockets, a fake card and a clock of their own.

use super::*;
use crate::device::fake::{Card, Fake};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use ultar_audio_stream::client::{ConnectError, Stream};
use ultar_audio_stream::{Direction, Hello};

const STEP: Duration = Duration::from_millis(5);

fn play_rights() -> Rights {
    Rights {
        play: true,
        ..Rights::default()
    }
}

fn call_rights() -> Rights {
    Rights {
        call: true,
        capture: true,
        ..Rights::default()
    }
}

fn mic_rights() -> Rights {
    Rights {
        capture: true,
        ..Rights::default()
    }
}

/// The user id the tests run as, as the kernel reports it for a socket pair.
fn my_uid() -> u32 {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    peer_credentials(&a).unwrap().uid
}

struct Rig {
    eng: Engine,
    card: Arc<Mutex<Card>>,
    slot: Arc<Mutex<Option<Fake>>>,
    dir: PathBuf,
    now: Instant,
    events: Vec<Event>,
}

impl Rig {
    fn new(tag: &str, who: &[(&str, Rights)]) -> Rig {
        let mut r = Rig::without_card(tag, who);
        r.plug();
        r
    }

    fn without_card(tag: &str, who: &[(&str, Rights)]) -> Rig {
        Rig::build(tag, who, my_uid())
    }

    /// The sockets belong to the identities of user `uid` (the peers are `my_uid()`).
    fn build(tag: &str, who: &[(&str, Rights)], uid: u32) -> Rig {
        let dir = std::env::temp_dir().join(format!("audiod-eng-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (_, card) = Fake::new();
        let slot: Arc<Mutex<Option<Fake>>> = Arc::new(Mutex::new(None));
        let opener_slot = slot.clone();
        let opener: Opener = Box::new(move || match opener_slot.lock().unwrap().take() {
            Some(f) => Ok(Box::new(f) as Box<dyn Device>),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        });
        let specs = who
            .iter()
            .map(|(id, rights)| ListenerSpec {
                path: dir.join(format!("{id}.sock")),
                identity: (*id).to_owned(),
                rights: *rights,
                uid,
                mode: Some(0o666),
            })
            .collect();
        Rig {
            eng: Engine::new(specs, opener).unwrap(),
            card,
            slot,
            dir,
            now: Instant::now(),
            events: Vec::new(),
        }
    }

    /// The card is there (again): the engine finds it at its next look.
    fn plug(&mut self) {
        self.card.lock().unwrap().gone = false;
        *self.slot.lock().unwrap() = Some(Fake(self.card.clone()));
        self.step();
    }

    fn step(&mut self) {
        self.now += STEP;
        self.eng.step(self.now);
        self.events.extend(self.eng.drain_events());
    }

    fn run(&mut self, ms: u64) {
        for _ in 0..ms / 5 {
            self.step();
        }
    }

    fn connect(&mut self, who: &str, hello: Hello) -> Result<Stream, ConnectError> {
        let path = self.dir.join(format!("{who}.sock"));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(Stream::connect(&path, hello));
        });
        loop {
            self.step();
            if let Ok(r) = rx.try_recv() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn played(&self) -> Vec<i16> {
        self.card.lock().unwrap().played.clone()
    }

    fn mic(&self, stereo: &[i16]) {
        self.card.lock().unwrap().mic.extend(stereo.iter().copied());
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `frames` frames of a constant level at the client's format (48 kHz stereo).
fn level(frames: usize, v: f32) -> Vec<i16> {
    vec![(v * 32768.0) as i16; frames * 2]
}

/// Mean of the last `n` samples as a fraction of full scale.
fn tail_level(x: &[i16], n: usize) -> f32 {
    let t = &x[x.len() - n..];
    t.iter().map(|&s| f32::from(s)).sum::<f32>() / n as f32 / 32768.0
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn media() -> Hello {
    Hello::new(Class::Media, Direction::Play, 48_000, 2)
}

fn call_play() -> Hello {
    Hello::new(Class::Call, Direction::Play, 48_000, 2)
}

fn capture(rate: u32, channels: u8) -> Hello {
    Hello::new(Class::Media, Direction::Capture, rate, channels)
}

#[test]
fn streams_of_several_programs_are_mixed_with_their_volumes() {
    let mut r = Rig::new(
        "mix",
        &[
            ("system.web", play_rights()),
            ("system.media", play_rights()),
        ],
    );
    let mut a = r.connect("system.web", media()).unwrap();
    let mut half = media();
    half.volume = 500;
    let mut b = r.connect("system.media", half).unwrap();
    for _ in 0..100 {
        a.push(&level(240, 0.2)).unwrap();
        b.push(&level(240, 0.2)).unwrap();
        r.step();
    }
    assert_eq!(r.eng.streams(), 2);
    let played = r.played();
    assert!(
        (tail_level(&played, 400) - 0.3).abs() < 0.002,
        "{}",
        tail_level(&played, 400)
    );
    assert!(r.eng.has_card() && r.eng.stats().opened == 2);
}

#[test]
fn a_call_ducks_media_by_15_db_and_media_comes_back() {
    let mut r = Rig::new(
        "duck",
        &[
            ("system.web", play_rights()),
            ("system.calld", call_rights()),
        ],
    );
    let mut web = r.connect("system.web", media()).unwrap();
    let mut call = r
        .connect(
            "system.calld",
            Hello::new(Class::Call, Direction::Play, 48_000, 2),
        )
        .unwrap();
    for _ in 0..100 {
        web.push(&level(240, 0.4)).unwrap();
        r.step();
    }
    assert!((tail_level(&r.played(), 400) - 0.4).abs() < 0.002);
    for _ in 0..100 {
        web.push(&level(240, 0.4)).unwrap();
        call.push(&level(240, 0.0)).unwrap();
        r.step();
    }
    let db = 20.0 * (tail_level(&r.played(), 400) / 0.4).log10();
    println!("media under a call: {db:.2} dB");
    assert!((db + 15.0).abs() < 0.3, "{db}");
    drop(call);
    for _ in 0..400 {
        web.push(&level(240, 0.4)).unwrap();
        r.step();
    }
    assert!(
        (tail_level(&r.played(), 400) - 0.4).abs() < 0.004,
        "back at full level"
    );
}

#[test]
fn a_client_that_dies_or_stops_reading_never_holds_up_the_others() {
    let mut r = Rig::new(
        "dead",
        &[
            ("system.web", play_rights()),
            ("system.calld", call_rights()),
            ("system.voiced", mic_rights()),
            ("system.mesh", mic_rights()),
        ],
    );
    let mut web = r.connect("system.web", media()).unwrap();
    let mut call = Some(r.connect("system.calld", call_play()).unwrap());
    // one recorder reads, one never does
    let mut reader = r.connect("system.voiced", capture(16_000, 1)).unwrap();
    let _sleeper = r.connect("system.mesh", capture(48_000, 2)).unwrap();
    let mut heard = Vec::new();
    for i in 0..600 {
        web.push(&level(240, 0.3)).unwrap();
        if let Some(c) = call.as_mut() {
            c.push(&level(240, 0.0)).unwrap();
        }
        if i == 200 {
            // the call's program dies in the middle of a stream
            call = None;
        }
        r.mic(&level(240, 0.1));
        r.step();
        reader.pull(&mut heard).unwrap();
    }
    // 3 s of virtual time: web played all along, the reader heard all of it
    assert!(
        (tail_level(&r.played(), 400) - 0.3).abs() < 0.003,
        "web goes on"
    );
    assert!(
        heard.len() > 16_000 * 3 * 95 / 100,
        "reader got {}",
        heard.len()
    );
    let backlogs = r.eng.recorder_backlogs();
    let sleeper = backlogs.iter().find(|(i, _)| i == "system.mesh").unwrap().1;
    assert!(
        sleeper <= 48_000 * 4 * 200 / 1000,
        "bounded backlog {sleeper}"
    );
    assert!(r.eng.streams() >= 3);
}

#[test]
fn rights_decide_who_plays_what_and_who_records() {
    let mut r = Rig::new(
        "rights",
        &[
            ("app.nobody", Rights::default()),
            ("app.player", play_rights()),
            ("system.calld", call_rights()),
        ],
    );
    assert!(matches!(
        r.connect("app.nobody", media()),
        Err(ConnectError::Refused(Status::Denied))
    ));
    assert!(
        matches!(
            r.connect(
                "app.player",
                Hello::new(Class::Media, Direction::Capture, 16_000, 1)
            ),
            Err(ConnectError::Refused(Status::Denied))
        ),
        "a player has no microphone"
    );
    assert!(
        matches!(
            r.connect(
                "app.player",
                Hello::new(Class::Call, Direction::Play, 16_000, 1)
            ),
            Err(ConnectError::Refused(Status::Denied))
        ),
        "and no priority"
    );
    assert!(r.connect("app.player", media()).is_ok());
    assert!(r
        .connect(
            "system.calld",
            Hello::new(Class::Call, Direction::Duplex, 16_000, 1)
        )
        .is_ok());
    assert_eq!(r.eng.stats().denied, 3);
    // nonsense is closed without an answer that means anything
    let mut s = std::os::unix::net::UnixStream::connect(r.dir.join("app.player.sock")).unwrap();
    std::io::Write::write_all(&mut s, &[7u8; 16]).unwrap();
    r.run(50);
    let mut reply = [0u8; 8];
    std::io::Read::read_exact(&mut s, &mut reply).unwrap();
    assert_eq!(Status::decode(&reply), Some(Status::BadHello));
}

#[test]
fn a_peer_with_another_user_id_is_not_the_identity_and_is_closed() {
    // the socket is open to everybody (0666), but it belongs to the identity of another user
    let mut r = Rig::build(
        "otheruid",
        &[("system.calld", call_rights())],
        my_uid().wrapping_add(1),
    );
    r.plug();
    assert!(
        r.connect("system.calld", media()).is_err(),
        "not even a hello is answered"
    );
    assert_eq!(r.eng.stats().wrong_peer, 1);
    assert_eq!(r.eng.stats().opened, 0);
    assert_eq!(r.eng.streams(), 0);
    // the right user is let in on the same kind of socket
    let mut ok = Rig::new("sameuid", &[("system.calld", call_rights())]);
    let call = Hello::new(Class::Call, Direction::Duplex, 16_000, 1);
    assert!(ok.connect("system.calld", call).is_ok());
    assert_eq!(ok.eng.stats().wrong_peer, 0);
}

#[test]
fn without_a_card_streams_are_refused_and_the_card_is_found_later() {
    let mut r = Rig::without_card("nocard", &[("system.web", play_rights())]);
    assert!(matches!(
        r.connect("system.web", media()),
        Err(ConnectError::Refused(Status::NoDevice))
    ));
    r.plug();
    r.run(2_500);
    assert!(r.events.contains(&Event::Card(true)));
    let mut s = r.connect("system.web", media()).unwrap();
    // the card goes: the stream ends, the client sees it and can run silent
    s.push(&level(240, 0.3)).unwrap();
    r.card.lock().unwrap().gone = true;
    for _ in 0..50 {
        s.push(&level(240, 0.3)).ok();
        r.step();
    }
    assert!(!r.eng.has_card() && r.eng.stats().card_lost == 1);
    assert!(r.events.contains(&Event::Card(false)));
    let mut sink = Vec::new();
    let end = Instant::now() + Duration::from_secs(5);
    while s.pull(&mut sink).is_ok() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(s.pull(&mut sink).is_err(), "the client is told");
    assert_eq!(r.eng.streams(), 0);
}

#[test]
fn there_are_at_most_sixteen_streams() {
    let mut r = Rig::new("many", &[("system.web", play_rights())]);
    let mut keep = Vec::new();
    for _ in 0..MAX_STREAMS {
        keep.push(r.connect("system.web", media()).unwrap());
    }
    assert!(matches!(
        r.connect("system.web", media()),
        Err(ConnectError::Refused(Status::TooMany))
    ));
    drop(keep);
    r.run(100);
    assert_eq!(r.eng.streams(), 0, "streams of programs that left are gone");
    assert!(r.connect("system.web", media()).is_ok());
}

#[path = "engine_mic_tests.rs"]
mod mic;
