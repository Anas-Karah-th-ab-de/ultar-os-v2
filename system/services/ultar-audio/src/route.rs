//! Tonwege für Ultar: Medien (Lautsprecher/Hörmuschel/Kopfhörer), Aufnahme und Telefongespräch.
//! Grundlage sind Samsungs Wege aus mixer_paths.xml; nur der Lautsprecher nutzt unseren am Gerät
//! bestätigten Weg (RDMA0 → SPUS OUT0 → SIFS1 → UAIF SPK3 → SMA1301).

use crate::codec::Codec;
use crate::codec_cod3035::Cod3035Codec;
use crate::ctl::Ctl;
use crate::jack::{self, Jack};
use crate::paths::Paths;
use crate::stream::Stream;

/// Steuerdatei des Gesprächstons: `auto|handset|speaker|headset|off` (schreiben RIL und Oberfläche).
pub const CALL_FILE: &str = "/run/ultar/callaudio";
const CALL_PID: &str = "/run/ultar/callaudio.pid";
/// Basisband-Strecken (mixer_paths: baseband_link / baseband_capture_link)
const BB_RX: u32 = 4;
const BB_TX: u32 = 10;
/// Aufnahme (capture_link)
pub const CAPTURE: u32 = 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dev {
    Speaker,
    Handset,
    Headset,
    Headphone,
}

