//! Bildschirm teilen und fernbedienen.
//!
//! Quelle: die Oberfläche (ultar-shell) legt bei Bedarf ihr Bild in `<RUN>/bildschirm.raw` ab (siehe
//! crates/ultar-shell/src/fern.rs). Hat das Gerät keine laufende Oberfläche (Server, Platine ohne Bildschirm),
//! startet der Dienst sie als virtuellen Bildschirm (`ultar-shell --virtuell`) und beendet sie wieder.
//! Übertragung: Kacheln 64×64, nur geänderte, RGB + LZ4. Eingaben und Steuerbefehle gehen als Zeilen an
//! `<RUN>/shell.fern`.

use std::io::Read;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const KACHEL: usize = 64;

pub fn run_dir() -> PathBuf {
    std::env::var("ULTAR_RUN")
        .unwrap_or_else(|_| "/run/ultar".into())
        .into()
}

pub struct Bild {
    pub w: usize,
    pub h: usize,
    pub nr: u64,
    pub px: Vec<u32>,
}

/// aktuelles Bild lesen (None: keins da oder gerade beim Schreiben)
pub fn lesen() -> Option<Bild> {
    let p = run_dir().join("bildschirm.raw");
    for _ in 0..5 {
        let mut f = std::fs::File::open(&p).ok()?;
        let mut kopf = [0u8; 20];
        f.read_exact(&mut kopf).ok()?;
        if &kopf[..4] != b"ULBS" {
            return None;
        }
        let w = u32::from_le_bytes(kopf[4..8].try_into().unwrap()) as usize;
        let h = u32::from_le_bytes(kopf[8..12].try_into().unwrap()) as usize;
        let nr = u64::from_le_bytes(kopf[12..20].try_into().unwrap());
        if w == 0 || h == 0 || w * h > 8192 * 8192 {
            return None;
        }
        if nr == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let mut roh = vec![0u8; w * h * 4 + 8];
        if f.read_exact(&mut roh).is_err() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let ende = u64::from_le_bytes(roh[w * h * 4..].try_into().unwrap());
        if ende != nr {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let px = roh[..w * h * 4]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        return Some(Bild { w, h, nr, px });
    }
    None
}

/// Teilen anfordern (die Oberfläche legt dann Bilder ab) – jede Sekunde erneuern
pub fn anfordern() {
    let d = run_dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join("teilen"), crate::jetzt_ms().to_string());
}

/// Zeile an die Oberfläche (Eingabe oder Steuerung)
pub fn an_oberflaeche(zeile: &str) -> Result<(), String> {
    let s = UnixDatagram::unbound().map_err(|e| e.to_string())?;
    s.send_to(zeile.as_bytes(), run_dir().join("shell.fern"))
        .map(|_| ())
        .map_err(|e| format!("Oberfläche nicht erreichbar: {e}"))
}

pub fn oberflaeche_laeuft() -> bool {
    run_dir().join("shell.fern").exists() && an_oberflaeche("").is_ok()
}

// ---------------- virtuelle Oberfläche ----------------

static VIRTUELL: Mutex<Option<(std::process::Child, Instant)>> = Mutex::new(None);

/// Oberfläche sicherstellen: läuft keine, `ultar-shell --virtuell` starten. Merkt die Nutzung (für das Beenden).
pub fn oberflaeche_sicherstellen() -> Result<(), String> {
    let mut v = VIRTUELL.lock().unwrap();
    if let Some((k, t)) = v.as_mut() {
        if k.try_wait().ok().flatten().is_none() {
            *t = Instant::now();
            return Ok(());
        }
        *v = None;
    }
    if oberflaeche_laeuft() {
        return Ok(());
    }
    let prog = std::env::var("ULTAR_SHELL").unwrap_or_else(|_| "ultar-shell".into());
    let mut cmd = std::process::Command::new(&prog);
    cmd.arg("--virtuell")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::dienst::ohne_netz(&mut cmd);
    if let Ok(g) = std::env::var("ULTAR_VIRTUELL") {
        cmd.arg(g);
    }
    let k = cmd
        .spawn()
        .map_err(|e| format!("keine Oberfläche ({prog}): {e}"))?;
    crate::dienst::log("virtueller Bildschirm gestartet (Gerät ohne Bildschirm)");
    *v = Some((k, Instant::now()));
    drop(v);
    // warten, bis sie Befehle annimmt
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        if oberflaeche_laeuft() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("virtuelle Oberfläche startet nicht".into())
}

/// unbenutzte virtuelle Oberfläche nach `ruhe` beenden (vom Dienst regelmäßig aufgerufen)
pub fn aufraeumen(ruhe: Duration) {
    let mut v = VIRTUELL.lock().unwrap();
    if let Some((k, t)) = v.as_mut() {
        if t.elapsed() > ruhe {
            let _ = k.kill();
            let _ = k.wait();
            crate::dienst::log("virtueller Bildschirm beendet (keiner schaut zu)");
            *v = None;
        }
    }
}

pub fn benutzt() {
    if let Some((_, t)) = VIRTUELL.lock().unwrap().as_mut() {
        *t = Instant::now();
    }
}

// ---------------- Kodierung ----------------

