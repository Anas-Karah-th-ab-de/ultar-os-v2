//! Gerät fernsteuern (eingebaute Befehle des Entwickler-Zugangs): Bildschirmfoto, Tippen, Wischen, Text, Tasten,
//! Bildschirm öffnen. Weg 1: Fernbedienung der Oberfläche (`/run/ultar/shell.fern`, Handy und PC unter Linux).
//! Weg 2 (Ultar-Kern ohne Unix-Datagramme): Ereignisse direkt in `/dev/input/event0/1` schreiben wie unter Linux.

use std::io::Write;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant, SystemTime};

const FOTO: &str = "/data/ultar/screen.png";

fn run_dir() -> String {
    std::env::var("ULTAR_RUN").unwrap_or_else(|_| "/run/ultar".into())
}

/// PID der Oberfläche (ultar-shell)
fn shell_pid() -> Option<i32> {
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let Ok(pid) = n.parse::<i32>() else { continue };
        let comm = std::fs::read_to_string(format!("/proc/{n}/comm")).unwrap_or_default();
        if comm.trim() == "ultar-shell" {
            return Some(pid);
        }
    }
    None
}

fn mtime(p: &str) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Breite/Höhe aus dem PNG-Kopf
fn png_groesse(p: &str) -> Option<(u32, u32)> {
    let f = std::fs::File::open(p).ok()?;
    let mut b = [0u8; 24];
    f.read_exact_at(&mut b, 0).ok()?;
    (&b[1..4] == b"PNG").then(|| (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap())))
}

/// Bildschirmfoto: Oberfläche (SIGUSR1 → /data/ultar/screen.png), sonst Bildspeicher /dev/fb0
pub fn bildschirm() -> Result<String, String> {
    if let Some(pid) = shell_pid() {
        let vorher = mtime(FOTO);
        unsafe { libc::kill(pid, libc::SIGUSR1) };
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(100));
            if mtime(FOTO).is_some() && mtime(FOTO) != vorher {
                // fertig geschrieben? (Größe stabil)
                std::thread::sleep(Duration::from_millis(150));
                let (w, h) = png_groesse(FOTO).unwrap_or((0, 0));
                return Ok(format!("{FOTO} {w}x{h}\n"));
            }
        }
        return Err("Oberfläche hat kein Foto geliefert (8 s)".into());
    }
    fb_foto()
}

fn fb_foto() -> Result<String, String> {
    let f = std::fs::File::open("/dev/fb0").map_err(|e| format!("kein Bildschirm: {e}"))?;
    let mut var = [0u32; 40];
    let mut fix = [0u8; 80];
    use std::os::fd::AsRawFd;
    unsafe {
        if libc::ioctl(f.as_raw_fd(), 0x4600, var.as_mut_ptr()) != 0 || libc::ioctl(f.as_raw_fd(), 0x4602, fix.as_mut_ptr()) != 0 {
            return Err("Bildspeicher: ioctl".into());
        }
    }
    let (w, h, bpp) = (var[0] as usize, var[1] as usize, var[6] as usize);
    let line = u32::from_le_bytes(fix[48..52].try_into().unwrap()) as usize;
    if bpp != 32 || w == 0 || h == 0 {
        return Err(format!("Bildspeicher {w}x{h}x{bpp} nicht unterstützt"));
    }
    let (ro, go, bo) = (var[8], var[11], var[14]);
    let yoff = var[5] as usize;
    let mut roh = vec![0u8; line * h];
    f.read_exact_at(&mut roh, (yoff * line) as u64).map_err(|e| format!("Bildspeicher lesen: {e}"))?;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let p = u32::from_le_bytes(roh[y * line + x * 4..y * line + x * 4 + 4].try_into().unwrap());
            rgb.extend_from_slice(&[(p >> ro) as u8, (p >> go) as u8, (p >> bo) as u8]);
        }
    }
    let ziel = "/tmp/bildschirm.png";
    let datei = std::fs::File::create(ziel).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(datei), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&rgb)).map_err(|e| e.to_string())?;
    Ok(format!("{ziel} {w}x{h}\n"))
}

/// Größe des Bildschirms (für Bildpunkt → 0..1): letztes Foto, sonst Bildspeicher
fn groesse() -> Option<(f32, f32)> {
    if let Some((w, h)) = png_groesse(FOTO).filter(|g| g.0 > 0) {
        return Some((w as f32, h as f32));
    }
    let f = std::fs::File::open("/dev/fb0").ok()?;
    let mut var = [0u32; 40];
    use std::os::fd::AsRawFd;
    (unsafe { libc::ioctl(f.as_raw_fd(), 0x4600, var.as_mut_ptr()) } == 0 && var[0] > 0).then(|| (var[0] as f32, var[1] as f32))
}

