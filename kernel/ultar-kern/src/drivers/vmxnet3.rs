//! vmxnet3: the paravirtual network card of VMware (15ad:07b0), also emulated by QEMU.
//! Norm: the device interface as VMware publishes it for its open drivers (`vmxnet3_defs.h`,
//! "VMXNET3 device interface", revision 1): BAR0 = pass-through registers (interrupt masks,
//! producer indices), BAR1 = virtual-device registers (version, shared-area address, command,
//! MAC); one TX queue and one RX queue (two rings: head and body buffers) in polling mode.

use crate::driver::{Driver, NetDev, Result, NET};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{fence, Ordering};

pub struct Drv;
pub static DRV: Drv = Drv;

// BAR0 (PT)
const PT_IMR: u64 = 0x000;
const PT_TXPROD: u64 = 0x600;
const PT_RXPROD: u64 = 0x800;
const PT_RXPROD2: u64 = 0xA00;
// BAR1 (VD)
const VD_VRRS: u64 = 0x00;
const VD_UVRS: u64 = 0x08;
const VD_DSAL: u64 = 0x10;
const VD_DSAH: u64 = 0x18;
const VD_CMD: u64 = 0x20;
const VD_MACL: u64 = 0x28;
const VD_MACH: u64 = 0x30;

const CMD_ACTIVATE: u32 = 0xCAFE_0000;
const CMD_QUIESCE: u32 = 0xCAFE_0001;
const CMD_RESET: u32 = 0xCAFE_0002;
const CMD_UPDATE_RX_MODE: u32 = 0xCAFE_0003;
const CMD_GET_LINK: u32 = 0xF00D_0002;

const MAGIC: u32 = 0xbabe_fee1;
const RXM_UCAST: u32 = 1;
const RXM_BCAST: u32 = 4;
const RXM_ALL_MULTI: u32 = 8;

/// Ring sizes (multiples of 32 as the device wants them)
const TXN: usize = 32;
const RXN: usize = 32;
const BUF: usize = 2048;
const DATA_DESC: usize = 128;

// Offsets in the driver-shared area (Vmxnet3_DriverShared)
const DS_MAGIC: u64 = 0;
const DS_SIZE: u64 = 4;
const DS_MISC: u64 = 8; // Vmxnet3_MiscConf (72 bytes)
const DS_INTR: u64 = 8 + 72; // Vmxnet3_IntrConf (40 bytes)
const DS_RXF: u64 = 8 + 72 + 40; // Vmxnet3_RxFilterConf (528 bytes)
const DS_LEN: u32 = 720;

struct Ring {
    base: u64,
    size: usize,
    next: usize,
    gen: u32,
}

impl Ring {
    fn new(size: usize) -> Option<Ring> {
        let base = alloc_pages((size * 16).div_ceil(4096))?;
        Some(Ring {
            base,
            size,
            next: 0,
            gen: 1,
        })
    }
    fn desc(&self, i: usize) -> u64 {
        self.base + (i * 16) as u64
    }
    fn advance(&mut self) {
        self.next += 1;
        if self.next == self.size {
            self.next = 0;
            self.gen ^= 1;
        }
    }
}

struct Nic {
    name: String,
    pt: u64,
    vd: u64,
    mac: [u8; 6],
    tx: Ring,
    tx_comp: Ring,
    tx_bufs: u64,
    tx_busy: usize,
    rx: [Ring; 2],
    rx_bufs: [u64; 2],
    rx_comp: Ring,
}

