//! A page through the whole chain, without a screen: browser side (`RemoteEngine`) → this host
//! (Servo) → the Tor port (`ultar-tor-port`, the same code the browser runs) → the internet.
//! Saves what the browser would show as PNG and prints every engine event. For builders and the
//! integration test of the engine.
//!
//! `shot <ultar-web-engine> <proxy> <address> <out.png> [width height]`
//! (`<proxy>`: the first line `ultar-tor-port` prints, or `tor:<path of ultar-tor-port>`: then
//! this program starts the port itself and stops it at the end – for a machine that can only
//! run one program, like the Ultar kernel's self test, tools/kern-test/servo-run.sh)
//!
//! After the picture it prints `SHOT-PIXELS: <pixels that are not the background colour> of
//! <all>` (the most frequent colour counts as background): a blank page has none. With the
//! variable `SHOT_SERIAL_PNG=1` the PNG follows as base64 lines between `PNG-BEGIN` and
//! `PNG-END`, for a machine whose only way out is the serial console.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};
use ultar_web::engine::remote::{HostConfig, RemoteEngine};
use ultar_web::engine::{EngineEvent, FrameView, WebEngine};

type Failure = Box<dyn std::error::Error>;

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, proxy, address, out, rest @ ..] = args.as_slice() else {
        return Err("usage: shot <engine> <proxy> <address> <out.png> [width height]".into());
    };
    let (w, h) = match rest {
        [w, h] => (w.parse()?, h.parse()?),
        _ => (1280u32, 800u32),
    };
    // the Tor port, if we are to start it: it runs as long as its standard input stays open
    let port = match proxy.strip_prefix("tor:") {
        Some(path) => Some(TorPort::start(path)?),
        None => None,
    };
    let proxy = port.as_ref().map_or(proxy.clone(), |p| p.proxy.clone());
    let work = std::env::temp_dir().join(format!("ultar-shot-{}", std::process::id()));
    std::fs::create_dir_all(work.join("profile"))?;
    let config = HostConfig {
        program: PathBuf::from(program),
        cell: None,
        profile: work.join("profile"),
        downloads: work.clone(),
        user_agent: "UltarWeb/0.8".into(),
        filter: None,
        proxy: Some(proxy),
    };
    let started = Instant::now();
    let mut engine = RemoteEngine::start(&config)?;
    println!("host ready after {:?}", started.elapsed());
    let tab = engine.create_tab(false)?;
    engine.resize(tab, w, h, 1.0)?;
    engine.set_visible(tab, true)?;
    engine.load(tab, address)?;
    wait_until_drawn(&mut engine, started)?;
    let frame = engine.frame(tab).ok_or("no frame came")?;
    let drawn = count_non_background(
        frame.pixels,
        frame.width as usize,
        frame.height as usize,
        frame.stride,
    );
    let all = frame.width as usize * frame.height as usize;
    save_png(frame, out)?;
    println!("saved {w}x{h} to {out} after {:?}", started.elapsed());
    println!("SHOT-PIXELS: {drawn} of {all}");
    if std::env::var_os("SHOT_SERIAL_PNG").is_some() {
        print_base64(&std::fs::read(out)?);
    }
    drop(port);
    Ok(())
}

/// The Tor port as a child process of this program.
struct TorPort {
    child: Child,
    /// Held open: the port ends when this closes.
    _stdin: ChildStdin,
    proxy: String,
}

impl TorPort {
    fn start(path: &str) -> Result<TorPort, Failure> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("no standard input of the Tor port")?;
        let out = child
            .stdout
            .take()
            .ok_or("no standard output of the Tor port")?;
        let mut line = String::new();
        BufReader::new(out).read_line(&mut line)?;
        let proxy = line.trim().to_string();
        if proxy.is_empty() {
            return Err("the Tor port printed no address".into());
        }
        println!("Tor port at {proxy}");
        Ok(TorPort {
            child,
            _stdin: stdin,
            proxy,
        })
    }
}