/// Koordinate: „0.5“ = Anteil, „640“ = Bildpunkt des Fotos
fn koord(x: &str, y: &str) -> Result<(f32, f32), String> {
    let (a, b): (f32, f32) = (x.parse().map_err(|_| format!("Zahl? {x}"))?, y.parse().map_err(|_| format!("Zahl? {y}"))?);
    if a <= 1.0 && b <= 1.0 && (x.contains('.') || y.contains('.')) {
        return Ok((a.clamp(0.0, 1.0), b.clamp(0.0, 1.0)));
    }
    let (w, h) = groesse().ok_or("Bildschirmgröße unbekannt – erst „bildschirm“ aufrufen oder Anteile (0.0–1.0) angeben")?;
    Ok(((a / (w - 1.0).max(1.0)).clamp(0.0, 1.0), (b / (h - 1.0).max(1.0)).clamp(0.0, 1.0)))
}

/// Zeile an die Fernbedienung der Oberfläche; false = nicht erreichbar
fn fern(zeilen: &[String]) -> bool {
    let pfad = format!("{}/shell.fern", run_dir());
    let Ok(s) = std::os::unix::net::UnixDatagram::unbound() else { return false };
    for z in zeilen {
        if s.send_to(z.as_bytes(), &pfad).is_err() {
            return false;
        }
    }
    true
}

// --- Weg 2: Ereignisse direkt (Ultar-Kern) -------------------------------------------------------

fn ev(typ: u16, code: u16, wert: i32) -> [u8; 24] {
    let mut e = [0u8; 24];
    e[16..18].copy_from_slice(&typ.to_le_bytes());
    e[18..20].copy_from_slice(&code.to_le_bytes());
    e[20..24].copy_from_slice(&wert.to_le_bytes());
    e
}

fn schreibe(dev: &str, evs: &[[u8; 24]]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new().write(true).open(dev).map_err(|e| format!("{dev}: {e}"))?;
    let alles: Vec<u8> = evs.iter().flatten().copied().collect();
    f.write_all(&alles).map_err(|e| format!("{dev}: {e}"))
}

const SYN: [u8; 24] = [0; 24];

fn maus_pos(u: f32, v: f32) -> Result<Vec<[u8; 24]>, String> {
    let (w, h) = groesse().ok_or("Bildschirmgröße unbekannt")?;
    Ok(vec![ev(3, 0, (u * (w - 1.0)).round() as i32), ev(3, 1, (v * (h - 1.0)).round() as i32), SYN])
}

fn ev_tippen(u: f32, v: f32) -> Result<(), String> {
    let mut e = maus_pos(u, v)?;
    e.extend([ev(1, 0x110, 1), SYN]);
    schreibe("/dev/input/event0", &e)?;
    std::thread::sleep(Duration::from_millis(60));
    schreibe("/dev/input/event0", &[ev(1, 0x110, 0), SYN])
}

fn ev_wischen(a: (f32, f32), b: (f32, f32), ms: u64) -> Result<(), String> {
    let mut e = maus_pos(a.0, a.1)?;
    e.extend([ev(1, 0x110, 1), SYN]);
    schreibe("/dev/input/event0", &e)?;
    let n = 12;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        std::thread::sleep(Duration::from_millis(ms / n));
        schreibe("/dev/input/event0", &maus_pos(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)?)?;
    }
    schreibe("/dev/input/event0", &[ev(1, 0x110, 0), SYN])
}

/// Belegung der Hardware-Tastatur wie die Oberfläche sie gemerkt hat (/data/ultar/tastatur), sonst nach Systemsprache
fn deutsch() -> bool {
    match std::fs::read_to_string("/data/ultar/tastatur").ok().as_deref().map(str::trim) {
        Some("de") => true,
        Some("us") | Some("en") => false,
        _ => std::fs::read_to_string("/data/ultar/settings.toml").map(|s| s.contains("lang = \"de\"")).unwrap_or(false),
    }
}

/// Umschalter einer Taste
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mod {
    Keine,
    Umschalt,
    AltGr,
}

