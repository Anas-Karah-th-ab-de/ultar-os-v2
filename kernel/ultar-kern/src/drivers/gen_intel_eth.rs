//! Ultar-Kern – Treiber „gen-intel-eth“ (Treiber-Werkstatt).
//! Gerät: Intel-Ethernet 8254x/82574 (e1000/e1000e) · Norm: öffentliche Intel-Datenblätter
//! („PCI/PCI-X Family of Gigabit Ethernet Controllers SDM“, „82574 GbE Controller Datasheet“):
//! Register CTRL/STATUS/RCTL/TCTL, Empfangs- und Sende-Deskriptorringe (Legacy-Format), Abfragebetrieb.
//! Auftrag: werkstatt/auftraege/intel-eth.md · Status: ausgeschrieben, in QEMU (e1000, e1000e) geprüft.

use crate::driver::{Driver, NetDev, Result, NET};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

pub struct Drv;
pub static DRV: Drv = Drv;

/// bekannte Geräte: 8254x (e1000), 82571–82574 (e1000e), I217/I218/I219 (e1000e, Laptops)
const IDS: &[u16] = &[
    0x100e, 0x100f, 0x1004, 0x1008, 0x1010, 0x1015, 0x1019, 0x101e, 0x1026, 0x1075, 0x1076, 0x1079, 0x107c, 0x105e, 0x105f, 0x10a4, 0x10b9,
    0x10d3, 0x10f6, 0x150c, 0x153a, 0x153b, 0x155a, 0x1559, 0x15a0, 0x15a1, 0x15a2, 0x15a3, 0x156f, 0x1570, 0x15b7, 0x15b8, 0x15b9, 0x15bb,
    0x15bc, 0x15bd, 0x15be, 0x15d6, 0x15d7, 0x15d8, 0x15e3, 0x0d4e, 0x0d4f, 0x0d4c, 0x0d4d, 0x15fb, 0x15fc, 0x1a1e, 0x1a1f,
    // I210/I211 (1G), I225/I226 (2.5G)
    0x1533, 0x1536, 0x1539, 0x157b, 0x157c, 0x15f2, 0x15f3, 0x0d9f, 0x15f8,
    0x125b, 0x125c, 0x125d, 0x125f,
];

const CTRL: u64 = 0x0000;
const STATUS: u64 = 0x0008;
const EERD: u64 = 0x0014;
const IMC: u64 = 0x00D8;
const RCTL: u64 = 0x0100;
const TCTL: u64 = 0x0400;
const TIPG: u64 = 0x0410;
const RDBAL: u64 = 0x2800;
const RDBAH: u64 = 0x2804;
const RDLEN: u64 = 0x2808;
const RDH: u64 = 0x2810;
const RDT: u64 = 0x2818;
const TDBAL: u64 = 0x3800;
const TDBAH: u64 = 0x3804;
const TDLEN: u64 = 0x3808;
const TDH: u64 = 0x3810;
const TDT: u64 = 0x3818;
const MTA: u64 = 0x5200;
const RAL: u64 = 0x5400;
const RAH: u64 = 0x5404;

// Four times the old ring depth: enough for wire-rate bursts without polling
// every packet through a 32-slot software/descriptor barrier.
const N: usize = 128;
const BUF: usize = 2048;

struct Nic {
    name: String,
    mmio: u64,
    mac: [u8; 6],
    rx_ring: u64,
    rx_bufs: u64,
    tx_ring: u64,
    tx_bufs: u64,
    rx_next: usize,
    tx_next: usize,
}

fn r(b: u64, o: u64) -> u32 {
    unsafe { core::ptr::read_volatile((b + o) as *const u32) }
}
fn w(b: u64, o: u64, v: u32) {
    unsafe { core::ptr::write_volatile((b + o) as *mut u32, v) }
}

fn link_speed(status: u32) -> &'static str {
    if status & 2 == 0 {
        "nicht verbunden"
    } else {
        match (status >> 6) & 3 {
            0 => "10 Mbit/s",
            1 => "100 Mbit/s",
            2 => "1 Gbit/s",
            _ => "1 Gbit/s+",
        }
    }
}

