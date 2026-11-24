//! virtio-blk: Festplatten in VMs und Cloud-Servern.

use super::virtio::{device_type, Buf, Queue, Transport};
use crate::driver::{BlockDev, Driver, Result, BLOCK};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

pub struct Drv;
pub static DRV: Drv = Drv;

struct Disk {
    name: String,
    q: Queue,
    sectors: u64,
    /// DMA-Seite: Kopf (16 B) + Status (1 B); Daten in eigener Seite
    req: u64,
    data: u64,
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "virtio-blk"
    }
    fn matches(&self, d: &PciDev) -> bool {
        device_type(d) == Some(2)
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let t = Transport::new(d)?;
        t.negotiate(0)?;
        let q = t.queue(0, 64)?;
        t.ready();
        let sectors = t.dev_r32(0) as u64 | (t.dev_r32(4) as u64) << 32;
        let req = alloc_pages(1).ok_or("kein Speicher")?;
        let data = alloc_pages(16).ok_or("kein Speicher")?;
        let mut bl = BLOCK.lock();
        let name = format!("vd{}", (b'a' + bl.len() as u8) as char);
        crate::kprintln!("[virtio-blk] {name}: {} MB", sectors / 2048);
        bl.push(Box::new(Disk { name, q, sectors, req, data }));
        Ok(())
    }
}

impl BlockDev for Disk {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        for (i, chunk) in buf.chunks_mut(64 * 1024).enumerate() {
            let n = chunk.len() as u32;
            let sector = lba + i as u64 * 128;
            unsafe {
                core::ptr::write_volatile(self.req as *mut u32, 0); // VIRTIO_BLK_T_IN
                core::ptr::write_volatile((self.req + 4) as *mut u32, 0);
                core::ptr::write_volatile((self.req + 8) as *mut u64, sector);
                core::ptr::write_volatile((self.req + 16) as *mut u8, 0xff);
            }
            self.q.submit(&[Buf { addr: self.req, len: 16, write: false }, Buf { addr: self.data, len: n, write: true }, Buf { addr: self.req + 16, len: 1, write: true }]);
            self.q.kick();
            self.q.wait(2000).ok_or("Zeitüberschreitung")?;
            let st = unsafe { core::ptr::read_volatile((self.req + 16) as *const u8) };
            if st != 0 {
                return Err(format!("Lesefehler (Status {st})"));
            }
            unsafe { core::ptr::copy_nonoverlapping(self.data as *const u8, chunk.as_mut_ptr(), chunk.len()) };
        }
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<()> {
        for (i, chunk) in buf.chunks(64 * 1024).enumerate() {
            let n = chunk.len() as u32;
            let sector = lba + i as u64 * 128;
            unsafe {
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.data as *mut u8, chunk.len());
                core::ptr::write_volatile(self.req as *mut u32, 1); // VIRTIO_BLK_T_OUT
                core::ptr::write_volatile((self.req + 4) as *mut u32, 0);
                core::ptr::write_volatile((self.req + 8) as *mut u64, sector);
                core::ptr::write_volatile((self.req + 16) as *mut u8, 0xff);
            }
            self.q.submit(&[Buf { addr: self.req, len: 16, write: false }, Buf { addr: self.data, len: n, write: false }, Buf { addr: self.req + 16, len: 1, write: true }]);
            self.q.kick();
            self.q.wait(5000).ok_or("Zeitüberschreitung")?;
            let st = unsafe { core::ptr::read_volatile((self.req + 16) as *const u8) };
            if st != 0 {
                return Err(format!("Schreibfehler (Status {st})"));
            }
        }
        Ok(())
    }
    /// No feature is negotiated, VIRTIO_BLK_F_FLUSH included: without it the device works
    /// write-through (virtio block device, write cache rules), so a completed write is
    /// already durable.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