/// Zeichen → Linux-Tastencode + Umschalter (US- oder deutsche Belegung, wie die Oberfläche sie auswertet)
fn zeichen_code(c: char, de: bool) -> Option<(u16, Mod)> {
    const US: [(u16, &str, &str); 4] = [(2, "1234567890-=", "!@#$%^&*()_+"), (16, "qwertyuiop[]", "QWERTYUIOP{}"), (30, "asdfghjkl;'`", "ASDFGHJKL:\"~"), (44, "zxcvbnm,./", "ZXCVBNM<>?")];
    const DE: [(u16, &str, &str); 4] = [(2, "1234567890ß´", "!\"§$%&/()=?`"), (16, "qwertzuiopü+", "QWERTZUIOPÜ*"), (30, "asdfghjklöä^", "ASDFGHJKLÖÄ°"), (44, "yxcvbnm,.-", "YXCVBNM;:_")];
    match c {
        '\n' => return Some((28, Mod::Keine)),
        ' ' => return Some((57, Mod::Keine)),
        '\t' => return Some((15, Mod::Keine)),
        _ => {}
    }
    if de {
        let x = match c {
            '#' => Some((43, Mod::Keine)),
            '\'' => Some((43, Mod::Umschalt)),
            '<' => Some((86, Mod::Keine)),
            '>' => Some((86, Mod::Umschalt)),
            '|' => Some((86, Mod::AltGr)),
            '@' => Some((16, Mod::AltGr)),
            '€' => Some((18, Mod::AltGr)),
            '{' => Some((8, Mod::AltGr)),
            '[' => Some((9, Mod::AltGr)),
            ']' => Some((10, Mod::AltGr)),
            '}' => Some((11, Mod::AltGr)),
            '\\' => Some((12, Mod::AltGr)),
            '~' => Some((27, Mod::AltGr)),
            _ => None,
        };
        if x.is_some() {
            return x;
        }
    } else {
        match c {
            '\\' => return Some((43, Mod::Keine)),
            '|' => return Some((43, Mod::Umschalt)),
            _ => {}
        }
    }
    for (start, lo, hi) in if de { DE } else { US } {
        if let Some(i) = lo.chars().position(|x| x == c) {
            return Some((start + i as u16, Mod::Keine));
        }
        if let Some(i) = hi.chars().position(|x| x == c) {
            return Some((start + i as u16, Mod::Umschalt));
        }
    }
    None
}

fn ev_tasten(codes: &[(u16, Mod)]) -> Result<(), String> {
    let mut e = Vec::new();
    for &(c, m) in codes {
        let umschalter = match m {
            Mod::Keine => None,
            Mod::Umschalt => Some(42),
            Mod::AltGr => Some(100),
        };
        if let Some(u) = umschalter {
            e.extend([ev(1, u, 1), SYN]);
        }
        e.extend([ev(1, c, 1), SYN, ev(1, c, 0), SYN]);
        if let Some(u) = umschalter {
            e.extend([ev(1, u, 0), SYN]);
        }
    }
    schreibe("/dev/input/event1", &e)
}

fn tasten_name(n: &str) -> Option<(u16, &'static str)> {
    Some(match n {
        "enter" | "eingabe" => (28, "enter"),
        "zurueck" | "zurück" | "back" | "esc" => (1, "zurueck"),
        "loeschen" | "löschen" | "backspace" => (14, "loeschen"),
        "lauter" => (115, "lauter"),
        "leiser" => (114, "leiser"),
        "tab" => (15, ""),
        "hoch" | "up" => (103, ""),
        "runter" | "down" => (108, ""),
        "links" | "left" => (105, ""),
        "rechts" | "right" => (106, ""),
        "home" | "start" => (102, ""),
        "ein" | "power" => (116, ""),
        _ => return None,
    })
}

// --- Befehle --------------------------------------------------------------------------------------

