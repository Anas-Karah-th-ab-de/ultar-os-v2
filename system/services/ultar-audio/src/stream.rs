//! Offener PCM-Strom (Wiedergabe oder Aufnahme) mit festem Format: 48 kHz, Stereo, 16 Bit.
//! Puffer = Periode × Perioden genau (sonst läuft beim ABOX der Lesezeiger davon, siehe mic.rs).
//! Für den Anruf öffnet man die Basisband-Strecken nur und startet sie – Daten fließen im ABOX
//! direkt zwischen Modem und Tonchip (wie Samsungs audio_proxy: voice_rx/voice_tx).

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

const fn ioc(dir: u64, ty: u8, nr: u64, sz: usize) -> u64 {
    (dir << 30) | ((sz as u64) << 16) | ((ty as u64) << 8) | nr
}
const HW_PARAMS: u64 = ioc(3, b'A', 0x11, 608);
const SW_PARAMS: u64 = ioc(3, b'A', 0x13, 136);
const PREPARE: u64 = ioc(0, b'A', 0x40, 0);
const START: u64 = ioc(0, b'A', 0x42, 0);
const DROP: u64 = ioc(0, b'A', 0x43, 0);
const READI: u64 = ioc(2, b'A', 0x51, 24);

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

pub const RATE: u32 = 48_000;

pub struct Stream {
    f: File,
    pub path: String,
    pub period: usize,
}

fn hw(period: u32, periods: u32, fixed: bool) -> [u8; 608] {
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
    fix(&mut p, 3, RATE);
    if fixed {
        fix(&mut p, 5, period);
        fix(&mut p, 7, periods);
    }
    put(&mut p, base + 12 * 12 + 9 * 12, u32::MAX); // rmask
    p
}

/// Software-Parameter: Wiedergabe läuft weiter, auch wenn niemand Daten liefert (kein Abbruch bei Leerlauf).
fn sw_silence(fd: i32, buffer: u64) {
    let mut s = [0u8; 136];
    // tstamp_mode(4) period_step(4) sleep_min(4) pad(4) avail_min(8) xfer_align(8) start_threshold(8)
    // stop_threshold(8) silence_threshold(8) silence_size(8) boundary(8)
    s[4..8].copy_from_slice(&1u32.to_le_bytes());
    s[16..24].copy_from_slice(&1u64.to_le_bytes());
    s[24..32].copy_from_slice(&1u64.to_le_bytes());
    s[32..40].copy_from_slice(&1u64.to_le_bytes()); // start_threshold
    let _ = buffer;
    let big = i64::MAX as u64; // >= boundary des Kerns: nie stoppen, Puffer immer mit Stille füllen
    s[40..48].copy_from_slice(&big.to_le_bytes()); // stop_threshold
    s[48..56].copy_from_slice(&0u64.to_le_bytes()); // silence_threshold
    s[56..64].copy_from_slice(&big.to_le_bytes()); // silence_size
    unsafe { ioctl(fd, SW_PARAMS, s.as_mut_ptr()) };
}

impl Stream {
    pub fn open(dev: u32, capture: bool, period: u32, periods: u32) -> Result<Stream, String> {
        let path = format!("/dev/snd/pcmC0D{dev}{}", if capture { 'c' } else { 'p' });
        let f = OpenOptions::new()
            .read(capture)
            .write(!capture)
            .open(&path)
            .map_err(|e| format!("{path}: {e}"))?;
        let fd = f.as_raw_fd();
        let mut hp = hw(period, periods, true);
        if unsafe { ioctl(fd, HW_PARAMS, hp.as_mut_ptr()) } != 0 {
            // Treiber will andere Periodengröße: frei wählen lassen
            hp = hw(0, 0, false);
            if unsafe { ioctl(fd, HW_PARAMS, hp.as_mut_ptr()) } != 0 {
                return Err(format!("{path}: HW_PARAMS {}", std::io::Error::last_os_error()));
            }
        }
        let base = 4 + 8 * 32;
        let rd = |i: usize| u32::from_le_bytes([hp[base + i * 12], hp[base + i * 12 + 1], hp[base + i * 12 + 2], hp[base + i * 12 + 3]]);
        let per = rd(5) as usize;
        let buf = rd(9) as u64;
        if !capture {
            sw_silence(fd, buf);
        }
        if unsafe { ioctl(fd, PREPARE) } != 0 {
            return Err(format!("{path}: PREPARE {}", std::io::Error::last_os_error()));
        }
        Ok(Stream { f, path, period: per.max(1) })
    }

    pub fn start(&self) -> Result<(), String> {
        if unsafe { ioctl(self.f.as_raw_fd(), START) } != 0 {
            return Err(format!("{}: START {}", self.path, std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Einen Block (Rahmen = 2 Werte) lesen; Überlauf → neu ansetzen.
    pub fn read(&self, buf: &mut [i16]) -> Result<usize, String> {
        let frames = buf.len() / 2;
        let mut x = [0u8; 24];
        x[8..16].copy_from_slice(&(buf.as_mut_ptr() as u64).to_le_bytes());
        x[16..24].copy_from_slice(&(frames as u64).to_le_bytes());
        if unsafe { ioctl(self.f.as_raw_fd(), READI, x.as_mut_ptr()) } != 0 {
            let e = std::io::Error::last_os_error();
            unsafe {
                ioctl(self.f.as_raw_fd(), PREPARE);
                ioctl(self.f.as_raw_fd(), START);
            }
            return Err(format!("{}: {e}", self.path));
        }
        let mut n = [0u8; 8];
        n.copy_from_slice(&x[0..8]);
        Ok(i64::from_le_bytes(n).max(0) as usize)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { ioctl(self.f.as_raw_fd(), DROP) };
    }
}

/// Aufnahme `ms` lang von pcmC0D<dev>c; liefert Stereo-Werte. Bricht nach 2 s ohne Daten ab.
pub fn record(dev: u32, ms: u32) -> Result<Vec<i16>, String> {
    let s = Stream::open(dev, true, 960, 8)?;
    s.start()?;
    let want = (RATE as usize * ms as usize / 1000) * 2;
    let mut all = Vec::with_capacity(want);
    let mut buf = vec![0i16; s.period * 2];
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<i16>, String>>();
    std::thread::spawn(move || {
        let s = s;
        loop {
            let r = s.read(&mut buf).map(|n| buf[..n * 2].to_vec());
            let stop = r.is_err();
            if tx.send(r).is_err() || stop {
                break;
            }
        }
    });
    while all.len() < want {
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(b)) => all.extend_from_slice(&b),
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(format!("keine Daten nach {} Werten (Strom läuft nicht)", all.len())),
        }
    }
    all.truncate(want);
    Ok(all)
}

pub fn rms(s: &[i16]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / s.len() as f32).sqrt()
}

/// WAV, 48 kHz Stereo 16 Bit.
pub fn wav(s: &[i16]) -> Vec<u8> {
    let n = s.len() as u32 * 2;
    let mut o = Vec::with_capacity(44 + n as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + n).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&RATE.to_le_bytes());
    o.extend_from_slice(&(RATE * 4).to_le_bytes());
    o.extend_from_slice(&4u16.to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&n.to_le_bytes());
    for v in s {
        o.extend_from_slice(&v.to_le_bytes());
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers() {
        assert_eq!(READI, 0x8018_4151);
        assert_eq!(SW_PARAMS, 0xC088_4113);
        assert_eq!(wav(&[1, 2]).len(), 48);
    }
}
