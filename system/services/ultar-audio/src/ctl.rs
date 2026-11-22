//! ALSA-Regler (SNDRV_CTL) vollständig: auflisten, lesen, schreiben – Zahl, Schalter, Auswahl per Name.
//! Ein Aufruf liest die Liste einmal; `set` sucht den Regler nach GENAUEM Namen (wie Samsungs audio_route).

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

const fn ioc(dir: u64, ty: u8, nr: u64, sz: usize) -> u64 {
    (dir << 30) | ((sz as u64) << 16) | ((ty as u64) << 8) | nr
}
const LIST: u64 = ioc(3, b'U', 0x10, 80);
const INFO: u64 = ioc(3, b'U', 0x11, 272);
const READ: u64 = ioc(3, b'U', 0x12, 1224);
const WRITE: u64 = ioc(3, b'U', 0x13, 1224);

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

fn u32a(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn i64a(b: &[u8], o: usize) -> i64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[o..o + 8]);
    i64::from_le_bytes(x)
}
fn cstr(b: &[u8]) -> String {
    let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}

pub const T_BOOL: u32 = 1;
pub const T_INT: u32 = 2;
pub const T_ENUM: u32 = 3;
pub const T_INT64: u32 = 6;

pub struct Ctl {
    f: File,
    ids: Vec<[u8; 64]>,
    names: Vec<String>,
}

pub struct Info {
    pub ty: u32,
    pub count: usize,
    pub min: i64,
    pub max: i64,
    pub items: Vec<String>,
}

impl Ctl {
    pub fn open() -> Option<Ctl> {
        let f = OpenOptions::new().read(true).write(true).open("/dev/snd/controlC0").ok()?;
        let mut raw = vec![0u8; 64 * 2048];
        let mut list = [0u8; 80];
        list[4..8].copy_from_slice(&2048u32.to_le_bytes());
        list[16..24].copy_from_slice(&(raw.as_mut_ptr() as u64).to_le_bytes());
        if unsafe { ioctl(f.as_raw_fd(), LIST, list.as_mut_ptr()) } != 0 {
            return None;
        }
        let used = (u32a(&list, 8) as usize).min(2048);
        let mut ids = Vec::with_capacity(used);
        let mut names = Vec::with_capacity(used);
        for i in 0..used {
            let mut id = [0u8; 64];
            id.copy_from_slice(&raw[i * 64..i * 64 + 64]);
            names.push(cstr(&id[16..60]));
            ids.push(id);
        }
        Some(Ctl { f, ids, names })
    }

    fn idx(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.idx(name).is_some()
    }

    pub fn info_at(&self, i: usize) -> Option<Info> {
        let fd = self.f.as_raw_fd();
        let mut b = [0u8; 272];
        b[..64].copy_from_slice(&self.ids[i]);
        if unsafe { ioctl(fd, INFO, b.as_mut_ptr()) } != 0 {
            return None;
        }
        let ty = u32a(&b, 64);
        let count = (u32a(&b, 72) as usize).clamp(1, 128);
        let mut inf = Info { ty, count, min: 0, max: 0, items: Vec::new() };
        if ty == T_ENUM {
            let n = u32a(&b, 80).min(256);
            for k in 0..n {
                let mut q = [0u8; 272];
                q[..64].copy_from_slice(&self.ids[i]);
                q[84..88].copy_from_slice(&k.to_le_bytes());
                if unsafe { ioctl(fd, INFO, q.as_mut_ptr()) } != 0 {
                    break;
                }
                inf.items.push(cstr(&q[88..152]));
            }
        } else if ty == T_INT || ty == T_INT64 {
            inf.min = i64a(&b, 80);
            inf.max = i64a(&b, 88);
        } else if ty == T_BOOL {
            inf.max = 1;
        }
        Some(inf)
    }

