//! ALSA-Control-Zugriff (SNDRV_CTL) – setzt die ABOX-Routing- und Codec-Controls per Name.
//! Enum-Controls (SPUS/SIFS/UAIF) werden über die Item-Namen gesetzt.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

const fn ioc(dir: u64, ty: u8, nr: u64, sz: usize) -> u64 {
    (dir << 30) | ((sz as u64) << 16) | ((ty as u64) << 8) | nr
}
const CTL_LIST: u64 = ioc(3, b'U', 0x10, 80);
const CTL_INFO: u64 = ioc(3, b'U', 0x11, 272);
#[allow(dead_code)] // not used by anything yet; kept, see docs/OFFEN-WARNUNGEN.md
const CTL_READ: u64 = ioc(3, b'U', 0x12, 1224);
const CTL_WRITE: u64 = ioc(3, b'U', 0x13, 1224);

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

fn u32a(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub struct Mixer {
    f: Option<File>,
}

impl Mixer {
    pub fn open() -> Self {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/snd/controlC0")
            .ok();
        Mixer { f }
    }

    #[allow(dead_code)] // not used by anything yet; kept, see docs/OFFEN-WARNUNGEN.md
    fn each<R>(&self, mut cb: impl FnMut(i32, u32, &str)) -> Option<R> {
        let f = self.f.as_ref()?;
        let fd = f.as_raw_fd();
        let mut list = [0u8; 80];
        let mut ids = vec![0u8; 64 * 2048];
        list[4..8].copy_from_slice(&2048u32.to_le_bytes());
        list[16..24].copy_from_slice(&(ids.as_mut_ptr() as u64).to_le_bytes());
        if unsafe { ioctl(fd, CTL_LIST, list.as_mut_ptr()) } != 0 {
            return None;
        }
        let used = (u32a(&list, 8) as usize).min(2048);
        for i in 0..used {
            let id = &ids[i * 64..i * 64 + 64];
            let numid = u32a(id, 0) as i32;
            let name = String::from_utf8_lossy(&id[16..60])
                .trim_end_matches('\0')
                .to_string();
            cb(numid, numid as u32, &name);
            let _ = name;
        }
        None::<R>
    }

    /// int/bool-Control, dessen Name `key` enthält, auf `val` setzen.
    pub fn set_by_name(&self, key: &str, val: i64) -> usize {
        let Some(f) = self.f.as_ref() else { return 0 };
        let fd = f.as_raw_fd();
        let mut list = [0u8; 80];
        let mut ids = vec![0u8; 64 * 2048];
        list[4..8].copy_from_slice(&2048u32.to_le_bytes());
        list[16..24].copy_from_slice(&(ids.as_mut_ptr() as u64).to_le_bytes());
        if unsafe { ioctl(fd, CTL_LIST, list.as_mut_ptr()) } != 0 {
            return 0;
        }
        let used = (u32a(&list, 8) as usize).min(2048);
        let mut done = 0;
        for i in 0..used {
            let id = &ids[i * 64..i * 64 + 64];
            let name = String::from_utf8_lossy(&id[16..60])
                .trim_end_matches('\0')
                .to_string();
            if !name.contains(key) {
                continue;
            }
            let mut info = [0u8; 272];
            info[..64].copy_from_slice(id);
            if unsafe { ioctl(fd, CTL_INFO, info.as_mut_ptr()) } != 0 {
                continue;
            }
            let ty = u32a(&info, 64);
            let count = (u32a(&info, 72) as usize).min(8);
            if ty != 1 && ty != 2 {
                continue;
            }
            let mut v = [0u8; 1224];
            v[..64].copy_from_slice(id);
            for c in 0..count.max(1) {
                v[72 + c * 8..80 + c * 8].copy_from_slice(&val.to_le_bytes());
            }
            if unsafe { ioctl(fd, CTL_WRITE, v.as_mut_ptr()) } == 0 {
                done += 1;
            }
        }
        done
    }

    /// Enum-Control (Name enthält `key`) auf den Item-String `item` setzen.
    pub fn set_enum(&self, key: &str, item: &str) -> bool {
        let Some(f) = self.f.as_ref() else {
            return false;
        };
        let fd = f.as_raw_fd();
        let mut list = [0u8; 80];
        let mut ids = vec![0u8; 64 * 2048];
        list[4..8].copy_from_slice(&2048u32.to_le_bytes());
        list[16..24].copy_from_slice(&(ids.as_mut_ptr() as u64).to_le_bytes());
        if unsafe { ioctl(fd, CTL_LIST, list.as_mut_ptr()) } != 0 {
            return false;
        }
        let used = (u32a(&list, 8) as usize).min(2048);
        for i in 0..used {
            let id = &ids[i * 64..i * 64 + 64];
            let name = String::from_utf8_lossy(&id[16..60])
                .trim_end_matches('\0')
                .to_string();
            if !name.contains(key) {
                continue;
            }
            let mut info = [0u8; 272];
            info[..64].copy_from_slice(id);
            if unsafe { ioctl(fd, CTL_INFO, info.as_mut_ptr()) } != 0 {
                continue;
            }
            if u32a(&info, 64) != 3 {
                continue; // nicht enum
            }
            let mut items = u32a(&info, 80);
            if items > 128 {
                items = 128;
            }
            for it in 0..items {
                let mut info2 = [0u8; 272];
                info2[..64].copy_from_slice(id);
                info2[84..88].copy_from_slice(&it.to_le_bytes());
                if unsafe { ioctl(fd, CTL_INFO, info2.as_mut_ptr()) } != 0 {
                    continue;
                }
                let en = String::from_utf8_lossy(&info2[88..152])
                    .trim_end_matches('\0')
                    .to_string();
                if en == item {
                    let mut v = [0u8; 1224];
                    v[..64].copy_from_slice(id);
                    v[72..76].copy_from_slice(&it.to_le_bytes());
                    return unsafe { ioctl(fd, CTL_WRITE, v.as_mut_ptr()) } == 0;
                }
            }
        }
        false
    }

    /// Lautsprecher-Route (media-speaker aus mixer_paths.xml) setzen: SPUS<rdma> -> SIFS1 -> UAIF SPK3 (UAIF3-Codec).
    pub fn route_speaker(&self, rdma: u32) {
        // ABOX Sampling Rates auf 48k (UAIF/SIFS)
        self.set_by_name("ABOX Sampling Rate Out1", 48000);
        self.set_by_name("ABOX Sampling Rate Out2", 48000);
        // FE(SPUS OUT<rdma>) -> SIFS1 ; SIFS1 Quelle = SPUS OUT<rdma>
        self.set_enum(&format!("ABOX SPUS OUT{rdma}"), "SIFS1");
        self.set_enum("ABOX SIFS1", &format!("SPUS OUT{rdma}"));
        // UAIF3-Lautsprecher-Link = SIFS1 ; NSRC3 = UAIF3
        self.set_enum("ABOX UAIF SPK3", "SIFS1");
        self.set_enum("ABOX NSRC3", "UAIF3");
    }
}