// SAFETY (all register and memory helpers below): `b`/`a` are addresses in the device's
// identity-mapped BARs or in pages this driver allocated for the device (rings, shared area);
// accesses are naturally aligned and volatile because the device reads and writes them too.
fn r(b: u64, o: u64) -> u32 {
    // SAFETY: see above (BAR register).
    unsafe { core::ptr::read_volatile((b + o) as *const u32) }
}
fn w(b: u64, o: u64, v: u32) {
    // SAFETY: see above (BAR register).
    unsafe { core::ptr::write_volatile((b + o) as *mut u32, v) }
}
fn m32(a: u64) -> u32 {
    // SAFETY: see above (driver-owned DMA memory).
    unsafe { core::ptr::read_volatile(a as *const u32) }
}
fn s32(a: u64, v: u32) {
    // SAFETY: see above (driver-owned DMA memory).
    unsafe { core::ptr::write_volatile(a as *mut u32, v) }
}
fn s64(a: u64, v: u64) {
    // SAFETY: see above (driver-owned DMA memory).
    unsafe { core::ptr::write_volatile(a as *mut u64, v) }
}
fn s16(a: u64, v: u16) {
    // SAFETY: see above (driver-owned DMA memory).
    unsafe { core::ptr::write_volatile(a as *mut u16, v) }
}
fn s8(a: u64, v: u8) {
    // SAFETY: see above (driver-owned DMA memory).
    unsafe { core::ptr::write_volatile(a as *mut u8, v) }
}

fn cmd(vd: u64, c: u32) -> u32 {
    w(vd, VD_CMD, c);
    r(vd, VD_CMD)
}

/// Post the RX buffer of slot `i` of ring `k` (head buffers in ring 0, body buffers in ring 1).
fn post_rx(ring: &Ring, bufs: u64, k: usize, i: usize) {
    let d = ring.desc(i);
    s64(d, bufs + (i * BUF) as u64);
    s32(d + 12, 0);
    fence(Ordering::SeqCst);
    // len:14, btype:1 (0 head, 1 body), dtype:1, gen bit 31 – written last
    s32(d + 8, BUF as u32 | (k as u32) << 14 | ring.gen << 31);
}

/// Rings and buffers of the one TX and one RX queue, and their queue descriptors.
struct Queues {
    qd: u64,
    tx: Ring,
    tx_comp: Ring,
    tx_data: u64,
    tx_bufs: u64,
    rx: [Ring; 2],
    rx_bufs: [u64; 2],
    rx_comp: Ring,
}

impl Queues {
    fn new() -> Option<Queues> {
        Some(Queues {
            // queue descriptors: one TX (256 bytes) then one RX (256 bytes)
            qd: alloc_pages(1)?,
            tx: Ring::new(TXN)?,
            tx_comp: Ring::new(TXN)?,
            tx_data: alloc_pages((TXN * DATA_DESC).div_ceil(4096))?,
            tx_bufs: alloc_pages(TXN * BUF / 4096)?,
            rx: [Ring::new(RXN)?, Ring::new(RXN)?],
            rx_bufs: [
                alloc_pages(RXN * BUF / 4096)?,
                alloc_pages(RXN * BUF / 4096)?,
            ],
            rx_comp: Ring::new(2 * RXN)?,
        })
    }

