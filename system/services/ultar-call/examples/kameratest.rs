//! Test of the camera node on the Ultar kernel (`/dev/ultar/kamera`, right `camera`): reads the
//! camera's pictures for 8 s without waiting, puts in the Huffman tables, decodes them and scales
//! them to the call size. Verdict lines: `KAMERA: ALLES OK` (at least 5 pictures decoded, none
//! broken),
//! `KAMERA: KEINE KAMERA` (no camera: the node is absent), else `KAMERA: FEHLER …`.
//! `kameratest [NODE]`.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};
use ultar_call::camera::from_camera;

fn main() {
    let node = std::env::args().nth(1).unwrap_or_else(|| {
        ultar_hal_system::seat_nodes::camera_node(std::path::Path::new("/"))
            .to_string_lossy()
            .into_owned()
    });
    let mut f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&node)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("KAMERA: KEINE KAMERA ({node})");
            return;
        }
        Err(e) => fail(&format!("open {node}: {e}")),
    };
    if std::fs::OpenOptions::new().read(true).open(&node).is_ok() {
        fail("a second open was allowed");
    }
    let secs = 8;
    let (good, bad, bytes, first) = read_for(&mut f, Duration::from_secs(secs));
    let fps = f64::from(good) / secs as f64;
    println!(
        "KAMERA: {good} pictures decoded ({fps:.1}/s), {bad} not, {bytes} bytes; first {first:?}"
    );
    if good >= 5 && bad == 0 {
        println!("KAMERA: ALLES OK");
    } else {
        fail("too few pictures");
    }
}

/// Width, height and mean colour of a picture.
type Look = (u16, u16, [u8; 3]);

/// Reads for `time`: decoded and broken pictures, bytes, size and mean colour of the first.
fn read_for(f: &mut std::fs::File, time: Duration) -> (u32, u32, usize, Option<Look>) {
    let mut buf = vec![0u8; 1 << 20];
    let (mut good, mut bad, mut bytes, mut first) = (0, 0, 0, None);
    let start = Instant::now();
    while start.elapsed() < time {
        match f.read(&mut buf) {
            Ok(n) => {
                bytes += n;
                match from_camera(&buf[..n]) {
                    Some(p) => {
                        good += 1;
                        first = first.or(Some((p.width, p.height, mean(&p.rgb))));
                    }
                    None => bad += 1,
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => fail(&format!("read: {e}")),
        }
    }
    (good, bad, bytes, first)
}

fn mean(rgb: &[u8]) -> [u8; 3] {
    let mut s = [0u64; 3];
    for p in rgb.chunks_exact(3) {
        for i in 0..3 {
            s[i] += u64::from(p[i]);
        }
    }
    let n = (rgb.len() / 3).max(1) as u64;
    [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
}

fn fail(why: &str) -> ! {
    println!("KAMERA: FEHLER {why}");
    std::process::exit(1);
}
