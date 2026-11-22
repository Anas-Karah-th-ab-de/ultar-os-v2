//! Fernbedienung der Oberfläche (für den Ultar-Verbund und Geräte ohne Bildschirm).
//!
//! * **Bild teilen:** solange `<RUN>/teilen` frisch ist (der Verbund-Dienst erneuert es jede Sekunde), legt jede
//!   Bildausgabe das Bild in `<RUN>/bildschirm.raw` ab: Kopf „ULBS“, Breite, Höhe (u32), Bildnummer (u64),
//!   danach XRGB-Bildpunkte (logische Ausrichtung). Die Nummer steht zusätzlich am Ende (Schreib-Prüfung).
//! * **Eingaben + Steuerung:** Unix-Datagramme an `<RUN>/shell.fern`, je eine Zeile:
//!   `ab x y` · `zieh x y` · `auf` (0..1, logisch) · `zeichen <text>` · `enter` · `loeschen` · `zurueck` ·
//!   `lauter` · `leiser` · `hell <0..100>` · `drehen <0|90|180|270>` · `bildschirm an|aus` ·
//!   `oeffne <bildschirm> [sprache]` · `helfer <text>`.
//! * **Ohne Bildschirm:** `ultar-shell --virtuell [BxH]` (oder automatisch, wenn kein Anzeigegerät da ist)
//!   zeichnet in einen virtuellen Bildschirm – sichtbar und bedienbar über das Teilen.

use crate::input::Ev;
use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn run_dir() -> String {
    std::env::var("ULTAR_RUN").unwrap_or_else(|_| "/run/ultar".into())
}

#[derive(Debug, Clone)]
pub enum Steuer {
    Hell(f32),
    Drehen(u16),
    Bildschirm(bool),
    Oeffne(String, Option<String>),
    Helfer(String),
    /// empfangene Datei öffnen (nur vom eigenen Verbund-Dienst)
    Datei(String),
    /// Fernansicht eines anderen Geräts im Browser öffnen (nur lokale Adresse)
    Browser(String),
}

static EINGABE: Mutex<VecDeque<Ev>> = Mutex::new(VecDeque::new());
static STEUER: Mutex<VecDeque<Steuer>> = Mutex::new(VecDeque::new());
/// letzte Fern-Eingabe (ms seit Start) – hält den Bildschirm wach
pub static LETZTE: AtomicU64 = AtomicU64::new(0);

/// Empfangs-Faden starten; `wake` ist das Schreibende der Weck-Röhre der Hauptschleife
pub fn start(wake: i32) {
    let dir = run_dir();
    let _ = std::fs::create_dir_all(&dir);
    let pfad = format!("{dir}/shell.fern");
    let _ = std::fs::remove_file(&pfad);
    let sock = match std::os::unix::net::UnixDatagram::bind(&pfad) {
        Ok(s) => s,
        Err(e) => return eprintln!("ultar-shell: Fernbedienung {pfad}: {e}"),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&pfad, std::fs::Permissions::from_mode(0o600));
    }
    std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        let t0 = Instant::now();
        while let Ok(n) = sock.recv(&mut b) {
            for zeile in String::from_utf8_lossy(&b[..n]).lines() {
                verarbeiten(zeile.trim());
            }
            // jede Nachricht kann ein neues Teilen bedeuten: Anforderung sofort neu prüfen
            *GEPRUEFT.lock().unwrap() = None;
            LETZTE.store(t0.elapsed().as_millis() as u64 + 1, Ordering::Relaxed);
            if wake >= 0 {
                let x = [1u8];
                unsafe { nix::libc::write(wake, x.as_ptr() as *const _, 1) };
            }
        }
    });
}

fn zahl(s: Option<&str>) -> Option<f32> {
    s.and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite())
}

