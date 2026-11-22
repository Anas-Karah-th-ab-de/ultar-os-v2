//! Mikrofon: Aufnahme direkt über ALSA (/dev/snd), ohne Zusatzprogramme.
//!
//! A30 (Exynos-ABOX + COD3035): Hauptmikrofon MIC1 → ADC → UAIF0 → NSRC0 → WDMA1 = `pcmC0D9c`
//! (Weg wie in Samsungs `mixer_paths.xml`: `route-ap-record` + `dev-main-mic`).
//! Wichtig: Der Puffer muss ein ganzes Vielfaches der Periode sein (hier 960 × 8 Rahmen). Lässt man die
//! Größe offen, wählt der Treiber 131072 Byte – kein Vielfaches –, und der Lesezeiger läuft davon:
//! man liest dann in Sekundenbruchteilen „mehrere Sekunden" alten Pufferinhalt statt Ton.
//!
//! Geliefert wird Mono, 16 kHz, 16 Bit in Blöcken von 20 ms – das Format für die Spracherkennung.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

const fn ioc(dir: u64, ty: u8, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | nr
}
const CTL_LIST: u64 = ioc(3, b'U', 0x10, 80);
const CTL_INFO: u64 = ioc(3, b'U', 0x11, 272);
const CTL_WRITE: u64 = ioc(3, b'U', 0x13, 1224);
const PCM_HW_PARAMS: u64 = ioc(3, b'A', 0x11, 608);
const PCM_PREPARE: u64 = ioc(0, b'A', 0x40, 0);
const PCM_START: u64 = ioc(0, b'A', 0x42, 0);
const PCM_READI: u64 = ioc(2, b'A', 0x51, 24);

