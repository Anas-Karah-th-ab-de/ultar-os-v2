use super::*;
use crate::e2e::crypto::{keys, Ephemeral, Role, Transcript, CALL_ID};
use std::sync::mpsc;
use std::sync::Mutex;

/// A microphone that says one tone and a speaker that records.
struct Tone {
    hz: f32,
    n: usize,
    heard: Arc<Mutex<Vec<i16>>>,
}

impl AudioIo for Tone {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let f = (0..FRAME)
            .map(|i| {
                let t = (self.n + i) as f32 / audio::RATE as f32;
                (6000.0 * (t * self.hz * std::f32::consts::TAU).sin()) as i16
            })
            .collect();
        self.n += FRAME;
        Some(f)
    }
    fn play(&mut self, frame: &[i16]) {
        self.heard.lock().unwrap().extend_from_slice(frame);
    }
}

struct Screen {
    tick: u32,
    shown: Arc<Mutex<u32>>,
}

impl VideoIo for Screen {
    fn capture(&mut self) -> Option<Picture> {
        self.tick += 1;
        Some(video::bars(self.tick))
    }
    fn show(&mut self, p: Picture) {
        assert_eq!((p.width, p.height), video::SIZE);
        *self.shown.lock().unwrap() += 1;
    }
}

fn pair() -> (MediaKeys, MediaKeys) {
    let (ea, eb) = (Ephemeral::new(), Ephemeral::new());
    let t = Transcript {
        call: [5; CALL_ID],
        ea: ea.public,
        eb: eb.public,
        da: [1; 32],
        db: [2; 32],
    };
    let a = keys(ea, &t.eb, &t, Role::Caller).unwrap().media.unwrap();
    let b = keys(eb, &t.ea, &t, Role::Callee).unwrap().media.unwrap();
    (a, b)
}

fn local(net: &Net) -> Vec<SocketAddr> {
    let port = net.candidates()[0].port();
    vec![SocketAddr::from(([127, 0, 0, 1], port))]
}

/// Strength of a tone in recorded samples (Goertzel).
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
fn two_devices_find_each_other_and_hear_and_see_each_other() {
    let (ka, kb) = pair();
    let (na, nb) = (Net::open(None).unwrap(), Net::open(None).unwrap());
    let (pa, pb) = (local(&nb), local(&na));
    let stop = Arc::new(AtomicBool::new(false));
    let heard_b = Arc::new(Mutex::new(Vec::new()));
    let shown_b = Arc::new(Mutex::new(0));
    let (tx, _rx) = mpsc::channel();
    let threads: Vec<_> = [
        (
            na,
            ka,
            pa,
            440.0,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(0)),
        ),
        (nb, kb, pb, 300.0, heard_b.clone(), shown_b.clone()),
    ]
    .into_iter()
    .map(|(net, k, peer, hz, heard, shown)| {
        let (stop, tx) = (stop.clone(), tx.clone());
        std::thread::spawn(move || {
            let mut m = Media::new(net, k, peer, Arc::new(AtomicBool::new(false)));
            assert!(m.find_way(&stop), "no way found");
            assert!(!m.relayed());
            let mut mic = Tone { hz, n: 0, heard };
            let mut cam = Screen { tick: 0, shown };
            m.run(&mut mic, Some(&mut cam), &stop, &tx);
            m.close();
        })
    })
    .collect();
    std::thread::sleep(Duration::from_millis(2500));
    stop.store(true, Ordering::Relaxed);
    for t in threads {
        t.join().unwrap();
    }
    let heard = heard_b.lock().unwrap().clone();
    let tail = &heard[heard.len() / 2..];
    assert!(
        power(tail, 440.0) > 50.0 * power(tail, 300.0),
        "B hears A's 440 Hz"
    );
    assert!(
        *shown_b.lock().unwrap() >= 5,
        "B saw {} pictures",
        shown_b.lock().unwrap()
    );
}

#[test]
fn packets_of_another_call_are_ignored_and_no_way_means_no_call() {
    let (ka, _) = pair();
    let (_, other) = pair();
    let net = Net::open(None).unwrap();
    let mut m = Media::new(
        net,
        ka,
        vec![SocketAddr::from(([127, 0, 0, 1], 9))],
        Arc::new(AtomicBool::new(false)),
    );
    let mut stranger = other;
    let p = [vec![VOICE], stranger.out.seal(&[VOICE], b"x")].concat();
    assert!(m.open(&p).is_none());
    let stop = AtomicBool::new(true);
    assert!(!m.find_way(&stop), "stopped before a way answered");
}
