//! Ultar-Verbund im Kern.
//!
//! * Schalter: `/proc/ultar/verbund` (lesen: an/aus + Geräte; schreiben: „an“ / „aus“) und Konsole `verbund an|aus`.
//! * AUS ist hart: der Kern verwirft dann alle Pakete der Verbund-Ports (UDP 47800 Meldungen, TCP 47801 Kanal),
//!   ein- wie ausgehend – das Gerät ist für andere unsichtbar, egal was ein Programm versucht.
//! * Geräte im Netz: Meldungen auf UDP 47800 werden auch ohne laufenden Dienst gelesen (`verbund` in der Konsole).
//! Die eigentliche Arbeit (Noise-Verschlüsselung, Kopplung, Aufgaben) macht der Dienst `ultar-verbund`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub const UDP_PORT: u16 = 47800;
pub const TCP_PORT: u16 = 47801;

pub static AKTIV: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct Geraet {
    pub kennung: String,
    pub name: String,
    pub ausgabe: String,
    pub ip: [u8; 4],
    pub port: u16,
    pub kerne: u32,
    pub frei: u32,
    pub zuletzt: u64,
}

static GERAETE: Mutex<Vec<Geraet>> = Mutex::new(Vec::new());

pub fn aktiv() -> bool {
    AKTIV.load(Ordering::Relaxed)
}

pub fn schalten(an: bool) {
    if AKTIV.swap(an, Ordering::SeqCst) != an {
        crate::kprintln!(
            "[verbund] {}",
            if an {
                "AN"
            } else {
                "AUS – Verbund-Ports gesperrt"
            }
        );
    }
    if !an {
        GERAETE.lock().clear();
    }
}

/// Paket über einen Verbund-Port? Dann nur bei AN durchlassen.
pub fn erlaubt(proto: u8, sport: u16, dport: u16) -> bool {
    if aktiv() {
        return true;
    }
    let p = |x: u16| (proto == 17 && x == UDP_PORT) || (proto == 6 && x == TCP_PORT);
    !(p(sport) || p(dport))
}

/// Wert eines JSON-Feldes (flach, ohne Verschachtelung) – genügt für die Meldungen
fn feld<'a>(s: &'a str, k: &str) -> Option<&'a str> {
    let key = format!("\"{k}\":");
    let i = s.find(&key)? + key.len();
    let r = s[i..].trim_start();
    if let Some(r) = r.strip_prefix('"') {
        let e = r.find('"')?;
        Some(&r[..e])
    } else {
        let e = r.find(|c: char| c == ',' || c == '}').unwrap_or(r.len());
        Some(r[..e].trim())
    }
}

/// Meldung eines Geräts (UDP 47800), die kein Programm abgeholt hat
pub fn meldung(src: [u8; 4], d: &[u8]) {
    if !aktiv() || d.len() > 1500 {
        return;
    }
    let Ok(s) = core::str::from_utf8(d) else {
        return;
    };
    if feld(s, "v") != Some("1") || feld(s, "aktiv") != Some("true") {
        return;
    }
    let (Some(k), Some(n)) = (feld(s, "k"), feld(s, "n")) else {
        return;
    };
    let g = Geraet {
        kennung: k.chars().take(16).collect(),
        name: n.chars().take(32).collect(),
        ausgabe: feld(s, "a").unwrap_or("").chars().take(12).collect(),
        ip: src,
        port: feld(s, "p")
            .and_then(|x| x.parse().ok())
            .unwrap_or(TCP_PORT),
        kerne: feld(s, "kerne").and_then(|x| x.parse().ok()).unwrap_or(0),
        frei: feld(s, "frei").and_then(|x| x.parse().ok()).unwrap_or(0),
        zuletzt: crate::arch::uptime_ms(),
    };
    let mut v = GERAETE.lock();
    let neu = !v.iter().any(|x| x.kennung == g.kennung);
    v.retain(|x| x.kennung != g.kennung);
    if neu {
        crate::kprintln!(
            "[verbund] gefunden: {} „{}“ ({}) {}.{}.{}.{}",
            g.kennung,
            g.name,
            g.ausgabe,
            src[0],
            src[1],
            src[2],
            src[3]
        );
    }
    if v.len() < 64 {
        v.push(g);
    }
}

pub fn geraete() -> Vec<Geraet> {
    let jetzt = crate::arch::uptime_ms();
    let mut v = GERAETE.lock();
    v.retain(|g| jetzt.saturating_sub(g.zuletzt) < 10_000);
    v.clone()
}

pub fn text() -> String {
    let mut s = String::from(if aktiv() { "an\n" } else { "aus\n" });
    for g in geraete() {
        s += &format!(
            "{} {} {} {}.{}.{}.{}:{} {}/{}\n",
            g.kennung,
            g.name,
            g.ausgabe,
            g.ip[0],
            g.ip[1],
            g.ip[2],
            g.ip[3],
            g.port,
            g.frei,
            g.kerne
        );
    }
    s
}

/// Schreiben auf /proc/ultar/verbund
pub fn proc_schreiben(d: &[u8]) {
    let s = core::str::from_utf8(d).unwrap_or("").trim();
    match s {
        "an" | "1" | "on" => schalten(true),
        "aus" | "0" | "off" => schalten(false),
        _ => {}
    }
}