pub fn verarbeiten(z: &str) {
    let mut it = z.splitn(2, ' ');
    let cmd = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("").trim();
    let mut w = rest.split_whitespace();
    let ev = |e: Ev| EINGABE.lock().unwrap().push_back(e);
    let st = |s: Steuer| STEUER.lock().unwrap().push_back(s);
    match cmd {
        "ab" | "zieh" => {
            if let (Some(x), Some(y)) = (zahl(w.next()), zahl(w.next())) {
                let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
                ev(if cmd == "ab" { Ev::Down(x, y) } else { Ev::Move(x, y) });
            }
        }
        "auf" => ev(Ev::Up),
        "zeichen" => rest.chars().for_each(|c| ev(Ev::Char(c))),
        "enter" => ev(Ev::Enter),
        "loeschen" => ev(Ev::Backspace),
        "zurueck" => ev(Ev::Back),
        "lauter" => ev(Ev::VolUp),
        "leiser" => ev(Ev::VolDown),
        "hell" => {
            if let Some(v) = zahl(w.next()) {
                st(Steuer::Hell((v / 100.0).clamp(0.02, 1.0)));
            }
        }
        "drehen" => {
            if let Some(r) = w.next().and_then(|v| v.parse::<u16>().ok()).filter(|r| [0, 90, 180, 270].contains(r)) {
                st(Steuer::Drehen(r));
            }
        }
        "bildschirm" => st(Steuer::Bildschirm(w.next() != Some("aus"))),
        "oeffne" => {
            if let Some(n) = w.next() {
                st(Steuer::Oeffne(n.into(), w.next().map(String::from)));
            }
        }
        "helfer" if !rest.is_empty() => st(Steuer::Helfer(rest.chars().take(500).collect())),
        "datei" if rest.starts_with('/') && !rest.contains("..") => st(Steuer::Datei(rest.into())),
        "browser" if rest.starts_with("http://127.0.0.1:") || rest.starts_with("http://localhost:") => st(Steuer::Browser(rest.into())),
        _ => {}
    }
}

/// eingespeiste Eingaben (Koordinaten logisch 0..1)
pub fn eingaben() -> Vec<Ev> {
    EINGABE.lock().unwrap().drain(..).collect()
}

pub fn steuerung() -> Vec<Steuer> {
    STEUER.lock().unwrap().drain(..).collect()
}

/// letzte Bedienung für den Verbund ablegen (Mining nur im Leerlauf); höchstens alle 5 s
pub fn bedient() {
    static ZULETZT: AtomicU64 = AtomicU64::new(0);
    let jetzt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if jetzt.saturating_sub(ZULETZT.load(Ordering::Relaxed)) >= 5 {
        ZULETZT.store(jetzt, Ordering::Relaxed);
        let _ = std::fs::write(format!("{}/eingabe.zuletzt", run_dir()), jetzt.to_string());
    }
}

/// Weitergeben-Geste: lange halten (≥ 600 ms, ruhig), dann nach oben wischen (≥ 15 % der Höhe)
#[derive(Default)]
pub struct Geste {
    start: Option<(Instant, f32, f32)>,
    unruhig: bool,
    pub ausgeloest: bool,
}

impl Geste {
    /// Koordinaten logisch 0..1; liefert true, sobald die Geste erkannt ist (einmal je Berührung)
    pub fn ereignis(&mut self, e: &Ev) -> bool {
        match *e {
            Ev::Down(x, y) => {
                *self = Geste { start: Some((Instant::now(), x, y)), ..Default::default() };
                false
            }
            Ev::Move(x, y) => {
                let Some((t, x0, y0)) = self.start else { return false };
                if self.ausgeloest {
                    return false;
                }
                let weg = ((x - x0).powi(2) + (y - y0).powi(2)).sqrt();
                if t.elapsed() < Duration::from_millis(600) {
                    if weg > 0.03 {
                        self.unruhig = true;
                    }
                    return false;
                }
                if !self.unruhig && y0 - y > 0.15 {
                    self.ausgeloest = true;
                    return true;
                }
                false
            }
            Ev::Up => {
                self.start = None;
                false
            }
            _ => false,
        }
    }
}

// ---------------- Bild teilen ----------------

