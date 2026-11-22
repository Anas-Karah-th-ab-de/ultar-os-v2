//! ultar-face – Gesichtsentsperrung (Frontkamera über /usr/bin/ultar-cam, Modelle unter /usr/lib/ultar/face).
//!
//!   ultar-face anlernen [sek]   Gesicht aufnehmen (mehrere Bilder), speichert /data/ultar/face/gesicht.bin
//!   ultar-face pruefen [sek]    vergleicht mit dem gespeicherten Gesicht
//!   ultar-face loeschen         gespeichertes Gesicht entfernen
//!   ultar-face status           "angelernt n" / "leer"
//!   ultar-face bild <a.ppm> [b.ppm]   Test: Gesichter im Bild, ggf. Ähnlichkeit zweier Bilder
//!
//! Ausgabe zeilenweise (für die Oberfläche): "kamera", "gesicht <score>", "kein-gesicht", "dunkel",
//! "fortschritt n/m", "ok <ähnlichkeit>", "nein <ähnlichkeit>", "fertig", "zeit", "fehler <text>".
//! Rückgabe: 0 = erkannt/fertig, 1 = nicht erkannt, 2 = Fehler, 3 = Zeit abgelaufen.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use ultar_face::*;

const DET: &str = "face_detection_yunet_2023mar.onnx";
const REC: &str = "face_recognition_sface_2021dec.onnx";
const STORE_DIR: &str = "/data/ultar/face";
const STORE: &str = "/data/ultar/face/gesicht.bin";
/// Schwelle für „gleiche Person" (OpenCV empfiehlt 0.363; etwas strenger)
const THRESH: f32 = 0.42;
const SAMPLES: usize = 6;

fn model_dir() -> String {
    std::env::var("ULTAR_FACE_MODELS").unwrap_or_else(|_| {
        for d in ["/usr/lib/ultar/face", "/data/ultar/face/models"] {
            if std::path::Path::new(&format!("{d}/{DET}")).exists() {
                return d.into();
            }
        }
        "/usr/lib/ultar/face".into()
    })
}

fn engine() -> Engine {
    let d = model_dir();
    match Engine::load(&format!("{d}/{DET}"), &format!("{d}/{REC}"), std::env::var("FACE_DET").ok().and_then(|s| s.parse().ok()).unwrap_or(640)) {
        Ok(e) => e,
        Err(e) => {
            say(&format!("fehler modelle: {e}"));
            std::process::exit(2);
        }
    }
}

fn say(s: &str) {
    let mut o = std::io::stdout();
    let _ = writeln!(o, "{s}");
    let _ = o.flush();
}

fn read_ppm(p: &str) -> Img {
    let b = std::fs::read(p).expect("Bild lesen");
    // P6 <w> <h> <max>\n<rgb>
    let mut fields = Vec::new();
    let mut i = 2;
    while fields.len() < 3 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&b[s..i]).unwrap().parse::<usize>().unwrap());
    }
    i += 1;
    let (w, h) = (fields[0], fields[1]);
    let mut img = Img::new(w, h);
    for k in 0..w * h {
        let p = &b[i + k * 3..];
        img.px[k] = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
    }
    img
}

/// Frontkamera als Bildstrom ("UCF1", b, h, nr, Pixel 0x00RRGGBB).
struct Cam {
    child: std::process::Child,
    out: std::process::ChildStdout,
}

impl Cam {
    fn open() -> Option<Cam> {
        let bin = std::env::var("ULTAR_CAM").unwrap_or("/usr/bin/ultar-cam".into());
        let mut child = Command::new(bin).args(["stream", "front"]).env("UCAM_STEP", "3").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let out = child.stdout.take()?;
        Some(Cam { child, out })
    }
    fn frame(&mut self) -> Option<Img> {
        let mut hd = [0u8; 16];
        self.out.read_exact(&mut hd).ok()?;
        let u = |o: usize| u32::from_le_bytes(hd[o..o + 4].try_into().unwrap()) as usize;
        let (w, h) = (u(4), u(8));
        if &hd[..4] != b"UCF1" || w == 0 || h == 0 || w * h > 20_000_000 {
            return None;
        }
        let mut b = vec![0u8; w * h * 4];
        self.out.read_exact(&mut b).ok()?;
        Some(Img { w, h, px: b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() })
    }
}