impl Dev {
    pub fn parse(s: &str) -> Option<Dev> {
        Some(match s.trim() {
            "speaker" | "spk" | "laut" => Dev::Speaker,
            "handset" | "earpiece" | "ep" | "ohr" => Dev::Handset,
            "headset" | "hp" => Dev::Headset,
            "headphone" => Dev::Headphone,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Dev::Speaker => "speaker",
            Dev::Handset => "handset",
            Dev::Headset => "headset",
            Dev::Headphone => "headphone",
        }
    }
    /// Modem-Tonweg (SND_AUDIO_PATH_CTRL): Hörer 1, Kopfhörer 2, Lautsprecher 6, Kopfhörer ohne Mikro 7
    pub fn modem_path(self) -> u8 {
        match self {
            Dev::Handset => 1,
            Dev::Headset => 2,
            Dev::Speaker => 6,
            Dev::Headphone => 7,
        }
    }
}

/// Standard ohne ausdrückliche Wahl: Kopfhörer, falls eingesteckt, sonst `fallback`.
pub fn auto(fallback: Dev) -> Dev {
    match jack::state() {
        Jack::Headset => Dev::Headset,
        Jack::Headphone => Dev::Headphone,
        Jack::None => fallback,
    }
}

fn amp_off(c: &Ctl) {
    let _ = c.set("Speaker Mute Switch(1:muted_0:un)", "1");
    let _ = c.set("Power Up(1:Up_0:Down)", "0");
}

/// Läuft gerade ein Gespräch (dann bleiben die Wege unangetastet)?
pub fn call_running() -> bool {
    let pid = std::fs::read_to_string(CALL_PID).unwrap_or_default();
    let pid = pid.trim();
    !pid.is_empty() && std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Medienwiedergabe (RDMA0) auf `dev` legen. `vol` 0..1 betrifft den Lautsprecher-Verstärker.
pub fn media(dev: Dev, vol: f32, verbose: bool) -> usize {
    let Some(c) = Ctl::open() else { return 1 };
    let p = Paths::load();
    let mut err = p.reset_playback(&c, verbose);
    match dev {
        Dev::Speaker => {
            crate::mixer::Mixer::open().route_speaker(0);
            let k = Cod3035Codec::new();
            k.enable_speaker();
            k.set_speaker_volume(vol);
        }
        Dev::Handset => {
            amp_off(&c);
            err += p.apply_path(&c, "media-handset", verbose);
        }
        Dev::Headset | Dev::Headphone => {
            amp_off(&c);
            err += p.apply_path(&c, "media-headset", verbose);
        }
    }
    err
}

/// Aufnahmeweg (route-ap-record + Mikrofon) setzen.
pub fn mic(src: &str, verbose: bool) -> usize {
    let Some(c) = Ctl::open() else { return 1 };
    let p = Paths::load();
    let path = match src {
        "2nd" => "media-2nd-mic",
        "headset" => "media-headset-mic",
        "dual" => "recording-dualmic",
        _ => "media-mic",
    };
    p.apply_path(&c, path, verbose)
}

fn call_paths(c: &Ctl, p: &Paths, dev: Dev, verbose: bool) {
    p.reset(c, verbose);
    // Wie Samsungs HAL (set_mode): ABOX in den Gesprächsmodus, sonst verbindet der DSP Modem und Tonchip nicht
    let _ = c.set("ABOX Audio Mode", "IN_CALL");
    let d = dev.key();
    p.apply_path(c, &format!("incall_nb-{d}"), verbose);
    p.apply_path(c, &format!("incall_nb-{d}-mic"), verbose);
    if dev == Dev::Speaker {
        // Wie unser Lautsprecherweg für Medien: Basisband-Strecke (RDMA4 → SPUS OUT4) über SIFS1 zum Verstärker
        let _ = c.set("ABOX SPUS OUT4", "SIFS1");
        let _ = c.set("ABOX SIFS1", "SPUS OUT4");
        let _ = c.set("ABOX UAIF SPK3", "SIFS1");
        let _ = c.set("ABOX NSRC3", "UAIF3");
        let k = Cod3035Codec::new();
        k.enable_speaker();
        k.set_speaker_volume(0.7);
    } else {
        amp_off(c);
    }
}

fn want_dev(cur: Dev) -> Option<Dev> {
    let s = std::fs::read_to_string(CALL_FILE).ok()?;
    let s = s.trim();
    match s {
        "off" | "" => None,
        "auto" => Some(auto(if cur == Dev::Speaker { Dev::Speaker } else { Dev::Handset })),
        x => Some(Dev::parse(x).unwrap_or(cur)),
    }
}

/// Gesprächston: Wege setzen, Basisband-Strecken öffnen und laufen lassen, bis CALL_FILE „off" sagt.
pub fn call(initial: Option<Dev>, verbose: bool) -> i32 {
    if call_running() {
        println!("Gesprächston läuft schon");
        return 0;
    }
    let _ = std::fs::write(CALL_PID, std::process::id().to_string());
    if !std::path::Path::new(CALL_FILE).exists() {
        let _ = std::fs::write(CALL_FILE, initial.map(|d| d.key()).unwrap_or("auto"));
    }
    let Some(c) = Ctl::open() else {
        println!("kein Tonchip");
        return 1;
    };
    let p = Paths::load();
    let mut dev = initial.unwrap_or_else(|| want_dev(Dev::Handset).unwrap_or(Dev::Handset));
    call_paths(&c, &p, dev, verbose);
    let rx = Stream::open(BB_RX, false, 480, 4).and_then(|s| s.start().map(|_| s));
    let tx = Stream::open(BB_TX, true, 480, 4).and_then(|s| s.start().map(|_| s));
    for (n, r) in [("Empfang", rx.as_ref().err()), ("Senden", tx.as_ref().err())] {
        match r {
            Some(e) => println!("Gesprächston {n}: {e}"),
            None => println!("Gesprächston {n}: läuft"),
        }
    }
    println!("Gesprächston auf {}", dev.key());
    let t0 = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        match want_dev(dev) {
            None => break,
            Some(d) if d != dev => {
                dev = d;
                call_paths(&c, &p, dev, verbose);
                println!("Gesprächston auf {}", dev.key());
            }
            _ => {}
        }
        // Sicherung: ohne Steuerdatei-Pflege höchstens 4 Stunden
        if t0.elapsed().as_secs() > 4 * 3600 {
            break;
        }
    }
    drop(rx);
    drop(tx);
    p.reset(&c, false);
    let _ = c.set("ABOX Audio Mode", "NORMAL");
    amp_off(&c);
    let _ = std::fs::remove_file(CALL_PID);
    println!("Gesprächston beendet");
    0
}