pub const RATE: u32 = 16_000;
/// Abtastwerte je geliefertem Block (20 ms)
pub const BLOCK: usize = 320;
const DEV: &str = "/dev/snd/pcmC0D9c";
const PERIOD: usize = 960;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Regler nach genauem Namen setzen; `item` = Zahl oder Name eines Auswahl-Eintrags.
fn ctl_set(f: &File, ids: &[u8], name: &str, item: &str) -> bool {
    let fd = f.as_raw_fd();
    for id in ids.chunks_exact(64) {
        let nm = &id[16..60];
        let end = nm.iter().position(|c| *c == 0).unwrap_or(nm.len());
        if &nm[..end] != name.as_bytes() {
            continue;
        }
        let mut info = [0u8; 272];
        info[..64].copy_from_slice(id);
        if unsafe { ioctl(fd, CTL_INFO, info.as_mut_ptr()) } != 0 {
            return false;
        }
        let (ty, count) = (u32_at(&info, 64), (u32_at(&info, 72) as usize).clamp(1, 8));
        let mut val = [0u8; 1224];
        val[..64].copy_from_slice(id);
        if ty == 3 {
            // Auswahl: Eintrag über seinen Namen finden
            let mut found = item.parse::<u32>().ok();
            for k in 0..u32_at(&info, 80).min(64) {
                let mut q = [0u8; 272];
                q[..64].copy_from_slice(id);
                q[84..88].copy_from_slice(&k.to_le_bytes());
                if unsafe { ioctl(fd, CTL_INFO, q.as_mut_ptr()) } != 0 {
                    break;
                }
                let e = &q[88..152];
                let n = e.iter().position(|c| *c == 0).unwrap_or(e.len());
                if &e[..n] == item.as_bytes() {
                    found = Some(k);
                    break;
                }
            }
            let Some(k) = found else { return false };
            for c in 0..count {
                val[72 + c * 4..76 + c * 4].copy_from_slice(&k.to_le_bytes());
            }
        } else {
            let v: i64 = item.parse().unwrap_or(0);
            for c in 0..count {
                val[72 + c * 8..80 + c * 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        return unsafe { ioctl(fd, CTL_WRITE, val.as_mut_ptr()) } == 0;
    }
    false
}

/// Aufnahmeweg ein-/ausschalten. Liefert die Zahl gesetzter Regler (0 = kein passender Tonchip).
pub fn route(on: bool) -> usize {
    let Ok(f) = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/snd/controlC0")
    else {
        return 0;
    };
    let mut ids = vec![0u8; 64 * 1024];
    let mut list = [0u8; 80];
    list[4..8].copy_from_slice(&1024u32.to_le_bytes());
    list[16..24].copy_from_slice(&(ids.as_mut_ptr() as u64).to_le_bytes());
    if unsafe { ioctl(f.as_raw_fd(), CTL_LIST, list.as_mut_ptr()) } != 0 {
        return 0;
    }
    ids.truncate((u32_at(&list, 8) as usize).min(1024) * 64);
    let sw = if on { "1" } else { "0" };
    let steps: [(&str, &str); 11] = [
        ("ABOX SPUM ASRC0", "On"),
        ("ABOX SPUM ASRC1", "On"),
        ("ABOX SPUM ASRC2", "On"),
        ("ABOX SPUM ASRC3", "On"),
        ("ABOX NSRC0", if on { "UAIF0" } else { "RESERVED" }),
        ("ADCL Mixer MIC1L Switch", sw),
        ("ADCR Mixer MIC1R Switch", sw),
        ("MIC1 MIC1 On", sw),
        ("MIC1 Boost Volume", "2"),
        ("ADC Left Gain", "127"),
        ("ADC Right Gain", "127"),
    ];
    steps
        .iter()
        .filter(|(n, v)| ctl_set(&f, &ids, n, v))
        .count()
}

pub fn available() -> bool {
    std::path::Path::new(DEV).exists()
}

fn hw_params() -> [u8; 608] {
    let mut p = [0u8; 608];
    let put = |p: &mut [u8], o: usize, v: u32| p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut p, 4, 1 << 3); // RW_INTERLEAVED
    put(&mut p, 4 + 32, 1 << 2); // S16_LE
    put(&mut p, 4 + 64, 1);
    let base = 4 + 8 * 32;
    for i in 0..12 {
        put(&mut p, base + i * 12 + 4, u32::MAX);
    }
    let fix = |p: &mut [u8], i: usize, v: u32| {
        put(p, base + i * 12, v);
        put(p, base + i * 12 + 4, v);
        put(p, base + i * 12 + 8, 4);
    };
    fix(&mut p, 0, 16);
    fix(&mut p, 1, 32);
    fix(&mut p, 2, 2);
    fix(&mut p, 3, 48_000);
    fix(&mut p, 5, PERIOD as u32); // Periode
    fix(&mut p, 7, 8); // Perioden je Puffer – ganzes Vielfaches, siehe oben
    put(&mut p, base + 12 * 12 + 9 * 12, u32::MAX);
    p
}

/// 48 kHz Stereo → 16 kHz Mono (Kanäle mitteln, je drei Werte zusammenfassen).
pub fn downmix(stereo48: &[i16], out: &mut Vec<i16>) {
    for c in stereo48.chunks_exact(6) {
        let s: i32 = c.iter().map(|v| *v as i32).sum();
        out.push((s / 6) as i16);
    }
}

/// Laufende Aufnahme. Blöcke von 20 ms kommen über `rx`; beim Fallenlassen endet sie und der Weg geht aus.
pub struct Mic {
    stop: Arc<AtomicBool>,
    pub rx: Receiver<Vec<i16>>,
}

impl Mic {
    pub fn start() -> Result<Mic, String> {
        if route(true) == 0 {
            return Err("kein Aufnahmeweg (Tonchip unbekannt)".into());
        }
        let f = OpenOptions::new()
            .read(true)
            .open(DEV)
            .map_err(|e| format!("{DEV}: {e}"))?;
        let fd = f.as_raw_fd();
        let mut hp = hw_params();
        if unsafe { ioctl(fd, PCM_HW_PARAMS, hp.as_mut_ptr()) } != 0 {
            return Err(format!(
                "{DEV}: Format abgelehnt ({})",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { ioctl(fd, PCM_PREPARE) } != 0 {
            return Err(format!(
                "{DEV}: PREPARE ({})",
                std::io::Error::last_os_error()
            ));
        }
        unsafe { ioctl(fd, PCM_START) };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let st = stop.clone();
        std::thread::spawn(move || {
            let f = f;
            let mut buf = vec![0i16; PERIOD * 2];
            let mut out = Vec::with_capacity(BLOCK);
            while !st.load(Ordering::Relaxed) {
                let mut x = [0u8; 24];
                x[8..16].copy_from_slice(&(buf.as_mut_ptr() as u64).to_le_bytes());
                x[16..24].copy_from_slice(&(PERIOD as u64).to_le_bytes());
                if unsafe { ioctl(f.as_raw_fd(), PCM_READI, x.as_mut_ptr()) } != 0 {
                    // Überlauf: neu ansetzen
                    if unsafe { ioctl(f.as_raw_fd(), PCM_PREPARE) } != 0 {
                        break;
                    }
                    unsafe { ioctl(f.as_raw_fd(), PCM_START) };
                    continue;
                }
                out.clear();
                downmix(&buf, &mut out);
                if tx.send(out.clone()).is_err() {
                    break;
                }
            }
            drop(f);
            route(false);
        });
        Ok(Mic { stop, rx })
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// WAV (16 kHz, Mono, 16 Bit) schreiben.
pub fn wav(samples: &[i16]) -> Vec<u8> {
    let n = samples.len() as u32 * 2;
    let mut o = Vec::with_capacity(44 + n as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + n).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&RATE.to_le_bytes());
    o.extend_from_slice(&(RATE * 2).to_le_bytes());
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&n.to_le_bytes());
    for s in samples {
        o.extend_from_slice(&s.to_le_bytes());
    }
    o
}

pub fn rms(s: &[i16]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / s.len() as f32).sqrt()
}

/// Prüfung von der Kommandozeile: `ultar-shell --mic <ms> [datei.wav]` – Pegel je Viertelsekunde.
pub fn test_cli(ms: u64, file: Option<&str>) -> i32 {
    let mic = match Mic::start() {
        Ok(m) => m,
        Err(e) => {
            println!("Mikrofon: {e}");
            return 1;
        }
    };
    let t0 = std::time::Instant::now();
    let mut all: Vec<i16> = Vec::new();
    while (all.len() as u64) < ms * RATE as u64 / 1000 {
        match mic.rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(b) => all.extend_from_slice(&b),
            Err(_) => {
                println!("Mikrofon: keine Daten");
                return 1;
            }
        }
    }
    let levels: Vec<String> = all
        .chunks(RATE as usize / 4)
        .map(|c| format!("{:.0}", rms(c)))
        .collect();
    println!(
        "Mikrofon: {} Werte in {} ms, RMS {:.1}, Spitze {}",
        all.len(),
        t0.elapsed().as_millis(),
        rms(&all),
        all.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0)
    );
    println!("Verlauf: {}", levels.join(" "));
    if let Some(p) = file {
        if let Err(e) = std::fs::write(p, wav(&all)) {
            println!("{p}: {e}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_and_wav() {
        let mut out = Vec::new();
        downmix(&[6, 6, 6, 6, 6, 6, -12, 0, -12, 0, -12, 0, 1, 2], &mut out);
        assert_eq!(out, vec![6, -6]);
        let w = wav(&[1, -1]);
        assert_eq!(w.len(), 48);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32_at(&w, 24), 16_000);
        assert_eq!(PCM_READI, 0x8018_4151);
    }
}

/// `ultar-shell --mic-roh`: Mikrofon roh (16 kHz, Mono, s16le) auf stdout, bis der Leser aufhört (Ultar-Verbund: Mithören)
pub fn roh_cli() -> i32 {
    use std::io::Write;
    let m = match Mic::start() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Mikrofon: {e}");
            return 1;
        }
    };
    let mut out = std::io::stdout().lock();
    while let Ok(b) = m.rx.recv() {
        let bytes: Vec<u8> = b.iter().flat_map(|s| s.to_le_bytes()).collect();
        if out.write_all(&bytes).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    0
}
