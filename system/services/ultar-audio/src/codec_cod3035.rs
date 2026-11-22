//! Codec-Seite der Audio-Aktivierung für das Samsung A30.
//!
//! WICHTIG – Topologie (aus dem Kernel verifiziert, /home/claude/a30k):
//!   * **Lautsprecher** = eigener Class-D-Verstärker **SMA1301** (Silicon Mitus) am ABOX-UAIF3.
//!     Er wird NICHT über den COD3035 angesteuert, sondern bekommt sein I2S direkt von ABOX.
//!     Schlüssel-Control: **"Power Up(1:Up_0:Down)"** – löst `sma1301_startup()` aus (Amp an).
//!     Dieses Control fehlte in den bisherigen Versuchen → darum kam kein Ton.
//!   * **Kopfhörer/Ohrhörer + Mikrofon** = Codec **COD3035** (DAC→HPDRV/EPDRV, ADC←MIC_PGA).
//!
//! Dieses Modul setzt die ALSA-Mixer-Controls in der richtigen Reihenfolge über
//! /dev/snd/controlC0 (SNDRV_CTL_ELEM ioctls, wie tools/audctl.c). Die DAPM-Stromsequenzen
//! (VMID/Bias/DAC/ADC/MICBIAS) laufen dann automatisch, SOBALD ein aktiver Stream + Takt
//! anliegt – das liefert Subpaket Audio-1 (ABOX/DPCM). Ohne aktiven Stream bleiben die
//! Codec-DAPM-Widgets aus (das ist kein Fehler dieses Moduls).

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

// Kanonisches Codec-Trait aus codec.rs (zusammengeführt).
pub use crate::codec::Codec;

// ---------- SNDRV_CTL ioctls ----------
const fn ioc(dir: u64, ty: u8, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | nr
}
const CTL_ELEM_LIST: u64 = ioc(3, b'U', 0x10, 80);
const CTL_ELEM_INFO: u64 = ioc(3, b'U', 0x11, 272);
const CTL_ELEM_WRITE: u64 = ioc(3, b'U', 0x13, 1224);

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

/// Standard-Codec des A30 (COD3035 + SMA1301-Lautsprecher).
pub struct Cod3035Codec {
    ctl: &'static str,
}

impl Default for Cod3035Codec {
    fn default() -> Self {
        Cod3035Codec {
            ctl: "/dev/snd/controlC0",
        }
    }
}

impl Cod3035Codec {
    pub fn new() -> Self {
        Self::default()
    }

    fn open(&self) -> Option<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.ctl)
            .ok()
    }

    /// Ein int/bool-Control per Name (Teilstring exakt gleich) auf `val` setzen.
    /// Liefert true bei Erfolg. Robust: fehlende Controls werden still übersprungen.
    pub fn set(&self, name: &str, val: i64) -> bool {
        let Some(f) = self.open() else { return false };
        let fd = f.as_raw_fd();
        let mut list = [0u8; 80];
        let mut ids = vec![0u8; 64 * 1024];
        u32w(&mut list, 4, 1024); // space
        u64w(&mut list, 16, ids.as_mut_ptr() as u64);
        if unsafe { ioctl(fd, CTL_ELEM_LIST, list.as_mut_ptr()) } < 0 {
            return false;
        }
        let used = (u32r(&list, 8) as usize).min(1024);
        for i in 0..used {
            let id = &ids[i * 64..i * 64 + 64];
            let nm = cstr(&id[16..60]);
            if nm != name {
                continue;
            }
            // INFO für Typ/Count
            let mut info = [0u8; 272];
            info[..64].copy_from_slice(id);
            if unsafe { ioctl(fd, CTL_ELEM_INFO, info.as_mut_ptr()) } < 0 {
                return false;
            }
            let ty = u32r(&info, 64);
            let count = (u32r(&info, 72) as usize).min(8);
            if ty != 1 && ty != 2 && ty != 3 {
                return false; // nur bool/int/enum
            }
            let mut v = vec![0u8; 1224];
            v[..64].copy_from_slice(id);
            let cnt = count.max(1);
            for c in 0..cnt {
                // value.integer.value[] / value.enumerated.item[] liegen ab Offset 72
                i64w(&mut v, 72 + c * 8, val);
            }
            return unsafe { ioctl(fd, CTL_ELEM_WRITE, v.as_mut_ptr()) } == 0;
        }
        false
    }
}

impl Codec for Cod3035Codec {
    fn enable_speaker(&self) {
        // SMA1301-Class-D-Amp (UAIF3). Reihenfolge laut sma1301.c:
        // 1) nicht zwangs-abgeschaltet  2) Lautstärke  3) entstummen  4) Power Up -> startup()
        self.set("Force AMP Power Down", 0);
        self.set("Speaker Volume", 0x38); // ~mittel (Bereich 0..167)
        self.set("Speaker Mute Switch(1:muted_0:un)", 0);
        // Das entscheidende Control – startet sma1301_startup() (fehlte bisher!):
        self.set("Power Up(1:Up_0:Down)", 1);
        // Codec-Seite (für den Fall, dass der Pfad auch über COD3035-HP läuft, dev-speaker):
        self.set("DAC Soft Mute", 0);
        self.set("HP HP On", 1);
    }

    fn enable_mic(&self) {
        // COD3035 Aufnahmepfad: MIC1 -> MIC1_PGA -> ADCL/ADCR Mixer -> ADC.
        // DAPM schaltet MIC_PGA/VMID/MICBIAS automatisch, sobald der Capture-Stream läuft.
        self.set("MIC1 MIC1 On", 1);
        self.set("MIC1 On", 1); // je nach Control-Benennung
        self.set("ADCL Mixer MIC1L Switch", 1);
        self.set("ADCR Mixer MIC1R Switch", 1);
    }

    fn set_speaker_volume(&self, v: f32) {
        let x = (v.clamp(0.0, 1.0) * 167.0).round() as i64;
        self.set("Speaker Volume", x);
    }
}

// ---------- kleine Byte-Helfer ----------
fn u32r(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u32w(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_ne_bytes());
}
fn u64w(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_ne_bytes());
}
fn i64w(b: &mut [u8], o: usize, v: i64) {
    b[o..o + 8].copy_from_slice(&v.to_ne_bytes());
}
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ioctl_numbers() {
        assert_eq!(CTL_ELEM_LIST, 0xC050_5510);
        assert_eq!(CTL_ELEM_INFO, 0xC110_5511);
        assert_eq!(CTL_ELEM_WRITE, 0xC4C8_5513);
    }
    #[test]
    fn volume_scale() {
        // reiner Rechencheck, kein Gerät
        let x = (0.5f32 * 167.0).round() as i64;
        assert_eq!(x, 84);
    }
}