impl Drop for TorPort {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How many pixels of the frame (BGRX rows) differ from the most frequent colour, which counts
/// as the background. A blank page is 0.
fn count_non_background(pixels: &[u8], width: usize, height: usize, stride: usize) -> usize {
    let mut histogram: HashMap<[u8; 3], usize> = HashMap::new();
    for y in 0..height {
        let Some(row) = pixels.get(y * stride..y * stride + width * 4) else {
            break;
        };
        for px in row.chunks_exact(4) {
            *histogram.entry([px[0], px[1], px[2]]).or_default() += 1;
        }
    }
    let total: usize = histogram.values().sum();
    total - histogram.values().max().copied().unwrap_or(0)
}

/// Standard base64, 76 characters per line, between `PNG-BEGIN` and `PNG-END`.
fn print_base64(bytes: &[u8]) {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    println!("PNG-BEGIN {}", bytes.len());
    let mut text = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    for line in text.as_bytes().chunks(76) {
        println!("{}", String::from_utf8_lossy(line));
    }
    println!("PNG-END");
}

/// Prints the events until the page loaded, a frame came after that and three seconds passed
/// without a new one (or ten seconds after loading without any new frame; at most 4 minutes).
fn wait_until_drawn(engine: &mut RemoteEngine, started: Instant) -> Result<(), Failure> {
    let (mut loaded, mut last_frame) = (None, None);
    let deadline = Instant::now() + Duration::from_secs(240);
    while Instant::now() < deadline {
        for event in engine.poll(Some(Duration::from_millis(500)))? {
            match &event {
                EngineEvent::Frame { .. } => last_frame = Some(Instant::now()),
                EngineEvent::Progress { loading: false, .. } => loaded = Some(Instant::now()),
                _ => {}
            }
            println!("{:>7.1}s {event:?}", started.elapsed().as_secs_f32());
        }
        let Some(done) = loaded else { continue };
        let drawn = last_frame.is_some_and(|t| t >= done && t.elapsed() > Duration::from_secs(3));
        if drawn || done.elapsed() > Duration::from_secs(10) {
            return Ok(());
        }
    }
    Ok(())
}

/// The frame (BGRX rows) as an RGB PNG.
fn save_png(frame: FrameView<'_>, out: &str) -> Result<(), Failure> {
    let mut rgb = Vec::with_capacity((frame.width * frame.height * 3) as usize);
    for y in 0..frame.height as usize {
        let row = &frame.pixels[y * frame.stride..y * frame.stride + frame.width as usize * 4];
        for px in row.chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut png = png::Encoder::new(file, frame.width, frame.height);
    png.set_color(png::ColorType::Rgb);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(height: usize, stride: usize, paint: &[(usize, usize, [u8; 3])]) -> Vec<u8> {
        // white background (BGRX)
        let mut px = vec![255u8; stride * height];
        for p in px.chunks_exact_mut(4) {
            p[3] = 0;
        }
        for (x, y, c) in paint {
            let at = y * stride + x * 4;
            px[at..at + 3].copy_from_slice(c);
        }
        px
    }

    #[test]
    fn a_blank_frame_has_no_pixel_that_differs_from_the_background() {
        let px = frame(8, 48, &[]);
        assert_eq!(count_non_background(&px, 10, 8, 48), 0);
    }

    #[test]
    fn drawn_pixels_are_counted_whatever_the_background_is() {
        let paint: Vec<_> = (0..5).map(|x| (x, 2, [0u8, 0, 0])).collect();
        let px = frame(8, 48, &paint);
        assert_eq!(count_non_background(&px, 10, 8, 48), 5);
        // a dark page with a few light pixels: the dark colour is the background
        let mut dark = frame(8, 48, &[]);
        for p in dark.chunks_exact_mut(4) {
            p[..3].copy_from_slice(&[10, 10, 10]);
        }
        dark[0..3].copy_from_slice(&[200, 200, 200]);
        assert_eq!(count_non_background(&dark, 10, 8, 48), 1);
    }

    #[test]
    fn padding_at_the_end_of_a_row_is_not_looked_at() {
        let mut px = frame(8, 48, &[]);
        // bytes 40..48 of every row are padding (stride 48, width 10)
        for y in 0..8 {
            px[y * 48 + 40..y * 48 + 48].fill(7);
        }
        assert_eq!(count_non_background(&px, 10, 8, 48), 0);
    }

    #[test]
    fn a_short_buffer_does_not_panic() {
        assert_eq!(count_non_background(&[1, 2, 3, 4], 10, 8, 48), 0);
    }
}
