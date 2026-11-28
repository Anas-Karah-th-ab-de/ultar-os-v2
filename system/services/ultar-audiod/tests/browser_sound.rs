//! The sound of a web page in the real engine host reaches the sound server's (fake) card:
//! a page with an `<audio>` element of an 880 Hz tone, played by Servo in the host, written by
//! the host's sink to the browser's stream socket of the real `ultar-audiod`, mixed and written
//! to the fake card's play file, where the test measures the tone.
//!
//! Needs the built engine host and the program that drives it
//! (`external/servo-host`: `cargo build --features media --bin ultar-web-engine --example
//! sound`); runs only when `ULTAR_WEB_ENGINE` and `ULTAR_SOUND_PAGE` name them, otherwise it
//! says `SKIPPED` and does nothing. The second test runs the host in its page cell (root,
//! cgroup v2) and sends the sound through the browser's relay.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.audiod"
register = ["audio"]
publish = ["audio.state", "audio.mic", "audio.mic_state"]
subscribe = ["settings.changed"]
call = ["settings.get"]

[[rule]]
identity = "app.browser"
call = ["audio.play"]
"#;

const IDENTITIES: [&str; 2] = ["system.audiod", "app.browser"];

fn programs() -> Option<(PathBuf, PathBuf)> {
    let engine = std::env::var_os("ULTAR_WEB_ENGINE")?;
    let page = std::env::var_os("ULTAR_SOUND_PAGE")?;
    Some((engine.into(), page.into()))
}

/// Magnitude of the tone `freq` in `samples` (48 kHz), as the amplitude of a pure sine.
fn amplitude(samples: &[f64], freq: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, s) in samples.iter().enumerate() {
        let a = 2.0 * PI * freq * i as f64 / 48_000.0;
        re += s * a.cos();
        im += s * a.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len().max(1) as f64
}

/// The left channel of the card's play file as -1..1.
fn left(path: &Path) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_default();
    bytes
        .chunks_exact(4)
        .map(|f| f64::from(i16::from_le_bytes([f[0], f[1]])) / 32768.0)
        .collect()
}

/// Runs the page program against a sound server and returns what it printed and the card's
/// left channel.
fn play_page(tag: &str, mode: &[&str]) -> Option<(String, Vec<f64>)> {
    let Some((engine, page)) = programs() else {
        println!("SKIPPED: ULTAR_WEB_ENGINE and ULTAR_SOUND_PAGE are not set");
        return None;
    };
    let world = World::new(tag, POLICY, &IDENTITIES);
    let (sockets, card) = (world.subdir("sock"), world.subdir("card"));
    let (play, capture) = (card.join("play.pcm"), card.join("capture.pcm"));
    std::fs::write(&play, b"").unwrap();
    std::fs::write(&capture, b"").unwrap();
    let _daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-audiod"),
        "system.audiod",
        &[
            "--state",
            world.subdir("state").to_str().unwrap(),
            "--root",
            world.subdir("root").to_str().unwrap(),
            "--play-file",
            play.to_str().unwrap(),
            "--capture-file",
            capture.to_str().unwrap(),
            "--policy",
            world.policy_file().to_str().unwrap(),
            "--stream-dir",
            sockets.to_str().unwrap(),
            "--streams",
            "app.browser",
        ],
    );
    let socket = sockets.join("app.browser.sock");
    assert!(wait_until(WAIT, || socket.exists()), "no browser socket");
    let out = Command::new(page)
        .arg(engine)
        .arg(&socket)
        .args(mode)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the page program failed:\n{text}");
    Some((text, left(&play)))
}

/// The loudest 880 Hz amplitude over one-second windows of the card's output.
fn loudest_tone(card: &[f64]) -> (f64, f64) {
    let (mut best, mut peak) = (0.0f64, 0.0f64);
    for window in card.chunks_exact(48_000) {
        best = best.max(amplitude(window, 880.0));
        peak = window.iter().fold(peak, |p, s| p.max(s.abs()));
    }
    (best, peak)
}

fn check(text: &str, card: &[f64]) {
    println!("{text}");
    assert!(
        text.contains("/tone.wav"),
        "the page never asked for its sound:\n{text}"
    );
    let (tone, peak) = loudest_tone(card);
    println!(
        "BROWSER-SOUND: {} s on the card, 880 Hz amplitude {tone:.3}, peak {peak:.3}",
        card.len() / 48_000
    );
    assert!(peak > 0.1, "the card got silence: peak {peak}");
    assert!(tone > 0.1, "the page's tone is not on the card: {tone}");
}

#[test]
fn the_audio_element_of_a_page_reaches_the_card() {
    if let Some((text, card)) = play_page("browser-sound", &[]) {
        check(&text, &card);
    }
}

#[test]
fn the_audio_element_of_a_page_in_its_cell_reaches_the_card_through_the_relay() {
    if let Some((text, card)) = play_page("browser-sound-cell", &["cell"]) {
        check(&text, &card);
    }
}