fn wait(mut f: impl FnMut() -> bool, ms: u64) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    while crate::arch::uptime_ms() < end {
        if f() {
            return true;
        }
        crate::arch::pause();
    }
    f()
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-intel-eth"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x8086 && d.class == 0x02 && IDS.contains(&d.device)
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?;
        d.enable();
        let m = bar.addr;
        // Rücksetzen (CTRL.RST), danach Verbindung aufbauen (SLU, ASDE)
        w(m, IMC, 0xffff_ffff);
        w(m, CTRL, r(m, CTRL) | (1 << 26));
        crate::arch::sleep_us(10_000);
        if !wait(|| r(m, CTRL) & (1 << 26) == 0, 500) {
            return Err("Rücksetzen hängt".into());
        }
        w(m, IMC, 0xffff_ffff);
        w(m, CTRL, (r(m, CTRL) | (1 << 6) | (1 << 5)) & !(1 << 3) & !(1 << 31) & !(1 << 7));
        // MAC: Empfangsadresse 0 (aus dem EEPROM geladen), sonst EEPROM direkt lesen
        let (lo, hi) = (r(m, RAL), r(m, RAH));
        let mut mac = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
        if mac == [0; 6] || mac == [0xff; 6] {
            for i in 0..3u32 {
                w(m, EERD, 1 | (i << 8));
                if !wait(|| r(m, EERD) & (1 << 4) != 0, 20) {
                    return Err("EEPROM antwortet nicht".into());
                }
                let v = (r(m, EERD) >> 16) as u16;
                mac[i as usize * 2] = v as u8;
                mac[i as usize * 2 + 1] = (v >> 8) as u8;
            }
            w(m, RAL, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
            w(m, RAH, u16::from_le_bytes([mac[4], mac[5]]) as u32 | (1 << 31));
        }
        for i in 0..128 {
            w(m, MTA + i * 4, 0);
        }
        // Empfangsring: 32 Deskriptoren à 16 Byte, Puffer 2 KB
        let rx_ring = alloc_pages(1).ok_or("kein Speicher")?;
        let rx_bufs = alloc_pages(N * BUF / 4096).ok_or("kein Speicher")?;
        for i in 0..N {
            unsafe { core::ptr::write_volatile((rx_ring + i as u64 * 16) as *mut u64, rx_bufs + (i * BUF) as u64) };
        }
        w(m, RDBAL, rx_ring as u32);
        w(m, RDBAH, (rx_ring >> 32) as u32);
        w(m, RDLEN, (N * 16) as u32);
        w(m, RDH, 0);
        w(m, RDT, (N - 1) as u32);
        // EN, BAM (Rundsendungen), SECRC (CRC entfernen), 2-KB-Puffer
        w(m, RCTL, (1 << 1) | (1 << 15) | (1 << 26));
        // Sendering
        let tx_ring = alloc_pages(1).ok_or("kein Speicher")?;
        let tx_bufs = alloc_pages(N * BUF / 4096).ok_or("kein Speicher")?;
        w(m, TDBAL, tx_ring as u32);
        w(m, TDBAH, (tx_ring >> 32) as u32);
        w(m, TDLEN, (N * 16) as u32);
        w(m, TDH, 0);
        w(m, TDT, 0);
        // EN, PSP, Kollisionsschwelle 15, Kollisionsabstand 64
        w(m, TCTL, (1 << 1) | (1 << 3) | (0x0f << 4) | (0x40 << 12));
        w(m, TIPG, 0x0060_200a);
        let link = wait(|| r(m, STATUS) & 2 != 0, 1500);
        let status = r(m, STATUS);
        let speed = link_speed(status);
        let mut nl = NET.lock();
        let name = format!("eth{}", nl.len());
        crate::kprintln!(
            "[intel-eth] {name}: {:04x} MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} Verbindung {} {}",
            d.device, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], if link { "steht" } else { "fehlt" }, speed
        );
        nl.push(Box::new(Nic { name, mmio: m, mac, rx_ring, rx_bufs, tx_ring, tx_bufs, rx_next: 0, tx_next: 0 }));
        Ok(())
    }
    fn can_remove(&self) -> core::result::Result<(), &'static str> {
        Ok(())
    }
    /// Teardown: receiver and transmitter off, interrupts masked, then a device reset (it ends
    /// every DMA of the rings; the device layer then turns bus mastering off).
    fn remove(&self, d: &PciDev) -> Result<()> {
        let m = d.bars[0].filter(|b| !b.io).ok_or("BAR0 missing")?.addr;
        w(m, RCTL, 0);
        w(m, TCTL, 0);
        w(m, IMC, 0xffff_ffff);
        w(m, CTRL, r(m, CTRL) | (1 << 26));
        if !wait(|| r(m, CTRL) & (1 << 26) == 0, 500) {
            return Err(String::from("reset did not finish"));
        }
        Ok(())
    }
}

/// The rings and buffers go back to the allocator: only dropped after `remove` reset the device.
impl Drop for Nic {
    fn drop(&mut self) {
        crate::mem::free_run(self.rx_ring, 1);
        crate::mem::free_run(self.tx_ring, 1);
        crate::mem::free_run(self.rx_bufs, (N * BUF / 4096) as u64);
        crate::mem::free_run(self.tx_bufs, (N * BUF / 4096) as u64);
    }
}

impl NetDev for Nic {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn link(&self) -> Option<bool> {
        Some(r(self.mmio, STATUS) & 2 != 0)
    }
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let n = frame.len().min(BUF);
        let i = self.tx_next;
        let d = self.tx_ring + i as u64 * 16;
        let buf = self.tx_bufs + (i * BUF) as u64;
        unsafe {
            // vorheriger Auftrag in diesem Platz fertig?
            if core::ptr::read_volatile((d + 8) as *const u64) != 0 && core::ptr::read_volatile((d + 12) as *const u8) & 1 == 0 {
                let mm = self.mmio;
                if !wait(|| core::ptr::read_volatile((d + 12) as *const u8) & 1 != 0, 100) {
                    let _ = r(mm, TDH);
                    return Err("Senden hängt".into());
                }
            }
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf as *mut u8, n);
            core::ptr::write_volatile(d as *mut u64, buf);
            core::ptr::write_volatile((d + 12) as *mut u8, 0);
            // Länge, CMD = EOP | IFCS | RS, Status löschen
            core::ptr::write_volatile((d + 8) as *mut u64, n as u64 | ((1u64 | 2 | 8) << 24));
        }
        self.tx_next = (i + 1) % N;
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        w(self.mmio, TDT, self.tx_next as u32);
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let i = self.rx_next;
        let d = self.rx_ring + i as u64 * 16;
        let status = unsafe { core::ptr::read_volatile((d + 12) as *const u8) };
        if status & 1 == 0 {
            return None;
        }
        let len = unsafe { core::ptr::read_volatile((d + 8) as *const u16) } as usize;
        let n = len.min(buf.len());
        unsafe {
            core::ptr::copy_nonoverlapping((self.rx_bufs + (i * BUF) as u64) as *const u8, buf.as_mut_ptr(), n);
            core::ptr::write_volatile((d + 8) as *mut u64, 0);
            core::ptr::write_volatile((d + 12) as *mut u8, 0);
        }
        self.rx_next = (i + 1) % N;
        w(self.mmio, RDT, i as u32);
        Some(n)
    }
}
