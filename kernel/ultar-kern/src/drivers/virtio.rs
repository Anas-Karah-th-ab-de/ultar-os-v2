//! virtio 1.x über PCI (moderne Schnittstelle): Fähigkeiten lesen, Gerät aushandeln, geteilte Warteschlangen.
//! Grundlage für virtio-blk, virtio-net (und später virtio-gpu, -input) – Standard in VMs und Servern.

use crate::mem::{alloc_pages, PAGE};
use crate::pci::PciDev;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

pub const VENDOR: u16 = 0x1af4;

/// virtio-Gerätetyp aus der PCI-Kennung (modern 0x1040+n, Übergang 0x1000..0x103f)
pub fn device_type(d: &PciDev) -> Option<u16> {
    if d.vendor != VENDOR {
        return None;
    }
    match d.device {
        0x1040..=0x107f => Some(d.device - 0x1040),
        0x1000 => Some(1),
        0x1001 => Some(2),
        0x1005 => Some(4),
        0x1009 => Some(9),
        _ => None,
    }
}

pub struct Transport {
    common: u64,
    notify: u64,
    notify_mul: u32,
    pub device: u64,
}

const ST_ACK: u8 = 1;
const ST_DRIVER: u8 = 2;
const ST_DRIVER_OK: u8 = 4;
const ST_FEATURES_OK: u8 = 8;
pub const F_VERSION_1: u64 = 1 << 32;

unsafe fn w8(a: u64, v: u8) {
    write_volatile(a as *mut u8, v)
}
unsafe fn r8(a: u64) -> u8 {
    read_volatile(a as *const u8)
}
unsafe fn w16(a: u64, v: u16) {
    write_volatile(a as *mut u16, v)
}
unsafe fn r16(a: u64) -> u16 {
    read_volatile(a as *const u16)
}
unsafe fn w32(a: u64, v: u32) {
    write_volatile(a as *mut u32, v)
}
unsafe fn r32(a: u64) -> u32 {
    read_volatile(a as *const u32)
}
unsafe fn w64(a: u64, v: u64) {
    w32(a, v as u32);
    w32(a + 4, (v >> 32) as u32);
}

impl Transport {
    pub fn new(d: &PciDev) -> crate::driver::Result<Transport> {
        d.enable();
        let (mut common, mut notify, mut mul, mut device) = (0, 0, 0, 0);
        for (p, id) in d.caps() {
            if id != 0x09 {
                continue;
            }
            let typ = d.r8(p + 3);
            let bar = d.r8(p + 4) as usize;
            let off = d.r32(p + 8) as u64;
            let Some(Some(b)) = d.bars.get(bar) else { continue };
            if b.io {
                continue;
            }
            let a = b.addr + off;
            match typ {
                1 => common = a,
                2 => {
                    notify = a;
                    mul = d.r32(p + 16);
                }
                4 => device = a,
                _ => {}
            }
        }
        if common == 0 || notify == 0 {
            return Err("keine moderne virtio-Schnittstelle (nur Altgerät)".into());
        }
        Ok(Transport { common, notify, notify_mul: mul, device })
    }

    /// Zurücksetzen, Merkmale aushandeln. Liefert die angenommenen Merkmale.
    pub fn negotiate(&self, wanted: u64) -> crate::driver::Result<u64> {
        unsafe {
            let c = self.common;
            w8(c + 20, 0);
            let mut n = 0;
            while r8(c + 20) != 0 && n < 1_000_000 {
                n += 1;
            }
            w8(c + 20, ST_ACK);
            w8(c + 20, ST_ACK | ST_DRIVER);
            w32(c, 0);
            let lo = r32(c + 4) as u64;
            w32(c, 1);
            let hi = r32(c + 4) as u64;
            let dev = lo | hi << 32;
            let take = dev & (wanted | F_VERSION_1);
            if take & F_VERSION_1 == 0 {
                return Err("Gerät kann virtio 1.0 nicht".into());
            }
            w32(c + 8, 0);
            w32(c + 12, take as u32);
            w32(c + 8, 1);
            w32(c + 12, (take >> 32) as u32);
            w8(c + 20, ST_ACK | ST_DRIVER | ST_FEATURES_OK);
            if r8(c + 20) & ST_FEATURES_OK == 0 {
                return Err("Merkmale abgelehnt".into());
            }
            Ok(take)
        }
    }