/// Unterschiede zu `alt` als Kachelpaket: je Kachel [x u16][y u16][w u16][h u16][len u32][lz4(RGB)]
pub fn kodieren(neu: &Bild, alt: Option<&Bild>, voll: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let gleiche_groesse = alt.map(|a| a.w == neu.w && a.h == neu.h).unwrap_or(false);
    let mut rgb = Vec::with_capacity(KACHEL * KACHEL * 3);
    for ty in (0..neu.h).step_by(KACHEL) {
        for tx in (0..neu.w).step_by(KACHEL) {
            let (kw, kh) = (KACHEL.min(neu.w - tx), KACHEL.min(neu.h - ty));
            let geaendert = voll
                || !gleiche_groesse
                || (0..kh).any(|y| {
                    let i = (ty + y) * neu.w + tx;
                    neu.px[i..i + kw] != alt.unwrap().px[i..i + kw]
                });
            if !geaendert {
                continue;
            }
            rgb.clear();
            for y in 0..kh {
                let i = (ty + y) * neu.w + tx;
                for &p in &neu.px[i..i + kw] {
                    rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
                }
            }
            let z = lz4_flex::compress_prepend_size(&rgb);
            for v in [tx as u16, ty as u16, kw as u16, kh as u16] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&(z.len() as u32).to_le_bytes());
            out.extend_from_slice(&z);
        }
    }
    out
}

/// Kachelpaket in einen Bildpuffer (RGBA, Zeilenlänge w) einspielen; liefert die geänderten Kacheln
pub fn dekodieren(
    paket: &[u8],
    w: usize,
    h: usize,
    rgba: &mut [u8],
) -> Result<Vec<(usize, usize, usize, usize)>, String> {
    let mut p = 0;
    let mut geaendert = Vec::new();
    while p + 12 <= paket.len() {
        let r = |o: usize| u16::from_le_bytes([paket[p + o], paket[p + o + 1]]) as usize;
        let (x, y, kw, kh) = (r(0), r(2), r(4), r(6));
        let len = u32::from_le_bytes(paket[p + 8..p + 12].try_into().unwrap()) as usize;
        p += 12;
        if p + len > paket.len() || x + kw > w || y + kh > h {
            return Err("Kachel außerhalb".into());
        }
        let rgb =
            lz4_flex::decompress_size_prepended(&paket[p..p + len]).map_err(|e| e.to_string())?;
        p += len;
        if rgb.len() != kw * kh * 3 {
            return Err("Kachelgröße".into());
        }
        for yy in 0..kh {
            for xx in 0..kw {
                let s = (yy * kw + xx) * 3;
                let d = ((y + yy) * w + x + xx) * 4;
                rgba[d..d + 4].copy_from_slice(&[rgb[s], rgb[s + 1], rgb[s + 2], 255]);
            }
        }
        geaendert.push((x, y, kw, kh));
    }
    Ok(geaendert)
}

/// erlaubte Steuerbefehle für die Oberfläche (keine beliebigen Zeilen von außen)
pub fn eingabe_erlaubt(z: &str) -> bool {
    let cmd = z.split_whitespace().next().unwrap_or("");
    matches!(
        cmd,
        "ab" | "zieh"
            | "auf"
            | "zeichen"
            | "enter"
            | "loeschen"
            | "zurueck"
            | "lauter"
            | "leiser"
            | "hell"
            | "drehen"
            | "bildschirm"
            | "oeffne"
            | "helfer"
    ) && z.len() < 600
        && !z.contains('\n')
}

/// Bild als BMP (für `ultar-verbund foto`)
pub fn bmp(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let zeile = (w * 3 + 3) & !3;
    let groesse = 54 + zeile * h;
    let mut b = Vec::with_capacity(groesse);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(groesse as u32).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0]);
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&[1, 0, 24, 0, 0, 0, 0, 0]);
    b.extend_from_slice(&((zeile * h) as u32).to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    for y in (0..h).rev() {
        for x in 0..w {
            let i = (y * w + x) * 4;
            b.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i]]);
        }
        b.extend(std::iter::repeat(0).take(zeile - w * 3));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hin_und_zurueck() {
        let (w, h) = (150, 70);
        let a = Bild {
            w,
            h,
            nr: 1,
            px: (0..w * h)
                .map(|i| (i as u32).wrapping_mul(2654435761) & 0xffffff)
                .collect(),
        };
        let mut b = Bild {
            w,
            h,
            nr: 2,
            px: a.px.clone(),
        };
        b.px[69 * w + 149] = 0x123456;
        let voll = kodieren(&a, None, true);
        let mut rgba = vec![0u8; w * h * 4];
        assert_eq!(dekodieren(&voll, w, h, &mut rgba).unwrap().len(), 3 * 2);
        let diff = kodieren(&b, Some(&a), false);
        let k = dekodieren(&diff, w, h, &mut rgba).unwrap();
        assert_eq!(k, vec![(128, 64, 22, 6)]);
        for i in 0..w * h {
            let p = b.px[i];
            assert_eq!(
                &rgba[i * 4..i * 4 + 3],
                &[(p >> 16) as u8, (p >> 8) as u8, p as u8]
            );
        }
        assert!(kodieren(&b, Some(&b), false).is_empty());
        assert!(
            eingabe_erlaubt("ab 0.1 0.2")
                && !eingabe_erlaubt("rm -rf /")
                && !eingabe_erlaubt("ab 1\nhell 1")
        );
    }
}