static AKTIV: AtomicBool = AtomicBool::new(false);
static GEPRUEFT: Mutex<Option<Instant>> = Mutex::new(None);
static NR: AtomicU64 = AtomicU64::new(0);
static DATEI: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// wird gerade geteilt? (Anforderungsdatei jünger als 5 s; höchstens alle 500 ms geprüft)
pub fn teilen_aktiv() -> bool {
    let mut g = GEPRUEFT.lock().unwrap();
    if g.map_or(true, |t| t.elapsed() > Duration::from_millis(500)) {
        *g = Some(Instant::now());
        let frisch = std::fs::metadata(format!("{}/teilen", run_dir()))
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|e| e < Duration::from_secs(5)).unwrap_or(true))
            .unwrap_or(false);
        AKTIV.store(frisch, Ordering::Relaxed);
        if !frisch {
            *DATEI.lock().unwrap() = None;
        }
    }
    AKTIV.load(Ordering::Relaxed)
}

/// Bild ablegen (nur wenn geteilt wird). `px` ist die logische Zeichenfläche, Zeilenlänge `w`.
pub fn bild(px: &[u32], w: i32) {
    if w <= 0 || !teilen_aktiv() {
        return;
    }
    let h = px.len() / w as usize;
    let mut d = DATEI.lock().unwrap();
    if d.is_none() {
        let pfad = format!("{}/bildschirm.raw", run_dir());
        *d = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(pfad).ok();
    }
    let Some(f) = d.as_ref() else { return };
    let nr = NR.fetch_add(1, Ordering::Relaxed) + 1;
    let roh = unsafe { std::slice::from_raw_parts(px.as_ptr() as *const u8, px.len() * 4) };
    // Kopf mit Nummer 0 = „wird geschrieben“, dann Bild, dann Kopf mit Nummer und Endnummer
    let kopf = |n: u64| {
        let mut k = Vec::with_capacity(20);
        k.extend_from_slice(b"ULBS");
        k.extend_from_slice(&(w as u32).to_le_bytes());
        k.extend_from_slice(&(h as u32).to_le_bytes());
        k.extend_from_slice(&n.to_le_bytes());
        k
    };
    let _ = f.write_all_at(&kopf(0), 0);
    let _ = f.write_all_at(roh, 20);
    let _ = f.write_all_at(&nr.to_le_bytes(), 20 + roh.len() as u64);
    let _ = f.set_len(28 + roh.len() as u64);
    let _ = f.write_all_at(&kopf(nr), 0);
    let _ = std::io::stdout().flush();
}

/// Größe des virtuellen Bildschirms: Argument, ULTAR_VIRTUELL oder /etc/ultar/virtuell, sonst 1280x800
pub fn virtuell_groesse(arg: Option<&str>) -> (u32, u32) {
    let s = arg.map(String::from).or_else(|| std::env::var("ULTAR_VIRTUELL").ok()).or_else(|| std::fs::read_to_string("/etc/ultar/virtuell").ok()).unwrap_or_default();
    let mut p = s.trim().split(['x', 'X']);
    match (p.next().and_then(|v| v.parse().ok()), p.next().and_then(|v| v.parse().ok())) {
        (Some(w), Some(h)) if (160..=7680).contains(&w) && (160..=4320).contains(&h) => (w, h),
        _ => (1280, 800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zeilen() {
        verarbeiten("ab 0.5 0.25");
        verarbeiten("zieh 2 -1");
        verarbeiten("auf");
        verarbeiten("zeichen hä");
        let e = eingaben();
        assert_eq!(e, vec![Ev::Down(0.5, 0.25), Ev::Move(1.0, 0.0), Ev::Up, Ev::Char('h'), Ev::Char('ä')]);
        verarbeiten("hell 50");
        verarbeiten("drehen 45");
        verarbeiten("drehen 90");
        verarbeiten("oeffne settings de");
        let s = steuerung();
        assert_eq!(s.len(), 3);
        assert_eq!(virtuell_groesse(Some("800x600")), (800, 600));
        assert_eq!(virtuell_groesse(Some("quatsch")), (1280, 800));
        verarbeiten("browser http://evil.example/");
        verarbeiten("datei ../etc/passwd");
        assert!(steuerung().is_empty());
        // Geste: Wischen ohne Halten löst nicht aus
        let mut g = Geste::default();
        assert!(!g.ereignis(&Ev::Down(0.5, 0.8)));
        assert!(!g.ereignis(&Ev::Move(0.5, 0.5)));
    }
}
