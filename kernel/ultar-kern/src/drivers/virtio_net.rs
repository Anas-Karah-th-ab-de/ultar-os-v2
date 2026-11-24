//! virtio-net: Netzwerk in VMs und Cloud-Servern.

use super::virtio::{device_type, Buf, Queue, Transport};
use crate::driver::{Driver, NetDev, Result, NET};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Drv;
pub static DRV: Drv = Drv;

const F_MAC: u64 = 1 << 5;
/// config field `status` (bit 0: link up)
const F_STATUS: u64 = 1 << 16;
/// config fields `speed` (Mbit/s) and `duplex`
const F_SPEED_DUPLEX: u64 = 1 << 63;
/// virtio_net_hdr mit num_buffers (virtio 1.0)
const HDR: usize = 12;
const RXN: u16 = 16;
const BUFSZ: u64 = 2048;

struct Nic {
    name: String,
    mac: [u8; 6],
    rx: Queue,
    tx: Queue,
    rxbufs: Vec<u64>,
    txbuf: u64,
    /// device configuration (virtio-net config space) and the features taken
    cfg: u64,
    features: u64,
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "virtio-net"
    }
    fn matches(&self, d: &PciDev) -> bool {
        device_type(d) == Some(1)
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let t = Transport::new(d)?;
        let f = t.negotiate(F_MAC | F_STATUS | F_SPEED_DUPLEX)?;
        let mut rx = t.queue(0, RXN)?;
        let tx = t.queue(1, 16)?;
        let mut mac = [0u8; 6];
        if f & F_MAC != 0 {
            for (i, m) in mac.iter_mut().enumerate() {
                *m = t.dev_r8(i as u64);
            }
        }
        // Empfangspuffer: je ein Deskriptor pro 2-KB-Puffer
        let pages = alloc_pages((RXN as usize * BUFSZ as usize).div_ceil(4096)).ok_or("kein Speicher")?;
        let mut rxbufs = Vec::new();
        for i in 0..rx.size.min(RXN) as u64 {
            let a = pages + i * BUFSZ;
            rx.submit(&[Buf { addr: a, len: BUFSZ as u32, write: true }]);
            rxbufs.push(a);
        }
        t.ready();
        rx.kick();
        let txbuf = alloc_pages(1).ok_or("kein Speicher")?;
        let mut nl = NET.lock();
        let name = format!("eth{}", nl.len());
        crate::kprintln!("[virtio-net] {name}: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        nl.push(Box::new(Nic { name, mac, rx, tx, rxbufs, txbuf, cfg: t.device, features: f }));
        Ok(())
    }
    fn can_remove(&self) -> core::result::Result<(), &'static str> {
        Ok(())
    }
    /// Teardown: device reset (virtio 1.2 §2.1: the device stops using the queues).
    fn remove(&self, d: &PciDev) -> Result<()> {
        if Transport::new(d)?.reset() {
            Ok(())
        } else {
            Err(String::from("reset not confirmed"))
        }
    }
}

impl NetDev for Nic {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    /// Without VIRTIO_NET_F_STATUS the link is always up (virtio 1.2 §5.1.4).
    fn link(&self) -> Option<bool> {
        if self.features & F_STATUS == 0 {
            return Some(true);
        }
        // SAFETY: `cfg` is the device-specific configuration in an identity-mapped BAR.
        let status = unsafe { core::ptr::read_volatile((self.cfg + 6) as *const u16) };
        Some(status & 1 != 0)
    }
    fn speed_mbit(&self) -> Option<u32> {
        if self.features & F_SPEED_DUPLEX == 0 || self.link() != Some(true) {
            return None;
        }
        // SAFETY: as in `link`; `speed` sits at offset 12 (after mac, status, max pairs, mtu).
        let speed = unsafe { core::ptr::read_volatile((self.cfg + 12) as *const u32) };
        (speed != 0 && speed != u32::MAX).then_some(speed)
    }
    fn driver(&self) -> &'static str {
        "virtio-net"
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        if frame.len() + HDR > 4096 {
            return Err("Rahmen zu groß".into());
        }
        unsafe {
            core::ptr::write_bytes(self.txbuf as *mut u8, 0, HDR);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), (self.txbuf + HDR as u64) as *mut u8, frame.len());
        }
        self.tx.submit(&[Buf { addr: self.txbuf, len: (HDR + frame.len()) as u32, write: false }]);
        self.tx.kick();
        self.tx.wait(1000).ok_or("Senden: Zeitüberschreitung")?;
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let (head, len) = self.rx.poll()?;
        let a = self.rxbufs.get(head as usize).copied().unwrap_or(self.rxbufs[0]);
        let n = (len as usize).saturating_sub(HDR).min(buf.len());
        unsafe { core::ptr::copy_nonoverlapping((a + HDR as u64) as *const u8, buf.as_mut_ptr(), n) };
        self.rx.resubmit(head);
        self.rx.kick();
        Some(n)
    }
}