    /// Queue descriptors and the driver-shared area (`Vmxnet3_DriverShared`) at `shared`.
    fn describe(&self, shared: u64) {
        // TX queue descriptor: ctrl (16; txThreshold 0: every packet is sent at once), conf (64)
        let c = self.qd + 16;
        s64(c, self.tx.base);
        s64(c + 8, self.tx_data);
        s64(c + 16, self.tx_comp.base);
        s32(c + 40, TXN as u32);
        s32(c + 44, TXN as u32);
        s32(c + 48, TXN as u32);
        s16(c + 58, DATA_DESC as u16);
        // RX queue descriptor
        let c = self.qd + 256 + 16;
        s64(c, self.rx[0].base);
        s64(c + 8, self.rx[1].base);
        s64(c + 16, self.rx_comp.base);
        s32(c + 40, RXN as u32);
        s32(c + 44, RXN as u32);
        s32(c + 48, (2 * RXN) as u32);
        s32(shared + DS_MAGIC, MAGIC);
        s32(shared + DS_SIZE, DS_LEN);
        let m = shared + DS_MISC;
        s32(m, 1); // driverInfo.version
        s32(m + 4, 2 | 1 << 2); // gos: 64 bit, type "Linux-like"
        s32(m + 8, 1); // vmxnet3RevSpt
        s32(m + 12, 1); // uptVerSpt
        s64(m + 16, 0); // uptFeatures: none (no checksum offload, no VLAN, no LRO)
        s64(m + 32, self.qd); // queueDescPA
        s32(m + 44, 512); // queueDescLen
        s32(m + 48, 1500); // mtu
        s16(m + 52, 1); // maxNumRxSG
        s8(m + 54, 1); // numTxQueues
        s8(m + 55, 1); // numRxQueues
        let i = shared + DS_INTR;
        s8(i + 1, 1); // numIntrs (autoMask 0, eventIntrIdx 0)
        s32(i + 28, 1); // intrCtrl: VMXNET3_IC_DISABLE_ALL (polling)
        s32(shared + DS_RXF, RXM_UCAST | RXM_BCAST | RXM_ALL_MULTI);
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "vmxnet3"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x15ad && d.device == 0x07b0
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let pt = d.bars[0].filter(|b| !b.io).ok_or("BAR0 missing")?.addr;
        let vd = d.bars[1].filter(|b| !b.io).ok_or("BAR1 missing")?.addr;
        d.enable();
        // revision 1 of the device and of UPT
        if r(vd, VD_VRRS) & 1 == 0 {
            return Err("device revision 1 not offered".into());
        }
        w(vd, VD_VRRS, 1);
        if r(vd, VD_UVRS) & 1 == 0 {
            return Err("UPT revision 1 not offered".into());
        }
        w(vd, VD_UVRS, 1);
        w(vd, VD_CMD, CMD_RESET);
        let (lo, hi) = (r(vd, VD_MACL), r(vd, VD_MACH));
        let mac = [
            lo as u8,
            (lo >> 8) as u8,
            (lo >> 16) as u8,
            (lo >> 24) as u8,
            hi as u8,
            (hi >> 8) as u8,
        ];

        let q = Queues::new().ok_or("no memory")?;
        let shared = alloc_pages(1).ok_or("no memory")?;
        q.describe(shared);
        let Queues {
            qd: _,
            tx,
            tx_comp,
            tx_data: _,
            tx_bufs,
            rx,
            rx_bufs,
            rx_comp,
        } = q;
        // RX rings: all slots but one posted (an empty slot tells full from empty)
        for (k, ring) in rx.iter().enumerate() {
            for slot in 0..RXN - 1 {
                post_rx(ring, rx_bufs[k], k, slot);
            }
        }
        let mut rx = rx;
        for ring in rx.iter_mut() {
            ring.next = RXN - 1;
        }
        fence(Ordering::SeqCst);
        w(vd, VD_DSAL, shared as u32);
        w(vd, VD_DSAH, (shared >> 32) as u32);
        let res = cmd(vd, CMD_ACTIVATE);
        if res != 0 {
            return Err(format!("activation refused ({res:#x})"));
        }
        w(pt, PT_IMR, 1); // the only interrupt stays masked
        w(vd, VD_CMD, CMD_UPDATE_RX_MODE);
        w(pt, PT_RXPROD, (RXN - 1) as u32);
        w(pt, PT_RXPROD2, (RXN - 1) as u32);

        let link = cmd(vd, CMD_GET_LINK);
        let mut nl = NET.lock();
        let name = format!("eth{}", nl.len());
        crate::kprintln!(
            "[vmxnet3] {name}: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} Verbindung {} ({} Mbit/s)",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], if link & 1 != 0 { "steht" } else { "fehlt" }, link >> 16
        );
        nl.push(Box::new(Nic {
            name,
            pt,
            vd,
            mac,
            tx,
            tx_comp,
            tx_bufs,
            tx_busy: 0,
            rx,
            rx_bufs,
            rx_comp,
        }));
        Ok(())
    }
    fn can_remove(&self) -> core::result::Result<(), &'static str> {
        Ok(())
    }
    /// Teardown: quiesce (the device stops using the rings), then reset.
    fn remove(&self, d: &PciDev) -> Result<()> {
        let vd = d.bars[1].filter(|b| !b.io).ok_or("BAR1 missing")?.addr;
        w(vd, VD_CMD, CMD_QUIESCE);
        w(vd, VD_CMD, CMD_RESET);
        Ok(())
    }
}

