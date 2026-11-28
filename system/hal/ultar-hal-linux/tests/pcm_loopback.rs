//! The PCM streams against the kernel's `snd-aloop` loopback card: what is played to the
//! playback side comes out of the capture side. Skipped unless `ULTAR_ALSA_LOOP` names the
//! card's number (`sudo modprobe snd-aloop` first, `rmmod snd-aloop` afterwards).

use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_hal_linux::sound::pcm::{Direction, Pcm};

const RATE: u32 = 48_000;

fn node(card: &str, device: u32, kind: char) -> PathBuf {
    PathBuf::from(format!("/dev/snd/pcmC{card}D{device}{kind}"))
}

/// Interleaved stereo, `frames` long, 880 Hz at half scale (`start` is the first frame number).
fn tone(start: usize, frames: usize) -> Vec<i16> {
    (start..start + frames)
        .flat_map(|n| {
            let t = n as f32 / RATE as f32;
            let v = (0.5 * (2.0 * std::f32::consts::PI * 880.0 * t).sin() * 32767.0) as i16;
            [v, v]
        })
        .collect()
}

fn peak(samples: &[i16]) -> i32 {
    samples
        .iter()
        .map(|s| i32::from(*s).abs())
        .max()
        .unwrap_or(0)
}

#[test]
fn what_is_played_comes_out_of_the_capture_side() {
    let Ok(card) = std::env::var("ULTAR_ALSA_LOOP") else {
        eprintln!("skipped: ULTAR_ALSA_LOOP is not set");
        return;
    };
    let mut capture = Pcm::open(&node(&card, 1, 'c'), Direction::Capture, RATE).unwrap();
    let mut play = Pcm::open(&node(&card, 0, 'p'), Direction::Playback, RATE).unwrap();
    let (mut sent, mut heard) = (0usize, Vec::new());
    let end = Instant::now() + Duration::from_millis(1500);
    let mut buf = vec![0i16; 960];
    while Instant::now() < end {
        let chunk = tone(sent / 2, 480);
        sent += play.write(&chunk).unwrap();
        let n = capture.read(&mut buf).unwrap();
        heard.extend_from_slice(&buf[..n]);
        std::thread::sleep(Duration::from_millis(2));
    }
    // the first periods may be silence while the cable starts; the tone is in the rest
    let tail = &heard[heard.len() / 2..];
    let p = peak(tail);
    eprintln!(
        "PCM-LOOP: sent {} frames, heard {} frames, peak {p}, xruns {}/{}",
        sent / 2,
        heard.len() / 2,
        play.xruns(),
        capture.xruns()
    );
    assert!(heard.len() / 2 > RATE as usize / 2, "heard too little");
    assert!((15_800..=16_900).contains(&p), "peak {p}");
    // the frequency: 880 Hz crosses zero upwards 880 times a second
    let mono: Vec<i16> = tail.chunks_exact(2).map(|f| f[0]).collect();
    let rises = mono.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
    let seconds = mono.len() as f32 / RATE as f32;
    let hz = rises as f32 / seconds;
    eprintln!("PCM-LOOP: {hz:.0} Hz");
    assert!((hz - 880.0).abs() < 25.0, "{hz} Hz");
}

#[test]
fn an_underrun_is_recovered_and_counted() {
    let Ok(card) = std::env::var("ULTAR_ALSA_LOOP") else {
        eprintln!("skipped: ULTAR_ALSA_LOOP is not set");
        return;
    };
    let mut capture = Pcm::open(&node(&card, 1, 'c'), Direction::Capture, RATE).unwrap();
    let mut play = Pcm::open(&node(&card, 0, 'p'), Direction::Playback, RATE).unwrap();
    let mut buf = vec![0i16; 960];
    assert!(play.write(&tone(0, 480)).unwrap() > 0);
    // nobody feeds the stream: it runs dry
    let end = Instant::now() + Duration::from_millis(400);
    while Instant::now() < end {
        let _ = capture.read(&mut buf).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = play.write(&tone(480, 480)).unwrap();
    eprintln!(
        "PCM-LOOP: after the underrun the stream took {took} samples, xruns {}",
        play.xruns()
    );
    assert!(took > 0, "the stream did not recover");
    assert!(play.xruns() >= 1);
    // and the flush empties the ring
    play.flush().unwrap();
    assert_eq!(play.queued().unwrap(), 0);
}
