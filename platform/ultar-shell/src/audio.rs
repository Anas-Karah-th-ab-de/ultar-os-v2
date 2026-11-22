//! Tonausgabe direkt über die ALSA-Schnittstelle des Kernels (/dev/snd), ohne Zusatzprogramme.
//! Mischer: Wiedergabe-Schalter einschalten und Lautstärkeregler hochdrehen (Treiber starten oft stumm).
//! Wiedergabe: 48 kHz, 16 Bit, Stereo; läuft in einem eigenen Faden, damit die Oberfläche nicht stockt.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

const fn ioc(dir: u64, ty: u8, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | nr
}

// ---- PCM ----
const HW_PARAMS_SIZE: usize = 608;
const PCM_HW_PARAMS: u64 = ioc(3, b'A', 0x11, HW_PARAMS_SIZE);
const PCM_PREPARE: u64 = ioc(0, b'A', 0x40, 0);
const PCM_DRAIN: u64 = ioc(0, b'A', 0x44, 0);
const RATE: u32 = 48_000;

/// snd_pcm_hw_params aufbauen: alles offen, nur Zugriff, Format, Kanäle und Rate festgelegt.
fn hw_params() -> [u8; HW_PARAMS_SIZE] {
    let mut p = [0u8; HW_PARAMS_SIZE];
    let put = |p: &mut [u8], o: usize, v: u32| p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    // 3 Masken (je 32 Byte) ab Byte 4: Zugriff, Format, Unterformat
    for m in 0..3 {
        for w in 0..8 {
            put(&mut p, 4 + m * 32 + w * 4, 0);
        }
    }
    put(&mut p, 4, 1 << 3); // Zugriff: RW_INTERLEAVED
    put(&mut p, 4 + 32, 1 << 2); // Format: S16_LE
    put(&mut p, 4 + 64, 1); // Unterformat: STD
                            // 12 Bereiche (je 12 Byte: min, max, Schalter) ab Byte 260
    let base = 4 + 8 * 32;
    for i in 0..12 {
        put(&mut p, base + i * 12, 0);
        put(&mut p, base + i * 12 + 4, u32::MAX);
        put(&mut p, base + i * 12 + 8, 0);
    }
    let fix = |p: &mut [u8], i: usize, v: u32| {
        put(p, base + i * 12, v);
        put(p, base + i * 12 + 4, v);
        put(p, base + i * 12 + 8, 4); // ganzzahlig
    };
    fix(&mut p, 0, 16); // Bits je Abtastwert
    fix(&mut p, 1, 32); // Bits je Rahmen
    fix(&mut p, 2, 2); // Kanäle
    fix(&mut p, 3, RATE);
    // Periodengröße eingrenzen: ohne Vorgabe wählt der Kernel die kleinste (8 Rahmen) – das gibt tausende
    // Unterbrechungen je Sekunde und Aussetzer. 512–4096 Rahmen = rund 10–85 ms.
    put(&mut p, base + 5 * 12, 512);
    put(&mut p, base + 5 * 12 + 4, 4096);
    // rmask (alle Angaben prüfen) hinter den Bereichen und ihrer Reserve
    put(&mut p, base + 12 * 12 + 9 * 12, u32::MAX);
    p
}