    pub fn queue(&self, idx: u16, max: u16) -> crate::driver::Result<Queue> {
        unsafe {
            let c = self.common;
            w16(c + 22, idx);
            let size = r16(c + 24).min(max);
            if size == 0 {
                return Err(alloc::format!("Warteschlange {idx} fehlt"));
            }
            w16(c + 24, size);
            let n = size as u64;
            let desc = alloc_pages(1).ok_or("kein Speicher")?;
            let avail = alloc_pages(1).ok_or("kein Speicher")?;
            let used = alloc_pages(1).ok_or("kein Speicher")?;
            debug_assert!(16 * n <= PAGE && 8 * n + 6 <= PAGE);
            w64(c + 32, desc);
            w64(c + 40, avail);
            w64(c + 48, used);
            let noff = r16(c + 30) as u64;
            w16(c + 28, 1);
            Ok(Queue { size, desc, avail, used, notify: self.notify + noff * self.notify_mul as u64, idx, last_used: 0, free_head: 0 })
        }
    }

    pub fn ready(&self) {
        unsafe { w8(self.common + 20, ST_ACK | ST_DRIVER | ST_FEATURES_OK | ST_DRIVER_OK) }
    }
    pub fn dev_r8(&self, off: u64) -> u8 {
        unsafe { r8(self.device + off) }
    }
    pub fn dev_r32(&self, off: u64) -> u32 {
        unsafe { r32(self.device + off) }
    }
}

/// Geteilte Warteschlange (split virtqueue)
pub struct Queue {
    pub size: u16,
    desc: u64,
    avail: u64,
    used: u64,
    notify: u64,
    idx: u16,
    last_used: u16,
    free_head: u16,
}

pub struct Buf {
    pub addr: u64,
    pub len: u32,
    pub write: bool,
}

impl Queue {
    /// Kette von Puffern einstellen; liefert den Kopf-Index
    pub fn submit(&mut self, bufs: &[Buf]) -> u16 {
        let head = self.free_head;
        unsafe {
            for (i, b) in bufs.iter().enumerate() {
                let di = (head as usize + i) % self.size as usize;
                let d = self.desc + di as u64 * 16;
                w64(d, b.addr);
                w32(d + 8, b.len);
                let mut flags = if b.write { 2 } else { 0 };
                if i + 1 < bufs.len() {
                    flags |= 1;
                }
                w16(d + 12, flags);
                w16(d + 14, ((di + 1) % self.size as usize) as u16);
            }
            self.free_head = ((head as usize + bufs.len()) % self.size as usize) as u16;
            let aidx = r16(self.avail + 2);
            w16(self.avail + 4 + (aidx % self.size) as u64 * 2, head);
            fence(Ordering::SeqCst);
            w16(self.avail + 2, aidx.wrapping_add(1));
            fence(Ordering::SeqCst);
        }
        head
    }
    /// Bereits beschriebenen Deskriptor erneut anbieten (Empfangspuffer nach dem Abholen)
    pub fn resubmit(&mut self, head: u16) {
        unsafe {
            let aidx = r16(self.avail + 2);
            w16(self.avail + 4 + (aidx % self.size) as u64 * 2, head);
            fence(Ordering::SeqCst);
            w16(self.avail + 2, aidx.wrapping_add(1));
            fence(Ordering::SeqCst);
        }
    }
    pub fn kick(&self) {
        unsafe { w16(self.notify, self.idx) }
    }
    /// Fertige Anfrage abholen: (Kopf-Index, geschriebene Länge)
    pub fn poll(&mut self) -> Option<(u16, u32)> {
        unsafe {
            fence(Ordering::SeqCst);
            let uidx = r16(self.used + 2);
            if uidx == self.last_used {
                return None;
            }
            let e = self.used + 4 + (self.last_used % self.size) as u64 * 8;
            let id = r32(e) as u16;
            let len = r32(e + 4);
            self.last_used = self.last_used.wrapping_add(1);
            Some((id, len))
        }
    }
    /// warten (mit Zeitgrenze in ms)
    pub fn wait(&mut self, ms: u64) -> Option<(u16, u32)> {
        let end = crate::arch::uptime_ms() + ms;
        loop {
            if let Some(r) = self.poll() {
                return Some(r);
            }
            if crate::arch::uptime_ms() > end {
                return None;
            }
            crate::arch::pause();
        }
    }
}
