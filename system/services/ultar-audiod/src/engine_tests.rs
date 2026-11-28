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
                mode: Some(0o600),
                group: None,
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
    let mut call = Some(
        r.connect(
            "system.calld",
            Hello::new(Class::Call, Direction::Play, 48_000, 2),
        )
        .unwrap(),
    );
    // one recorder reads, one never does
    let mut reader = r
        .connect(
            "system.voiced",
            Hello::new(Class::Media, Direction::Capture, 16_000, 1),
        )
        .unwrap();
    let _sleeper = r
        .connect(
            "system.mesh",
            Hello::new(Class::Media, Direction::Capture, 48_000, 2),
        )
        .unwrap();
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

#[test]
fn the_microphone_goes_to_every_recorder_at_its_own_rate_and_to_the_bus() {
    let mut r = Rig::new(
        "fan",
        &[
            ("system.voiced", mic_rights()),
            ("system.mesh", mic_rights()),
        ],
    );
    let mut a = r
        .connect(
            "system.voiced",
            Hello::new(Class::Media, Direction::Capture, 16_000, 1),
        )
        .unwrap();
    let mut b = r
        .connect(
            "system.mesh",
            Hello::new(Class::Media, Direction::Capture, 48_000, 2),
        )
        .unwrap();
    r.eng.set_bus_capture(vec!["system.voiced".into()]);
    assert!(
        r.events.contains(&Event::Mic(vec![
            "system.mesh".into(),
            "system.voiced".into()
        ])) || {
            r.step();
            r.events
                .iter()
                .any(|e| matches!(e, Event::Mic(u) if u.len() == 2))
        }
    );
    let (mut ga, mut gb) = (Vec::new(), Vec::new());
    let mut n = 0u64;
    for _ in 0..200 {
        // 1 kHz at about -6 dBFS on both channels
        let frame: Vec<i16> = (0..240)
            .flat_map(|i| {
                let s = (0.5
                    * (2.0 * std::f32::consts::PI * 1000.0 * (n + i) as f32 / 48_000.0).sin()
                    * 32768.0) as i16;
                [s, s]
            })
            .collect();
        n += 240;
        r.mic(&frame);
        r.step();
        a.pull(&mut ga).unwrap();
        b.pull(&mut gb).unwrap();
    }
    assert!(
        (ga.len() as i64 - 16_000).abs() < 600,
        "16 kHz mono: {}",
        ga.len()
    );
    assert!(
        (gb.len() as i64 - 96_000).abs() < 1_200,
        "48 kHz stereo: {}",
        gb.len()
    );
    let fa: Vec<f32> = ga[800..].iter().map(|&s| f32::from(s) / 32768.0).collect();
    let fb: Vec<f32> = gb[4800..]
        .iter()
        .step_by(2)
        .map(|&s| f32::from(s) / 32768.0)
        .collect();
    let want = 0.5 / 2f32.sqrt();
    assert!((rms(&fa) - want).abs() < 0.02, "{}", rms(&fa));
    assert!((rms(&fb) - want).abs() < 0.02, "{}", rms(&fb));
    let chunks: Vec<&Vec<i16>> = r
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Chunk(c) = e {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    assert!(
        (chunks.len() as i64 - 50).abs() <= 2,
        "{} chunks of 20 ms for the bus",
        chunks.len()
    );
    assert!(chunks.iter().all(|c| c.len() == CHUNK));
    // everybody stops: the microphone is not in use any more
    drop((a, b));
    r.eng.set_bus_capture(Vec::new());
    r.run(100);
    assert_eq!(
        r.events
            .iter()
            .rev()
            .find_map(|e| if let Event::Mic(u) = e {
                Some(u.clone())
            } else {
                None
            }),
        Some(Vec::new())
    );
}

#[test]
fn a_clip_of_the_bus_is_played_as_a_prompt_and_reported_done() {
    let mut r = Rig::new("clip", &[]);
    let tone: Vec<i16> = (0..1600)
        .map(|i| ((i as f32 * 0.3).sin() * 12_000.0) as i16)
        .collect();
    r.eng.play_clip(7, 16_000, &tone);
    r.run(400);
    assert!(r.events.contains(&Event::ClipDone {
        request: 7,
        ok: true
    }));
    let played = r.played();
    assert!(played.len() > 4_000 && played.iter().any(|s| s.abs() > 8_000));
    assert!(
        (played.len() as i64 - 9_600).abs() < 1_000,
        "100 ms at 48 kHz stereo: {}",
        played.len()
    );
    // no card: the answer is "no"
    let mut n = Rig::without_card("clip2", &[]);
    n.eng.play_clip(8, 16_000, &tone);
    n.step();
    assert!(n.events.contains(&Event::ClipDone {
        request: 8,
        ok: false
    }));
}

#[test]
fn the_latency_of_a_stream_is_inside_the_budget() {
    let mut r = Rig::new("lat", &[("system.web", play_rights())]);
    let mut s = r.connect("system.web", media()).unwrap();
    let mut first_write = None;
    let mut marker_at = 0u64;
    for step in 0..400u64 {
        let v = if step < 200 { 0.1 } else { 0.6 };
        if step == 200 {
            marker_at = step;
        }
        s.push(&level(240, v)).unwrap();
        r.step();
        if first_write.is_none() && !r.card.lock().unwrap().played.is_empty() {
            first_write = Some(step);
        }
    }
    let played = r.played();
    // frame index of the first loud frame; the card plays frame j at start + j / 48000
    let j = played
        .chunks_exact(2)
        .position(|f| f[0] > (0.3 * 32768.0) as i16)
        .unwrap() as f64;
    let start_ms = first_write.unwrap() as f64 * 5.0;
    let latency = start_ms + j / 48.0 - marker_at as f64 * 5.0;
    println!("latency of a media stream in steady state: {latency:.1} ms (cushion 10 ms + queue 10 ms + 5 ms step)");
    assert!(latency <= 40.0, "{latency} ms");
    // and the start of sound after the first byte
    assert!(
        first_write.unwrap() <= 4,
        "sound starts within {} steps",
        first_write.unwrap()
    );
}

#[test]
fn echo_cancellation_through_the_engine_removes_the_speaker_from_the_call_microphone() {
    let mut r = Rig::new(
        "echo",
        &[
            ("system.calld", call_rights()),
            ("system.mesh", mic_rights()),
        ],
    );
    let mut hello = Hello::new(Class::Call, Direction::Duplex, 16_000, 1);
    hello.echo_cancel = true;
    let mut call = r.connect("system.calld", hello).unwrap();
    // a plain recorder hears the same microphone: what the call would hear without it
    let mut plain = r
        .connect(
            "system.mesh",
            Hello::new(Class::Media, Direction::Capture, 16_000, 1),
        )
        .unwrap();
    let (mut with, mut without) = (Vec::new(), Vec::new());
    let mut seed = 5u64;
    let mut far = 0.0f32;
    let mut src: Vec<f32> = Vec::new(); // mono sound the card played, by frame
    let mut done = 0usize;
    let mut mic_at = 0usize;
    for _ in 0..1800 {
        // the far end speaks: 5 ms of speech-like noise at 16 kHz
        let chunk: Vec<i16> = (0..80)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let n = ((seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0;
                far = 0.9 * far + 0.25 * n;
                (far * 32768.0) as i16
            })
            .collect();
        call.push(&chunk).unwrap();
        r.step();
        // the room: what the card played comes back 30 ms and 55 ms later, weaker
        let played = r.played();
        let new = &played[done * 2..];
        src.extend(
            new.chunks_exact(2)
                .map(|f| (f32::from(f[0]) + f32::from(f[1])) / 65536.0),
        );
        done = played.len() / 2;
        let mut mic = Vec::new();
        for _ in 0..240 {
            let at = |back: usize| {
                mic_at
                    .checked_sub(back)
                    .and_then(|i| src.get(i))
                    .copied()
                    .unwrap_or(0.0)
            };
            let e = 0.30 * at(1440) + 0.15 * at(2640);
            let s = (e * 32768.0) as i16;
            mic.extend([s, s]);
            mic_at += 1;
        }
        r.mic(&mic);
        call.pull(&mut with).unwrap();
        plain.pull(&mut without).unwrap();
    }
    let n = with.len().min(without.len());
    let last = |x: &[i16]| -> Vec<f32> {
        x[n - 16_000..n]
            .iter()
            .map(|&s| f32::from(s) / 32768.0)
            .collect()
    };
    let erle = 20.0 * (rms(&last(&without)) / rms(&last(&with)).max(1e-9)).log10();
    println!(
        "ERLE through the engine after 9 s: {erle:.1} dB (echo {:.1} dBFS)",
        20.0 * rms(&last(&without)).log10()
    );
    assert!(rms(&last(&without)) > 0.01, "there is an echo to remove");
    assert!(erle > 20.0, "ERLE {erle} dB");
}

#[test]
fn a_stream_that_runs_dry_gets_a_bigger_cushion_and_never_clicks() {
    let mut r = Rig::new("dry", &[("system.web", play_rights())]);
    let mut s = r.connect("system.web", media()).unwrap();
    // a client that sends 20 ms every 40 ms: it runs dry again and again
    for i in 0..400 {
        if i % 8 == 0 {
            s.push(&level(960, 0.3)).unwrap();
        }
        r.step();
    }
    let played = r.played();
    let left: Vec<f32> = played
        .chunks_exact(2)
        .map(|f| f32::from(f[0]) / 32768.0)
        .collect();
    let jump = left
        .windows(2)
        .fold(0.0f32, |a, w| a.max((w[1] - w[0]).abs()));
    println!("largest step between samples with an irregular client: {jump:.4}");
    assert!(jump < 0.3 / 96.0 * 1.3, "no click: {jump}");
}
