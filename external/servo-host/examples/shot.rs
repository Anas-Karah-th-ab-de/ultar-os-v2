//! A page through the whole chain, without a screen: browser side (`RemoteEngine`) → this host
//! (Servo) → the Tor port (`ultar-tor-port`, the same code the browser runs) → the internet.
//! Saves what the browser would show as PNG and prints every engine event. For builders and the
//! integration test of the engine.
//!
//! `shot <ultar-web-engine> <proxy> <address> <out.png> [width height]`
//! (`<proxy>`: the first line `ultar-tor-port` prints)

use std::path::PathBuf;
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
    let work = std::env::temp_dir().join(format!("ultar-shot-{}", std::process::id()));
    std::fs::create_dir_all(work.join("profile"))?;
    let config = HostConfig {
        program: PathBuf::from(program),
        cell: None,
        profile: work.join("profile"),
        downloads: work.clone(),
        user_agent: "UltarWeb/0.8".into(),
        filter: None,
        proxy: Some(proxy.clone()),
    };
    let started = Instant::now();
    let mut engine = RemoteEngine::start(&config)?;
    println!("host ready after {:?}", started.elapsed());
    let tab = engine.create_tab(false)?;
    engine.resize(tab, w, h, 1.0)?;
    engine.set_visible(tab, true)?;
    engine.load(tab, address)?;
    wait_until_drawn(&mut engine, started)?;
    save_png(engine.frame(tab).ok_or("no frame came")?, out)?;
    println!("saved {w}x{h} to {out} after {:?}", started.elapsed());
    Ok(())
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
