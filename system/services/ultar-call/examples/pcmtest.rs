//! Test of the sound card's running streams on the Ultar kernel (`/dev/ultar/pcm`, right `sound`):
//! plays 2 s of a 440 Hz tone through the output ring at the pace of the card and records at the
//! same time. Verdict line: `PCM: ALLES OK` (output taken at the card's pace, input delivered).
//! `pcmtest [NODE] [--output-only]` (`--output-only`: a machine whose card records nothing).

use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output_only = args.iter().any(|a| a == "--output-only");
    let node = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            ultar_hal_system::seat_nodes::pcm_node(std::path::Path::new("/"))
                .to_string_lossy()
                .into_owned()
        });
    let mut f = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0o4000)
        .open(&node)
    {
        Ok(f) => f,
        Err(e) => {
            println!("PCM: FEHLER open {node}: {e}");
            std::process::exit(1);
        }
    };
    if std::fs::OpenOptions::new().read(true).open(&node).is_ok() {
        println!("PCM: FEHLER a second open was allowed");
        std::process::exit(1);
    }
    let tone = tone();
    let (mut written, mut recorded) = (0usize, 0usize);
    let start = Instant::now();
    let mut buf = vec![0u8; 16384];
    while start.elapsed() < Duration::from_secs(2) {
        let at = written % tone.len();
        let end = (at + 7680).min(tone.len());
        if let Ok(n) = f.write(&tone[at..end]) {
            written += n;
        }
        if let Ok(n) = f.read(&mut buf) {
            recorded += n;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let secs = start.elapsed().as_secs_f64();
    let rate = written as f64 / secs / 4.0;
    println!("PCM: written {written} bytes ({rate:.0} frames/s), recorded {recorded} bytes in {secs:.2} s");
    // the card takes 48000 frames/s; the ring lets a writer run 200 ms ahead
    let paced = (40_000.0..56_000.0).contains(&rate);
    let heard = output_only || recorded >= 48_000 * 4;
    if paced && heard {
        println!("PCM: ALLES OK");
    } else {
        println!("PCM: FEHLER paced={paced} recorded={heard}");
        std::process::exit(1);
    }
}

/// 1 s of 440 Hz, 48 kHz 16 bit stereo.
fn tone() -> Vec<u8> {
    (0..48_000usize)
        .flat_map(|i| {
            let s = (8000.0 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()) as i16;
            let b = s.to_le_bytes();
            [b[0], b[1], b[0], b[1]]
        })
        .collect()
}