impl Drop for Cam {
    fn drop(&mut self) {
        // Helfer schaltet Sensor und Bildkette bei 'q' bzw. Ende der Eingabe ab
        if let Some(mut i) = self.child.stdin.take() {
            let _ = i.write_all(b"q");
        }
        for _ in 0..40 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Frontsensor ist (Samsung) um 270° gedreht eingebaut; sonst andere Lagen probieren.
const ROTS: [u8; 4] = [3, 1, 0, 2];

/// Bestes Gesicht im Bild (größtes mit gutem Score), samt der Drehung, in der es gefunden wurde.
fn best_face(e: &Engine, img: &Img, rot: &mut usize) -> Option<(Img, Face)> {
    for k in 0..4 {
        let r = ROTS[(*rot + k) % 4];
        let im = img.rotate(r);
        let faces = e.detect(&im, 0.75).ok()?;
        if let Some(f) = faces.into_iter().max_by(|a, b| (a.bbox[2] * a.bbox[3]).total_cmp(&(b.bbox[2] * b.bbox[3]))) {
            *rot = (*rot + k) % 4;
            return Some((im, f));
        }
    }
    None
}

fn load_store() -> Option<Vec<Vec<f32>>> {
    decode(&std::fs::read(STORE).ok()?)
}

fn run_camera(learn: bool, secs: u64) -> i32 {
    let stored = if learn {
        None
    } else {
        match load_store() {
            Some(s) if !s.is_empty() => Some(s),
            _ => {
                say("fehler nicht-angelernt");
                return 2;
            }
        }
    };
    let e = engine();
    let Some(mut cam) = Cam::open() else {
        say("fehler kamera");
        return 2;
    };
    say("kamera");
    let t0 = Instant::now();
    let mut rot = 0usize;
    let mut got: Vec<Vec<f32>> = Vec::new();
    let mut best = -1f32;
    let mut frames = 0;
    while t0.elapsed() < Duration::from_secs(secs) {
        let Some(img) = cam.frame() else {
            say("fehler kamera");
            return 2;
        };
        frames += 1;
        if frames <= 2 {
            continue; // erste Bilder: Belichtung regelt noch
        }
        if img.brightness() < 12.0 {
            say("dunkel");
            continue;
        }
        let Some((im, f)) = best_face(&e, &img, &mut rot) else {
            say("kein-gesicht");
            continue;
        };
        // zu klein / angeschnitten -> nicht verwenden
        let min_side = (im.w.min(im.h) as f32) * 0.18;
        if f.bbox[2] < min_side || f.bbox[0] < 0.0 || f.bbox[1] < 0.0 || f.bbox[0] + f.bbox[2] > im.w as f32 || f.bbox[1] + f.bbox[3] > im.h as f32 {
            say("naeher");
            continue;
        }
        say(&format!("gesicht {:.2}", f.score));
        let Ok(v) = e.embed(&im, &f) else { continue };
        if learn {
            // nur Bilder, die zueinander passen (eine Person), aber nicht identisch sind
            if got.iter().any(|g| cosine(g, &v) > 0.985) {
                continue;
            }
            if let Some(first) = got.first() {
                if cosine(first, &v) < THRESH {
                    say("fortschritt-neu");
                    got.clear();
                }
            }
            got.push(v);
            say(&format!("fortschritt {}/{SAMPLES}", got.len()));
            if got.len() >= SAMPLES {
                let _ = std::fs::create_dir_all(STORE_DIR);
                let tmp = format!("{STORE}.neu");
                if std::fs::write(&tmp, encode(&got)).is_err() || std::fs::rename(&tmp, STORE).is_err() {
                    say("fehler speichern");
                    return 2;
                }
                set_private(STORE);
                say("fertig");
                return 0;
            }
        } else {
            let s = stored.as_ref().unwrap().iter().map(|g| cosine(g, &v)).fold(-1f32, f32::max);
            best = best.max(s);
            if s >= THRESH {
                say(&format!("ok {s:.2}"));
                return 0;
            }
            say(&format!("nein {s:.2}"));
        }
    }
    let _ = best;
    say("zeit");
    3
}

fn set_private(p: &str) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
    let _ = std::fs::set_permissions(STORE_DIR, std::fs::Permissions::from_mode(0o700));
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("");
    let secs = |i: usize, d: u64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let rc = match cmd {
        "anlernen" => run_camera(true, secs(2, 40)),
        "pruefen" => run_camera(false, secs(2, 8)),
        "foto" if a.len() > 2 => {
            // Test: ein Kamerabild speichern (PPM) und je Drehung die Gesichtssuche zeigen
            let e = engine();
            let Some(mut cam) = Cam::open() else {
                say("fehler kamera");
                std::process::exit(2)
            };
            let mut img = None;
            for _ in 0..6 {
                img = cam.frame();
            }
            drop(cam);
            let Some(img) = img else {
                say("fehler kamera");
                std::process::exit(2)
            };
            say(&format!("bild {}x{} helligkeit {:.0}", img.w, img.h, img.brightness()));
            let mut o = format!("P6 {} {} 255\n", img.w, img.h).into_bytes();
            for p in &img.px {
                o.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
            }
            let _ = std::fs::write(&a[2], o);
            for r in ROTS {
                let t = Instant::now();
                let n = e.detect(&img.rotate(r), 0.5).map(|f| f.len()).unwrap_or(0);
                say(&format!("drehung {r}: {n} gesicht(er), {:?}", t.elapsed()));
            }
            0
        }
        "loeschen" => {
            let _ = std::fs::remove_file(STORE);
            say("geloescht");
            0
        }
        "status" => match load_store() {
            Some(v) => {
                say(&format!("angelernt {}", v.len()));
                0
            }
            None => {
                say("leer");
                1
            }
        },
        "bild" if a.len() > 2 => {
            let e = engine();
            let mut vs = Vec::new();
            for p in &a[2..] {
                let img = read_ppm(p);
                let t = Instant::now();
                let faces = e.detect(&img, 0.6).unwrap();
                let td = t.elapsed();
                for f in &faces {
                    say(&format!("{p}: score {:.3} box {:?} kps {:?}", f.score, f.bbox.map(|v| v.round()), f.kps.map(|k| k.map(|v| v.round()))));
                }
                if let Some(f) = faces.first() {
                    let t = Instant::now();
                    let v = e.embed(&img, f).unwrap();
                    say(&format!("{p}: erkennen {td:?}, merkmale {:?}, v[0..4] {:?}", t.elapsed(), &v[..4]));
                    vs.push(v);
                }
            }
            if vs.len() == 2 {
                say(&format!("aehnlichkeit {:.4}", cosine(&vs[0], &vs[1])));
            }
            0
        }
        _ => {
            eprintln!("ultar-face anlernen [sek] | pruefen [sek] | loeschen | status | bild <a.ppm> [b.ppm]");
            2
        }
    };
    std::process::exit(rc);
}