// ---- Mischer ----
const ID: usize = 64;
const LIST_SIZE: usize = 80;
const INFO_SIZE: usize = 272;
const VALUE_SIZE: usize = 1224;
const CTL_ELEM_LIST: u64 = ioc(3, b'U', 0x10, LIST_SIZE);
const CTL_ELEM_INFO: u64 = ioc(3, b'U', 0x11, INFO_SIZE);
const CTL_ELEM_WRITE: u64 = ioc(3, b'U', 0x13, VALUE_SIZE);

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn i64_at(b: &[u8], o: usize) -> i64 {
    i64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Alle Wiedergabe-Schalter ein, alle Wiedergabe-Regler auf `level` (0..1). Liefert die Zahl gesetzter Regler.
pub fn mixer_setup(level: f32) -> usize {
    // Exynos-ABOX (A30): Lautsprecherweg und Verstärker (SMA1301) schaltet ultar-audio – generische Schalter reichen dort nicht
    if std::path::Path::new("/sys/devices/platform/14a50000.abox").exists() {
        for bin in ["/usr/bin/ultar-audio", "/data/ultar/tt/ultar-audio"] {
            if std::path::Path::new(bin).exists() {
                let ok = std::process::Command::new(bin)
                    .args(["--route", &format!("{level:.2}")])
                    .stdout(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                return ok as usize;
            }
        }
    }
    let mut done = 0;
    for card in 0..4 {
        let Ok(f) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/snd/controlC{card}"))
        else {
            continue;
        };
        let fd = f.as_raw_fd();
        let mut ids = vec![0u8; ID * 256];
        let mut list = [0u8; LIST_SIZE];
        list[4..8].copy_from_slice(&256u32.to_le_bytes()); // space
        list[16..24].copy_from_slice(&(ids.as_mut_ptr() as u64).to_le_bytes());
        if unsafe { ioctl(fd, CTL_ELEM_LIST, list.as_mut_ptr()) } != 0 {
            continue;
        }
        let used = (u32_at(&list, 8) as usize).min(256);
        for i in 0..used {
            let id = &ids[i * ID..(i + 1) * ID];
            let name = String::from_utf8_lossy(&id[16..60])
                .trim_end_matches('\0')
                .to_string();
            let (is_switch, is_volume) = (
                name.ends_with("Playback Switch"),
                name.ends_with("Playback Volume"),
            );
            if !(is_switch || is_volume)
                || name.contains("Mic")
                || name.contains("Capture")
                || name.contains("Beep")
            {
                continue;
            }
            let mut info = [0u8; INFO_SIZE];
            info[..ID].copy_from_slice(id);
            if unsafe { ioctl(fd, CTL_ELEM_INFO, info.as_mut_ptr()) } != 0 {
                continue;
            }
            let (ty, count) = (u32_at(&info, 64), (u32_at(&info, 72) as usize).min(8));
            let mut val = [0u8; VALUE_SIZE];
            val[..ID].copy_from_slice(id);
            let v: i64 = match ty {
                1 if is_switch => 1, // Schalter
                2 if is_volume => {
                    let (min, max) = (i64_at(&info, 80), i64_at(&info, 88));
                    min + ((max - min) as f32 * level.clamp(0.0, 1.0)).round() as i64
                }
                _ => continue,
            };
            for c in 0..count {
                val[72 + c * 8..80 + c * 8].copy_from_slice(&v.to_le_bytes());
            }
            if unsafe { ioctl(fd, CTL_ELEM_WRITE, val.as_mut_ptr()) } == 0 {
                done += 1;
            }
        }
    }
    done
}

/// true, wenn es ein Wiedergabegerät gibt.
pub fn available() -> bool {
    (0..4).any(|c| std::path::Path::new(&format!("/dev/snd/pcmC{c}D0p")).exists())
}

/// Tonfolge erzeugen: (Frequenz in Hz, Dauer in ms); Frequenz 0 = Pause. Weiche Ein-/Ausblendung gegen Knacken.
pub fn tones(seq: &[(f32, u32)], volume: f32) -> Vec<u8> {
    let amp = 26_000.0 * volume.clamp(0.0, 1.0);
    let mut out = Vec::new();
    for &(hz, ms) in seq {
        let n = (RATE as u64 * ms as u64 / 1000) as usize;
        let fade = (RATE as usize / 200).min(n / 2).max(1);
        for i in 0..n {
            let env = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
            // Phase in f64 rechnen: in f32 verliert das Produkt bei langen Tönen die Genauigkeit (falsche Tonhöhe)
            let s = if hz > 0.0 {
                ((i as f64 * hz as f64 * std::f64::consts::TAU / RATE as f64).sin() as f32
                    * amp
                    * env) as i16
            } else {
                0
            };
            out.extend_from_slice(&s.to_le_bytes());
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out
}

static BUSY: AtomicBool = AtomicBool::new(false);

fn play_blocking(pcm: &[u8]) -> Result<(), String> {
    let mut last = "kein Wiedergabegerät".to_string();
    for card in 0..4 {
        let path = format!("/dev/snd/pcmC{card}D0p");
        let mut f = match OpenOptions::new().write(true).open(&path) {
            Ok(f) => f,
            Err(e) => {
                if std::path::Path::new(&path).exists() {
                    last = format!("{path}: {e}");
                }
                continue;
            }
        };
        let mut hp = hw_params();
        if unsafe { ioctl(f.as_raw_fd(), PCM_HW_PARAMS, hp.as_mut_ptr()) } != 0 {
            last = format!(
                "{path}: Format 48 kHz/16 Bit/Stereo abgelehnt ({})",
                std::io::Error::last_os_error()
            );
            continue;
        }
        if std::env::var("ULTAR_AUDIO_DEBUG").is_ok() {
            let iv = |i: usize| (u32_at(&hp, 260 + i * 12), u32_at(&hp, 260 + i * 12 + 4));
            println!(
                "{path}: bits {:?} rahmenbits {:?} kanäle {:?} rate {:?} periode {:?} puffer {:?}",
                iv(0),
                iv(1),
                iv(2),
                iv(3),
                iv(5),
                iv(9)
            );
        }
        unsafe { ioctl(f.as_raw_fd(), PCM_PREPARE) };
        f.write_all(pcm).map_err(|e| format!("{path}: {e}"))?;
        unsafe { ioctl(f.as_raw_fd(), PCM_DRAIN) };
        return Ok(());
    }
    Err(last)
}

/// Läuft gerade eine Wiedergabe? (Das Mikrofon soll eigene Töne nicht für Sprache halten.)
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

/// Tonfolge im Hintergrund abspielen. Läuft schon eine, wird die neue verworfen (kein Stau).
pub fn play(seq: Vec<(f32, u32)>, volume: f32) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        mixer_setup(0.85);
        if let Err(e) = play_blocking(&tones(&seq, volume)) {
            eprintln!("Ton: {e}");
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Weckton: drei aufsteigende Töne
pub fn alarm(volume: f32) {
    play(
        vec![
            (880.0, 180),
            (0.0, 60),
            (1175.0, 180),
            (0.0, 60),
            (1568.0, 320),
            (0.0, 200),
        ],
        volume,
    );
}

/// Kurzer Hinweiston
pub fn notify(volume: f32) {
    play(vec![(1175.0, 120), (0.0, 40), (1568.0, 160)], volume);
}

/// Testton für die Kommandozeile: `ultar-shell --beep`
pub fn test_cli() -> i32 {
    let n = mixer_setup(0.85);
    println!("Mischer: {n} Regler/Schalter gesetzt");
    match play_blocking(&tones(
        &[
            (880.0, 300),
            (0.0, 80),
            (1175.0, 300),
            (0.0, 80),
            (1568.0, 500),
        ],
        0.8,
    )) {
        Ok(()) => {
            println!("Ton OK");
            0
        }
        Err(e) => {
            println!("Ton FEHLER: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_tones() {
        assert_eq!(PCM_HW_PARAMS, 0xC260_4111);
        assert_eq!(PCM_PREPARE, 0x4140);
        assert_eq!(CTL_ELEM_LIST, 0xC050_5510);
        assert_eq!(CTL_ELEM_INFO, 0xC110_5511);
        assert_eq!(CTL_ELEM_WRITE, 0xC4C8_5513);
        let p = hw_params();
        assert_eq!(u32_at(&p, 4), 8); // RW_INTERLEAVED
        assert_eq!(u32_at(&p, 260 + 3 * 12), 48_000); // Rate min
        assert_eq!(u32_at(&p, 512), u32::MAX); // rmask
        let t = tones(&[(1000.0, 100), (0.0, 50)], 1.0);
        assert_eq!(t.len(), (4800 + 2400) * 4);
        assert!(t[..4800 * 4]
            .chunks(2)
            .any(|s| i16::from_le_bytes([s[0], s[1]]).abs() > 20_000));
        assert!(t[4800 * 4..].iter().all(|&b| b == 0));
        // Tonhöhe prüfen: 1000 Hz über 100 ms = 100 Schwingungen (Nulldurchgänge von unten nach oben)
        let left: Vec<i16> = t[..4800 * 4]
            .chunks(4)
            .map(|s| i16::from_le_bytes([s[0], s[1]]))
            .collect();
        let z = left.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        assert!((99..=101).contains(&z), "Nulldurchgänge: {z}");
        let long = tones(&[(1568.0, 500)], 1.0);
        let l2: Vec<i16> = long
            .chunks(4)
            .map(|s| i16::from_le_bytes([s[0], s[1]]))
            .collect();
        let z2 = l2[12000..]
            .windows(2)
            .filter(|w| w[0] < 0 && w[1] >= 0)
            .count();
        assert!((390..=394).contains(&z2), "späte Nulldurchgänge: {z2}");
    }
}

/// `ultar-shell --ton-roh`: stdin (16 kHz, Mono, s16le) über den Lautsprecher ausgeben (Ultar-Verbund: Durchsage).
/// Hochrechnen auf 48 kHz Stereo (je Abtastwert dreimal, beide Kanäle).
pub fn roh_cli() -> i32 {
    use std::io::Read;
    mixer_setup(0.85);
    for card in 0..4 {
        let path = format!("/dev/snd/pcmC{card}D0p");
        let Ok(mut f) = OpenOptions::new().write(true).open(&path) else {
            continue;
        };
        let mut hp = hw_params();
        if unsafe { ioctl(f.as_raw_fd(), PCM_HW_PARAMS, hp.as_mut_ptr()) } != 0 {
            continue;
        }
        unsafe { ioctl(f.as_raw_fd(), PCM_PREPARE) };
        let mut inp = std::io::stdin().lock();
        let mut b = [0u8; 640];
        loop {
            let n = match inp.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => n & !1,
            };
            let mut out = Vec::with_capacity(n * 6);
            for s in b[..n].chunks_exact(2) {
                for _ in 0..3 {
                    out.extend_from_slice(s);
                    out.extend_from_slice(s);
                }
            }
            if f.write_all(&out).is_err() {
                // Unterlauf: neu ansetzen
                unsafe { ioctl(f.as_raw_fd(), PCM_PREPARE) };
            }
        }
        unsafe { ioctl(f.as_raw_fd(), PCM_DRAIN) };
        return 0;
    }
    eprintln!("Ton: kein Wiedergabegerät");
    1
}