    /// Werte als Text (Auswahl: Eintragsname).
    pub fn get_at(&self, i: usize) -> Option<Vec<String>> {
        let inf = self.info_at(i)?;
        let mut v = [0u8; 1224];
        v[..64].copy_from_slice(&self.ids[i]);
        if unsafe { ioctl(self.f.as_raw_fd(), READ, v.as_mut_ptr()) } != 0 {
            return None;
        }
        let n = inf.count.min(16);
        Some(match inf.ty {
            T_ENUM => (0..n)
                .map(|c| {
                    let k = u32a(&v, 72 + c * 4) as usize;
                    inf.items.get(k).cloned().unwrap_or_else(|| k.to_string())
                })
                .collect(),
            T_BOOL | T_INT | T_INT64 => (0..n).map(|c| i64a(&v, 72 + c * 8).to_string()).collect(),
            _ => vec!["?".into()],
        })
    }

    pub fn get(&self, name: &str) -> Option<Vec<String>> {
        self.get_at(self.idx(name)?)
    }

    /// Regler nach genauem Namen setzen. `val` = Zahl, "On"/"Off" oder Auswahl-Eintrag.
    pub fn set(&self, name: &str, val: &str) -> Result<(), String> {
        let i = self.idx(name).ok_or_else(|| format!("unbekannt: {name}"))?;
        let inf = self.info_at(i).ok_or_else(|| format!("INFO: {name}"))?;
        let mut v = [0u8; 1224];
        v[..64].copy_from_slice(&self.ids[i]);
        match inf.ty {
            T_ENUM => {
                let k = inf
                    .items
                    .iter()
                    .position(|it| it == val)
                    .or_else(|| val.parse::<usize>().ok().filter(|k| *k < inf.items.len()))
                    .ok_or_else(|| format!("{name}: kein Eintrag '{val}'"))?;
                for c in 0..inf.count {
                    v[72 + c * 4..76 + c * 4].copy_from_slice(&(k as u32).to_le_bytes());
                }
            }
            T_BOOL | T_INT | T_INT64 => {
                let x: i64 = match val {
                    "On" | "on" | "true" => 1,
                    "Off" | "off" | "false" => 0,
                    _ => val.parse().map_err(|_| format!("{name}: keine Zahl '{val}'"))?,
                };
                for c in 0..inf.count {
                    v[72 + c * 8..80 + c * 8].copy_from_slice(&x.to_le_bytes());
                }
            }
            t => return Err(format!("{name}: Typ {t} nicht unterstützt")),
        }
        if unsafe { ioctl(self.f.as_raw_fd(), WRITE, v.as_mut_ptr()) } != 0 {
            return Err(format!("{name}={val}: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Zahl-Regler anteilig (0..1) zwischen min und max setzen.
    pub fn set_frac(&self, name: &str, f: f32) -> bool {
        let Some(i) = self.idx(name) else { return false };
        let Some(inf) = self.info_at(i) else { return false };
        let x = inf.min + ((inf.max - inf.min) as f32 * f.clamp(0.0, 1.0)).round() as i64;
        self.set(name, &x.to_string()).is_ok()
    }

    pub fn dump(&self, filter: &str) {
        for (i, n) in self.names.iter().enumerate() {
            if !filter.is_empty() && !n.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
            let Some(inf) = self.info_at(i) else { continue };
            let cur = self.get_at(i).map(|v| v.join(",")).unwrap_or_default();
            match inf.ty {
                T_ENUM => println!("{n} = {cur}   [{}]", inf.items.join("|")),
                T_INT | T_INT64 => println!("{n} = {cur}   ({}..{})", inf.min, inf.max),
                T_BOOL => println!("{n} = {cur}   (bool)"),
                t => println!("{n}   (Typ {t}, {} Werte)", inf.count),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ioctl_numbers() {
        assert_eq!(LIST, 0xC050_5510);
        assert_eq!(INFO, 0xC110_5511);
        assert_eq!(READ, 0xC4C8_5512);
        assert_eq!(WRITE, 0xC4C8_5513);
    }
}