pub fn befehl(w: &[&str], roh: &str) -> Option<Result<String, String>> {
    let r = match w {
        ["bildschirm"] | ["foto"] | ["screenshot"] => bildschirm(),
        ["tippen", x, y] | ["tap", x, y] => koord(x, y).and_then(|(u, v)| {
            if fern(&[format!("ab {u} {v}"), "auf".into()]) {
                Ok(format!("getippt {u:.3} {v:.3}\n"))
            } else {
                ev_tippen(u, v).map(|_| format!("getippt {u:.3} {v:.3} (Eingabegerät)\n"))
            }
        }),
        ["wischen", x1, y1, x2, y2, rest @ ..] | ["swipe", x1, y1, x2, y2, rest @ ..] => {
            let ms: u64 = rest.first().and_then(|m| m.parse().ok()).unwrap_or(300).clamp(50, 5000);
            koord(x1, y1).and_then(|a| koord(x2, y2).map(|b| (a, b))).and_then(|(a, b)| {
                let n = 12;
                let mut z = vec![format!("ab {} {}", a.0, a.1)];
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    z.push(format!("zieh {} {}", a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                }
                z.push("auf".into());
                // Fernbedienung: Schritte mit Pause, damit die Oberfläche den Zug sieht
                let pfad = format!("{}/shell.fern", run_dir());
                let s = std::os::unix::net::UnixDatagram::unbound().ok();
                let ok = s.as_ref().is_some_and(|s| {
                    z.iter().all(|l| {
                        std::thread::sleep(Duration::from_millis(ms / (n as u64 + 1)));
                        s.send_to(l.as_bytes(), &pfad).is_ok()
                    })
                });
                if ok {
                    Ok("gewischt\n".into())
                } else {
                    ev_wischen(a, b, ms).map(|_| "gewischt (Eingabegerät)\n".into())
                }
            })
        }
        ["text", ..] => {
            let t = roh.trim_start().strip_prefix("text").unwrap_or("").trim_start();
            let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
            if fern(&[format!("zeichen {t}")]) {
                Ok(format!("getippt: {t}\n"))
            } else {
                let de = deutsch();
                let codes: Vec<(u16, Mod)> = t.chars().filter_map(|c| zeichen_code(c, de)).collect();
                ev_tasten(&codes).map(|_| format!("getippt: {t} (Tastatur)\n"))
            }
        }
        ["taste", n] | ["key", n] => match tasten_name(n) {
            None => Err(format!("unbekannte Taste {n} (enter, zurueck, loeschen, lauter, leiser, tab, hoch, runter, links, rechts, home, ein)")),
            Some((code, fern_name)) => {
                if !fern_name.is_empty() && fern(&[fern_name.into()]) {
                    Ok(format!("taste {n}\n"))
                } else {
                    ev_tasten(&[(code, Mod::Keine)]).map(|_| format!("taste {n} (Tastatur)\n"))
                }
            }
        },
        ["oeffne", s, rest @ ..] | ["öffne", s, rest @ ..] | ["open", s, rest @ ..] => {
            let z = format!("oeffne {s} {}", rest.first().copied().unwrap_or("")).trim().to_string();
            if fern(&[z]) {
                std::thread::sleep(Duration::from_millis(400));
                Ok(format!("geöffnet: {s}\n"))
            } else if let Some(pid) = shell_pid() {
                // Ultar-Kern: über die Tür der Oberfläche (shell.goto + Foto)
                let _ = std::fs::create_dir_all(run_dir());
                let _ = std::fs::write("/run/ultar/shell.goto", format!("{s} {}", rest.first().copied().unwrap_or("")));
                let vorher = mtime(FOTO);
                unsafe { libc::kill(pid, libc::SIGUSR1) };
                let t = Instant::now();
                while t.elapsed() < Duration::from_secs(8) && mtime(FOTO) == vorher {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Ok(format!("geöffnet: {s} (Foto: {FOTO})\n"))
            } else {
                Err("Oberfläche läuft nicht".into())
            }
        }
        ["helfer", ..] => {
            let t = roh.trim_start().strip_prefix("helfer").unwrap_or("").trim();
            if fern(&[format!("helfer {t}")]) {
                Ok("an den Helfer geschickt\n".into())
            } else {
                Err("Fernbedienung der Oberfläche nicht erreichbar".into())
            }
        }
        _ => return None,
    };
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn koordinaten() {
        assert_eq!(koord("0.5", "0.25").unwrap(), (0.5, 0.25));
        assert!(koord("x", "1").is_err());
    }

    #[test]
    fn tasten() {
        assert_eq!(zeichen_code('a', false), Some((30, Mod::Keine)));
        assert_eq!(zeichen_code('A', false), Some((30, Mod::Umschalt)));
        assert_eq!(zeichen_code('z', false), Some((44, Mod::Keine)));
        assert_eq!(zeichen_code('z', true), Some((21, Mod::Keine)), "QWERTZ");
        assert_eq!(zeichen_code('+', true), Some((27, Mod::Keine)));
        assert_eq!(zeichen_code('/', true), Some((8, Mod::Umschalt)));
        assert_eq!(zeichen_code('@', true), Some((16, Mod::AltGr)));
        assert!(tasten_name("enter").is_some());
        assert!(tasten_name("quatsch").is_none());
    }

    #[test]
    fn ereignis_format() {
        let e = ev(1, 0x110, 1);
        assert_eq!(&e[16..18], &1u16.to_le_bytes());
        assert_eq!(&e[18..20], &0x110u16.to_le_bytes());
        assert_eq!(&e[20..24], &1i32.to_le_bytes());
    }
}
