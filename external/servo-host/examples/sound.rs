//! The sound of a page through the whole chain, without a screen and without the internet:
//! browser side (`RemoteEngine`) → this host (Servo, its page cell with `cell`) → a page with an
//! `<audio>` element that this program serves itself (as the engine's proxy: the only way out of
//! the cell) → the host's sound sink → the stream socket → the sound server. For the
//! integration test `system/services/ultar-audiod/tests/browser_sound.rs`, which reads what the sound
//! server's (fake) card received.
//!
//! `sound <ultar-web-engine> <sound-server-socket> [cell]`
//!
//! The page is `http://sound.onion/` (onion sites keep plain HTTP, every other one is upgraded to HTTPS) with one `<audio autoplay loop>` of a 880 Hz tone (WAV,
//! served from memory). With `cell` the host runs in its page cell without network: the proxy
//! is a Unix socket and the page's sound goes through the browser's relay
//! (`ultar_web::media::relay`), as in the browser; without it the host connects to the sound
//! server itself. Prints `SOUND-PAGE: <requests the page made>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_web::cell::CellSpec;
use ultar_web::engine::remote::{HostConfig, RemoteEngine};
use ultar_web::engine::WebEngine;

type Failure = Box<dyn std::error::Error>;
type Served = Arc<Mutex<Vec<String>>>;

/// Frequency of the page's tone.
const TONE_HZ: f64 = 880.0;
/// How long the page plays before this program ends (seconds).
const PLAY_SECONDS: u64 = 25;

const PAGE: &str = "<!doctype html><title>sound</title><body>sound test\
<audio id=a autoplay loop src=\"/tone.wav\"></audio>\
<script>var a=document.getElementById('a');a.loop=true;a.play();</script></body>";

/// A WAV file of five seconds of the tone: 22.05 kHz, mono, 16 bit, half scale.
fn tone_wav() -> Vec<u8> {
    let rate = 22_050u32;
    let samples: Vec<u8> = (0..rate as usize * 5)
        .flat_map(|i| {
            let s = (0.5
                * (2.0 * std::f64::consts::PI * TONE_HZ * i as f64 / f64::from(rate)).sin()
                * 32767.0) as i16;
            s.to_le_bytes()
        })
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

/// Answers the requests of one connection: a `CONNECT` is accepted (what follows is plain
/// HTTP for the same page server), a `GET` gets the page or the tone, whatever the host name.
fn serve(stream: impl Read + Write + Send + 'static, served: Served, wav: Arc<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        loop {
            let mut head = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let end = line == "\r\n";
                head.push_str(&line);
                line.clear();
                if end {
                    break;
                }
            }
            let Some(first) = head.lines().next().map(str::to_owned) else {
                return;
            };
            let target = first.split(' ').nth(1).unwrap_or("/").to_owned();
            let out = reader.get_mut();
            if first.starts_with("CONNECT") {
                served.lock().map(|mut s| s.push(first)).ok();
                if out
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .is_err()
                {
                    return;
                }
                continue;
            }
            let path = target
                .split("://")
                .last()
                .and_then(|r| r.find('/').map(|i| &r[i..]));
            let path = path.unwrap_or(&target).to_owned();
            served
                .lock()
                .map(|mut s| s.push(format!("GET {path}")))
                .ok();
            let (kind, body): (&str, &[u8]) = if path.starts_with("/tone.wav") {
                ("audio/wav", &wav)
            } else {
                ("text/html", PAGE.as_bytes())
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nAccept-Ranges: none\r\nConnection: keep-alive\r\n\r\n",
                body.len()
            );
            if out.write_all(reply.as_bytes()).is_err() || out.write_all(body).is_err() {
                return;
            }
        }
    });
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, upstream, rest @ ..] = args.as_slice() else {
        return Err("usage: sound <engine> <sound-server-socket> [cell]".into());
    };
    let in_cell = rest.first().is_some_and(|a| a == "cell");
    let work = std::env::temp_dir().join(format!("ultar-sound-{}", std::process::id()));
    let engine_dir = work.join("engine");
    let downloads = work.join("downloads");
    std::fs::create_dir_all(&engine_dir)?;
    std::fs::create_dir_all(&downloads)?;
    let (served, wav) = (Served::default(), Arc::new(tone_wav()));
    let mut config = HostConfig {
        program: PathBuf::from(program),
        cell: None,
        profile: engine_dir.clone(),
        downloads: downloads.clone(),
        user_agent: "UltarWeb/0.8".into(),
        filter: None,
        proxy: None,
        proxy_socket: None,
        audio_socket: Some(PathBuf::from(upstream)),
    };
    if in_cell {
        let socket = ultar_web::cell::proxy_socket(&engine_dir);
        let listener = UnixListener::bind(&socket)?;
        let (s, w) = (served.clone(), wav.clone());
        std::thread::spawn(move || {
            for c in listener.incoming().flatten() {
                serve(c, s.clone(), w.clone());
            }
        });
        let sound = ultar_web::cell::audio_socket(&engine_dir);
        ultar_web::media::relay::spawn(&sound, PathBuf::from(upstream))?;
        into_cell(&mut config, socket, &downloads)?;
        config.audio_socket = Some(sound);
    } else {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        config.proxy = Some(format!(
            "http://127.0.0.1:{}",
            listener.local_addr()?.port()
        ));
        let (s, w) = (served.clone(), wav.clone());
        std::thread::spawn(move || {
            for c in listener.incoming().flatten() {
                serve(c, s.clone(), w.clone());
            }
        });
    }
    let started = Instant::now();
    let mut engine = RemoteEngine::start(&config)?;
    println!("host ready after {:?}", started.elapsed());
    open_page(&mut engine)?;
    watch(&mut engine, started)?;
    let seen = served.lock().map(|s| s.clone()).unwrap_or_default();
    println!("SOUND-PAGE: {seen:?}");
    Ok(())
}

/// Opens a visible tab with the sound page.
fn open_page(engine: &mut RemoteEngine) -> Result<(), Failure> {
    let tab = engine.create_tab(false)?;
    engine.resize(tab, 640, 400, 1.0)?;
    engine.set_visible(tab, true)?;
    engine.load(tab, "http://sound.onion/")?;
    Ok(())
}

/// Prints the events of the host while the page plays.
fn watch(engine: &mut RemoteEngine, started: Instant) -> Result<(), Failure> {
    let end = Instant::now() + Duration::from_secs(PLAY_SECONDS);
    while Instant::now() < end {
        for event in engine.poll(Some(Duration::from_millis(500)))? {
            println!("{:>6.1}s {event:?}", started.elapsed().as_secs_f32());
        }
    }
    Ok(())
}

/// Runs the host in its page cell without network: the socket is the only way out.
fn into_cell(config: &mut HostConfig, socket: PathBuf, downloads: &Path) -> Result<(), Failure> {
    let engine_dir = socket.parent().ok_or("socket has no folder")?.to_path_buf();
    let read_only = std::env::var("ULTAR_SHOT_CELL_RO")
        .unwrap_or_else(|_| "/usr:/lib:/lib64:/bin:/etc/ssl:/etc/fonts".into())
        .split(':')
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    let spec = CellSpec {
        network: false,
        read_only,
        read_write: vec![engine_dir.clone(), downloads.to_path_buf()],
        memory_mb: 2048,
    };
    spec.validate()?;
    config.cell = Some(spec);
    config.profile = engine_dir;
    config.proxy_socket = Some(socket);
    println!("host in its page cell (no network)");
    Ok(())
}
