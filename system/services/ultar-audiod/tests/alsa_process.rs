//! The real `ultar-audiod` on a real ALSA card: the kernel's `snd-aloop` loopback. What a
//! program plays through a stream socket leaves the server through `--alsa-play` and is heard
//! on the other end of the loopback; what is played into the loopback's other side is the
//! microphone, `--alsa-capture`, and reaches a recording client.
//!
//! Runs only when `ULTAR_ALSA_LOOP` names the loopback card's number (`modprobe snd-aloop`
//! first, `rmmod snd_aloop` afterwards); otherwise it says `SKIPPED` and does nothing.

use std::f64::consts::PI;
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_audio_stream::{Class, Direction, Hello, Stream};
use ultar_hal_linux::sound::pcm::{Direction as PcmDirection, Pcm};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.audiod"
register = ["audio"]
publish = ["audio.state", "audio.mic", "audio.mic_state"]
subscribe = ["settings.changed"]
call = ["settings.get"]

[[rule]]
identity = "system.mediad"
call = ["audio.play"]

[[rule]]
identity = "system.voiced"
call = ["audio.capture", "audio.play"]
subscribe = ["audio.mic"]
"#;

const IDENTITIES: [&str; 3] = ["system.audiod", "system.mediad", "system.voiced"];
const RATE: u32 = 48_000;

fn amplitude(samples: &[f64], freq: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, s) in samples.iter().enumerate() {
        let a = 2.0 * PI * freq * i as f64 / f64::from(RATE);
        re += s * a.cos();
        im += s * a.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len().max(1) as f64
}

/// Interleaved stereo of a 1 kHz tone at half scale, `frames` frames from frame `start`.
fn tone(start: usize, frames: usize) -> Vec<i16> {
    (start..start + frames)
        .flat_map(|n| {
            let v = (0.5 * (2.0 * PI * 1000.0 * n as f64 / f64::from(RATE)).sin() * 32767.0) as i16;
            [v, v]
        })
        .collect()
}

fn left(stereo: &[i16]) -> Vec<f64> {
    stereo
        .chunks_exact(2)
        .map(|f| f64::from(f[0]) / 32768.0)
        .collect()
}

fn node(card: &str, device: u32, kind: char) -> String {
    format!("/dev/snd/pcmC{card}D{device}{kind}")
}

/// The command line of the server on `card`: speaker `D0p`, microphone `D0c`.
fn daemon_args(world: &World, card: &str, sockets: &Path) -> Vec<String> {
    let path = |p: &Path| p.to_str().unwrap().to_owned();
    vec![
        "--state".into(),
        path(&world.subdir("state")),
        "--root".into(),
        path(&world.subdir("root")),
        "--alsa-play".into(),
        node(card, 0, 'p'),
        "--alsa-capture".into(),
        node(card, 0, 'c'),
        "--policy".into(),
        path(&world.policy_file()),
        "--stream-dir".into(),
        path(sockets),
        "--streams".into(),
        "system.mediad,system.voiced".into(),
    ]
}

fn open(card: &str, device: u32, kind: char, direction: PcmDirection) -> Pcm {
    Pcm::open(Path::new(&node(card, device, kind)), direction, RATE).unwrap()
}

/// Plays a tone into the server for `seconds` and into the microphone side, and collects what
/// leaves the speaker side and what the recording client gets.
fn exchange(
    (player, recorder): (&mut Stream, &mut Stream),
    (speaker, mic): (&mut Pcm, &mut Pcm),
    seconds: u64,
) -> (Vec<i16>, Vec<i16>) {
    let (mut sent, mut sent_mic) = (0usize, 0usize);
    let (mut heard, mut recorded, mut buf) = (Vec::new(), Vec::new(), vec![0i16; 960]);
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        sent += player.push(&tone(sent, 480)).unwrap() / 2;
        sent_mic += mic.write(&tone(sent_mic, 480)).unwrap() / 2;
        let n = speaker.read(&mut buf).unwrap();
        heard.extend_from_slice(&buf[..n]);
        recorder.pull(&mut recorded).unwrap();
        std::thread::sleep(Duration::from_millis(3));
    }
    (heard, recorded)
}

#[test]
fn the_server_plays_and_records_through_a_real_alsa_card() {
    let Ok(card) = std::env::var("ULTAR_ALSA_LOOP") else {
        println!("SKIPPED: ULTAR_ALSA_LOOP is not set");
        return;
    };
    let world = World::new("alsa", POLICY, &IDENTITIES);
    let sockets = world.subdir("sock");
    let args = daemon_args(&world, &card, &sockets);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let _daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-audiod"), "system.audiod", &args);
    let (media, voiced) = (
        sockets.join("system.mediad.sock"),
        sockets.join("system.voiced.sock"),
    );
    assert!(wait_until(WAIT, || media.exists() && voiced.exists()));
    // the ends of the loopback the server does not use: the speaker's output, the microphone's input
    let mut speaker = open(&card, 1, 'c', PcmDirection::Capture);
    let mut mic = open(&card, 1, 'p', PcmDirection::Playback);
    let play = Hello::new(Class::Media, Direction::Play, RATE, 2);
    let record = Hello::new(Class::Media, Direction::Capture, RATE, 2);
    let mut player = Stream::connect(&media, play).expect("play stream");
    let mut recorder = Stream::connect(&voiced, record).expect("capture stream");
    let (heard, recorded) = exchange((&mut player, &mut recorder), (&mut speaker, &mut mic), 3);
    let (out, inn) = (
        left(&heard[heard.len() / 2..]),
        left(&recorded[recorded.len() / 2..]),
    );
    let (a_out, a_in) = (amplitude(&out, 1000.0), amplitude(&inn, 1000.0));
    println!(
        "ALSA-AUDIOD: speaker {} s, 1 kHz amplitude {a_out:.3}; microphone {} s, amplitude {a_in:.3}",
        heard.len() / 2 / RATE as usize,
        recorded.len() / 2 / RATE as usize
    );
    assert!(a_out > 0.3, "the played tone is not on the card: {a_out}");
    assert!(
        a_in > 0.3,
        "the microphone tone did not reach the client: {a_in}"
    );
}
