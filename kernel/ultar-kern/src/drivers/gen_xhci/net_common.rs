//! Gemeinsamer Unterbau der USB-Netzfunktionen (RNDIS, CDC-ECM): dauerhaft armierte Bulk-IN-
//! Übertragungen (wie `bt`) in eigenen DMA-Seiten, je Seite ein Rahmen, und die 8/16/32-Bit-Zugriffe
//! auf DMA-Puffer. Jeder Treiber (`rndis`, `cdc_ecm`) behält seine eigene Rahmen-Aufbereitung und
//! seine eigene Bindung an [`NetDev`]; hier liegt nur der geteilte Teil, damit nichts kopiert wird.

use super::{w32, Hc, TRB_NORMAL};
use crate::mem::alloc_pages;

/// Gleichzeitig armierte Bulk-IN-Übertragungen (eine Seite je Übertragung).
pub(super) const TDS: usize = 8;
/// Bytes je Übertragung: eine Seite, fasst jeden Ethernet-Rahmen samt 44-Byte-RNDIS-Kopf.
pub(super) const TD: u32 = 4096;
/// Höchstens so viele empfangene Rahmen je Karte warten auf den Netzstapel.
pub(super) const RX_MAX: usize = 256;

pub(super) fn rd8(p: u64, off: usize) -> u8 {
    unsafe { core::ptr::read_unaligned((p + off as u64) as *const u8) }
}
pub(super) fn rd16(p: u64, off: usize) -> u16 {
    unsafe { core::ptr::read_unaligned((p + off as u64) as *const u16) }
}
pub(super) fn rd32(p: u64, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned((p + off as u64) as *const u32) }
}
pub(super) fn wr32(p: u64, off: usize, v: u32) {
    unsafe { core::ptr::write_unaligned((p + off as u64) as *mut u32, v) }
}

/// Armierter Bulk-IN-Eingang: `TDS` Übertragungen in `TDS` eigenen DMA-Seiten. Wird eine fertig,
/// nimmt die Ereignisschleife den Rahmen, legt ihn in die Warteschlange und armiert genau diese
/// Seite neu (`arm`), dann läutet sie die Türklingel.
pub(super) struct InEp {
    /// Türklingel-/Endpunktnummer (DCI).
    pub dci: u32,
    /// DMA-Seite je Übertragung (Adresse = physische Adresse).
    bufs: [u64; TDS],
    /// TRB-Adresse je Übertragung, um die zugehörige Seite wiederzufinden.
    trb: [u64; TDS],
}

impl InEp {
    pub fn new(dci: u32) -> Option<InEp> {
        let mut bufs = [0u64; TDS];
        let mut n = 0;
        for b in &mut bufs {
            let Some(p) = alloc_pages(1) else {
                for x in &bufs[..n] {
                    crate::mem::free_page(*x);
                }
                return None;
            };
            *b = p;
            n += 1;
        }
        Some(InEp { dci, bufs, trb: [0; TDS] })
    }

    /// Übertragung `i` einreihen und als NORMAL-TRB mit ISP|IOC armeren.
    pub fn arm(&mut self, hc: &mut Hc, slot: u32, i: usize) {
        let ring = hc.ring(slot, self.dci);
        self.trb[i] = ring.push(self.bufs[i], TD, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
    }

    /// Alle Übertragungen einreihen und die Türklingel läuten.
    pub fn arm_all(&mut self, hc: &mut Hc, slot: u32) {
        for i in 0..TDS {
            self.arm(hc, slot, i);
        }
        w32(hc.db + slot as u64 * 4, self.dci);
    }

    /// Welche Seite zur fertigen Übertragung mit TRB `trb` gehört.
    pub fn part(&self, trb: u64) -> usize {
        self.trb.iter().position(|&t| t == trb).unwrap_or(0)
    }

    /// DMA-Adresse von Seite `part`.
    pub fn base(&self, part: usize) -> u64 {
        self.bufs[part]
    }

    /// Alle DMA-Seiten zurückgeben (Gerät wurde abgezogen).
    pub fn free(&self) {
        for &b in &self.bufs {
            crate::mem::free_page(b);
        }
    }
}