impl Nic {
    /// Collect TX completions: every completion frees the slots up to its descriptor.
    fn reap_tx(&mut self) {
        loop {
            let d = self.tx_comp.desc(self.tx_comp.next);
            let w3 = m32(d + 12);
            if w3 >> 31 != self.tx_comp.gen {
                break;
            }
            fence(Ordering::SeqCst);
            let idx = (m32(d) & 0xfff) as usize;
            // slots from the oldest busy one up to `idx` are done
            let oldest = (self.tx.next + TXN - self.tx_busy) % TXN;
            let done = (idx + TXN - oldest) % TXN + 1;
            self.tx_busy = self.tx_busy.saturating_sub(done);
            self.tx_comp.advance();
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
    fn link(&self) -> Option<bool> {
        Some(cmd(self.vd, CMD_GET_LINK) & 1 != 0)
    }
    fn speed_mbit(&self) -> Option<u32> {
        let l = cmd(self.vd, CMD_GET_LINK);
        (l & 1 != 0 && l >> 16 != 0).then_some(l >> 16)
    }
    fn driver(&self) -> &'static str {
        "vmxnet3"
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.reap_tx();
        if self.tx_busy >= TXN - 1 {
            let end = crate::arch::uptime_ms() + 100;
            while self.tx_busy >= TXN - 1 && crate::arch::uptime_ms() < end {
                crate::arch::pause();
                self.reap_tx();
            }
            if self.tx_busy >= TXN - 1 {
                return Err("send ring full".into());
            }
        }
        let n = frame.len().min(BUF);
        let i = self.tx.next;
        let buf = self.tx_bufs + (i * BUF) as u64;
        // SAFETY: slot `i` is free (not in the device's hands) and its buffer holds BUF >= n bytes.
        unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), buf as *mut u8, n) };
        let d = self.tx.desc(i);
        s64(d, buf);
        // dword3: eop (bit 12) and cq (bit 13: a completion for this packet)
        s32(d + 12, 1 << 12 | 1 << 13);
        fence(Ordering::SeqCst);
        // dword2: len, gen (bit 14) – written last, it hands the descriptor to the device
        s32(d + 8, n as u32 | self.tx.gen << 14);
        fence(Ordering::SeqCst);
        self.tx.advance();
        self.tx_busy += 1;
        w(self.pt, PT_TXPROD, self.tx.next as u32);
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        loop {
            let c = self.rx_comp.desc(self.rx_comp.next);
            let w3 = m32(c + 12);
            if w3 >> 31 != self.rx_comp.gen {
                return None;
            }
            fence(Ordering::SeqCst);
            let w0 = m32(c);
            let w2 = m32(c + 8);
            self.rx_comp.advance();
            let idx = (w0 & 0xfff) as usize;
            // rqID: 0 = ring 0 of queue 0, 1 (= numRxQueues) = its ring 1
            let k = usize::from((w0 >> 16) & 0x3ff != 0);
            let (sop, eop) = (w0 & (1 << 14) != 0, w0 & (1 << 15) != 0);
            let len = (w2 & 0x3fff) as usize;
            let err = w2 & (1 << 14) != 0;
            let src = self.rx_bufs[k] + (idx * BUF) as u64;
            let mut got = None;
            if sop && eop && !err && len > 0 {
                let n = len.min(buf.len());
                // SAFETY: the device completed slot `idx`; its buffer holds `len` <= BUF bytes.
                unsafe { core::ptr::copy_nonoverlapping(src as *const u8, buf.as_mut_ptr(), n) };
                got = Some(n);
            }
            // give a buffer back: the free slot of the ring takes its own buffer
            let ring = &mut self.rx[k];
            let slot = ring.next;
            post_rx(ring, self.rx_bufs[k], k, slot);
            ring.advance();
            w(
                self.pt,
                if k == 0 { PT_RXPROD } else { PT_RXPROD2 },
                self.rx[k].next as u32,
            );
            if got.is_some() {
                return got;
            }
        }
    }
}
