//! Ultar-Kern – Treiber „gen-ixgbe“ (Treiber-Werkstatt).
//! Gerät: Intel 10-Gigabit-Ethernet 82599 / X540 / X550 / E610 · Norm: öffentliche Intel-Datenblätter
//! („82599 10 GbE Controller Datasheet“, „X550 Datasheet“, „E610 Datasheet“): Register CTRL/CTRL_EXT/HLREG0/FCTRL,
//! SRRCTL/RXDCTL/TXDCTL/DMATXCTL, Empfangs-/Sendering mit Legacy-Deskriptoren, Abfragebetrieb.
//! E610: Verbindung und Status über das Admin Command Interface (ACI, Register PF_HIDA/PF_HIBA/PF_HICR).
//! ACI-Konstanten nach FreeBSD ix(4) (BSD-3, ixgbe_type_e610.h) – kein Linux-Code (GPL).
//! Zielgerät: Beelink GTi15 Ultra (server3) mit E610-XT2 (8086:57b0), 2 × 10GBASE-T.
//! Auftrag: werkstatt/auftraege/ixgbe.md · Status: ausgeschrieben, Prüfung am echten E610 über VFIO (siehe Auftrag).
//!
//! Ohne Kabel prüft der Treiber beim Start Sende- und Empfangsweg über die interne MAC-Schleife (HLREG0.LPBK).

use crate::driver::{Driver, NetDev, Result, NET};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

pub struct Drv;
pub static DRV: Drv = Drv;

/// E610 (ACI): Backplane, SFP, 10GBASE-T, 2,5G, SGMII
const E610: &[u16] = &[0x57ae, 0x57af, 0x57b0, 0x57b1, 0x57b2];
/// 82599, X540, X550/X550EM, X552, X553 (Verbindung über LINKS)
const ALT: &[u16] = &[
    0x10f7, 0x10f8, 0x10f9, 0x10fb, 0x10fc, 0x1507, 0x1514, 0x1517, 0x151c, 0x1528, 0x154a, 0x154d, 0x1557, 0x1558, 0x1560, 0x1563,
    0x15aa, 0x15ab, 0x15ac, 0x15ad, 0x15ae, 0x15c2, 0x15c3, 0x15c4, 0x15c6, 0x15c7, 0x15c8, 0x15ce, 0x15d1, 0x15e4, 0x15e5,
];

const CTRL: u64 = 0x00000;
const STATUS: u64 = 0x00008;
const CTRL_EXT: u64 = 0x00018;
const EICR: u64 = 0x00800;
const EIMC: u64 = 0x00888;
const EIMC_EX0: u64 = 0x00ab0;
const EIMC_EX1: u64 = 0x00ab4;
const EEC: u64 = 0x10010;
const RDBAL: u64 = 0x01000;
const RDBAH: u64 = 0x01004;
const RDLEN: u64 = 0x01008;
const RDH: u64 = 0x01010;
const RDT: u64 = 0x01018;
const RXDCTL: u64 = 0x01028;
const SRRCTL: u64 = 0x02100;
const RDRXCTL: u64 = 0x02f00;
const RXCTRL: u64 = 0x03000;
const HLREG0: u64 = 0x04240;
const LINKS: u64 = 0x042a4;
const MACC: u64 = 0x04330;
const DMATXCTL: u64 = 0x04a80;
const FCTRL: u64 = 0x05080;
const MTA: u64 = 0x05200;
const TDBAL: u64 = 0x06000;
const TDBAH: u64 = 0x06004;
const TDLEN: u64 = 0x06008;
const TDH: u64 = 0x06010;
const TDT: u64 = 0x06018;
const TXDCTL: u64 = 0x06028;
const RAL0: u64 = 0x0a200;
const RAH0: u64 = 0x0a204;

// E610 Admin Command Interface
const PF_HICR: u64 = 0x82048;
const HICR_EN: u32 = 1 << 0;
const HICR_C: u32 = 1 << 1;
const HICR_SV: u32 = 1 << 2;
const HICR_EV: u32 = 1 << 3;
fn pf_hida(i: usize) -> u64 {
    0x85000 + i as u64 * 4
}
fn pf_hida2(i: usize) -> u64 {
    0x85020 + i as u64 * 4
}
fn pf_hiba(i: usize) -> u64 {
    0x84000 + i as u64 * 4
}
const ACI_GET_VER: u16 = 0x0001;
const ACI_RESTART_AN: u16 = 0x0605;
const ACI_GET_LINK: u16 = 0x0607;

// Large descriptor rings absorb 10 Gb/s burst traffic instead of stalling
// the packet path behind a short software ring.
const N: usize = 256;
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
    e610: bool,
    /// E610: last link answer of the firmware (bit 31 up, low bits Mbit/s) and when it was asked
    link_cache: core::sync::atomic::AtomicU32,
    link_asked: core::sync::atomic::AtomicU64,
}

fn r(b: u64, o: u64) -> u32 {
    unsafe { core::ptr::read_volatile((b + o) as *const u32) }
}
fn w(b: u64, o: u64, v: u32) {
    unsafe { core::ptr::write_volatile((b + o) as *mut u32, v) }
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

/// ACI-Befehl ausführen. `params` = 16 Byte Befehlsparameter (Byte 16–31 des Deskriptors).
/// `buf`: indirekter Puffer – mit `an_fw` wird er an die Firmware geschickt, sonst füllt ihn die Antwort.
fn aci(m: u64, op: u16, params: [u32; 4], buf: Option<&mut [u8]>, an_fw: bool) -> core::result::Result<[u32; 8], String> {
    let hicr = r(m, PF_HICR);
    if hicr & HICR_EN == 0 {
        return Err("ACI ausgeschaltet".into());
    }
    if hicr & HICR_C != 0 {
        return Err("ACI belegt".into());
    }
    let len = buf.as_ref().map(|b| b.len()).unwrap_or(0).min(4096);
    let words = len.div_ceil(4);
    // Flags: SI (0x2000), BUF (0x1000), RD (0x400, Puffer an Firmware), LB (0x200, Puffer > 512 Byte)
    let mut flags: u32 = 0x2000;
    if len > 0 {
        flags |= 0x1000;
        if an_fw {
            flags |= 0x400;
        }
        if len > 512 {
            flags |= 0x200;
        }
    }
    if let (Some(b), true) = (buf.as_ref(), an_fw) {
        for i in 0..words {
            let mut x = [0xffu8; 4];
            for j in 0..4 {
                if i * 4 + j < len {
                    x[j] = b[i * 4 + j];
                }
            }
            w(m, pf_hiba(i), u32::from_le_bytes(x));
        }
    }
    let d = [flags | (op as u32) << 16, len as u32, 0, 0, params[0], params[1], params[2], params[3]];
    for (i, v) in d.iter().enumerate() {
        w(m, pf_hida(i), *v);
    }
    w(m, PF_HICR, (r(m, PF_HICR) | HICR_C) & !(HICR_SV | HICR_EV));
    // erst synchrone Antwort, dann ggf. verzögerte (Ereignis)
    wait(|| r(m, PF_HICR) & HICR_SV != 0 || r(m, PF_HICR) & HICR_C == 0, 1000);
    let mut h = r(m, PF_HICR);
    if h & HICR_SV != 0 && h & HICR_C != 0 {
        wait(|| r(m, PF_HICR) & HICR_EV != 0 || r(m, PF_HICR) & HICR_C == 0, 3000);
        h = r(m, PF_HICR);
    }
    let mut a = [0u32; 8];
    if h & HICR_SV != 0 {
        for (i, v) in a.iter_mut().enumerate() {
            *v = r(m, pf_hida(i));
        }
    } else if h & HICR_EV != 0 && h & HICR_C == 0 {
        for (i, v) in a.iter_mut().enumerate() {
            *v = r(m, pf_hida2(i));
        }
    }
    if h & HICR_C != 0 {
        return Err(format!("ACI {op:#06x}: keine Antwort (Zeit abgelaufen)"));
    }
    if h & (HICR_SV | HICR_EV) == 0 {
        return Err(format!("ACI {op:#06x}: Antwort fehlt"));
    }
    if (a[0] >> 16) as u16 != op {
        return Err(format!("ACI {op:#06x}: Antwort auf {:#06x}", a[0] >> 16));
    }
    let ret = a[1] >> 16;
    if ret != 0 {
        return Err(format!("ACI {op:#06x}: Firmware meldet Fehler {ret}"));
    }
    if let Some(b) = buf {
        if !an_fw && a[0] & 0x1000 != 0 {
            for i in 0..words {
                let v = r(m, pf_hiba(i)).to_le_bytes();
                for j in 0..4 {
                    if i * 4 + j < len {
                        b[i * 4 + j] = v[j];
                    }
                }
            }
        }
    }
    Ok(a)
}

/// E610: Verbindungsstatus über ACI (Antwortpuffer 56 Byte: Byte 2 link_info, Byte 10–11 link_speed)
fn e610_link(m: u64) -> core::result::Result<(bool, u32), String> {
    let mut b = [0u8; 56];
    aci(m, ACI_GET_LINK, [0; 4], Some(&mut b), false)?;
    let up = b[2] & 1 != 0;
    let sp = u16::from_le_bytes([b[10], b[11]]);
    let mbit = match sp & 0x7ff {
        x if x & (1 << 5) != 0 => 10_000,
        x if x & (1 << 4) != 0 => 5_000,
        x if x & (1 << 3) != 0 => 2_500,
        x if x & (1 << 2) != 0 => 1_000,
        x if x & (1 << 1) != 0 => 100,
        x if x & 1 != 0 => 10,
        _ => 0,
    };
    Ok((up, mbit))
}

/// 82599/X540/X550: LINKS.LINK_UP (Bit 30), Geschwindigkeit Bit 29:28
fn links(m: u64) -> (bool, u32) {
    let l = r(m, LINKS);
    let mbit = match (l >> 28) & 3 {
        1 => 100,
        2 => 1_000,
        3 => 10_000,
        _ => 0,
    };
    (l & (1 << 30) != 0, mbit)
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-ixgbe"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x8086 && d.class == 0x02 && (E610.contains(&d.device) || ALT.contains(&d.device))
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?;
        d.enable();
        let m = bar.addr;
        if r(m, STATUS) == 0xffff_ffff {
            return Err("Gerät antwortet nicht (BAR0 liest 0xffffffff)".into());
        }
        let e610 = E610.contains(&d.device);
        // Unterbrechungen aus, laufende DMA beenden (GIO_MASTER_DISABLE), dann Rücksetzen (CTRL.RST)
        w(m, EIMC, 0x7fff_ffff);
        w(m, EIMC_EX0, 0xffff_ffff);
        w(m, EIMC_EX1, 0xffff_ffff);
        w(m, CTRL, r(m, CTRL) | (1 << 2));
        wait(|| r(m, STATUS) & (1 << 19) == 0, 100);
        w(m, CTRL, r(m, CTRL) | (1 << 26));
        crate::arch::sleep_us(1_000);
        if !wait(|| r(m, CTRL) & (1 << 26) == 0, 1000) {
            return Err("Rücksetzen hängt".into());
        }
        crate::arch::sleep_us(100_000);
        wait(|| r(m, EEC) & (1 << 9) != 0, 500); // NVM automatisch gelesen
        w(m, EIMC, 0x7fff_ffff);
        w(m, EIMC_EX0, 0xffff_ffff);
        w(m, EIMC_EX1, 0xffff_ffff);
        let _ = r(m, EICR);
        // Treiber geladen (DRV_LOAD), kein Snoop-Verbot (NS_DIS)
        w(m, CTRL_EXT, r(m, CTRL_EXT) | (1 << 28) | (1 << 16));
        // MAC aus Empfangsadresse 0 (beim Rücksetzen aus dem NVM geladen)
        let (lo, hi) = (r(m, RAL0), r(m, RAH0));
        let mac = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
        if mac == [0; 6] || mac == [0xff; 6] {
            return Err("keine MAC-Adresse im NVM".into());
        }
        w(m, RAH0, (hi & 0xffff) | (1 << 31));
        for i in 0..128 {
            w(m, MTA + i * 4, 0);
        }
        // Empfang: CRC entfernen (HLREG0 und RDRXCTL gleich), Sende-CRC und Auffüllen an, keine Schleife
        w(m, RXCTRL, 0);
        w(m, HLREG0, (r(m, HLREG0) | 1 | 2 | (1 << 10)) & !(1 << 15));
        w(m, RDRXCTL, r(m, RDRXCTL) | 2);
        w(m, FCTRL, (r(m, FCTRL) | (1 << 10)) & !((1 << 8) | (1 << 9)));
        let rx_ring = alloc_pages(1).ok_or("kein Speicher")?;
        let rx_bufs = alloc_pages(N * BUF / 4096).ok_or("kein Speicher")?;
        for i in 0..N {
            unsafe {
                core::ptr::write_volatile((rx_ring + i as u64 * 16) as *mut u64, rx_bufs + (i * BUF) as u64);
                core::ptr::write_volatile((rx_ring + i as u64 * 16 + 8) as *mut u64, 0);
            }
        }
        // Legacy-Deskriptoren (DESCTYPE 0), Puffer 2 KB, bei Überlauf verwerfen (DROP_EN)
        w(m, SRRCTL, (BUF / 1024) as u32 | (1 << 28));
        w(m, RDBAL, rx_ring as u32);
        w(m, RDBAH, (rx_ring >> 32) as u32);
        w(m, RDLEN, (N * 16) as u32);
        w(m, RDH, 0);
        w(m, RDT, 0);
        w(m, RXDCTL, r(m, RXDCTL) | (1 << 25));
        if !wait(|| r(m, RXDCTL) & (1 << 25) != 0, 50) {
            return Err("Empfangsring startet nicht".into());
        }
        w(m, RDT, (N - 1) as u32);
        w(m, RXCTRL, r(m, RXCTRL) | 1);
        // Senden
        let tx_ring = alloc_pages(1).ok_or("kein Speicher")?;
        let tx_bufs = alloc_pages(N * BUF / 4096).ok_or("kein Speicher")?;
        unsafe { core::ptr::write_bytes(tx_ring as *mut u8, 0, 4096) };
        w(m, DMATXCTL, r(m, DMATXCTL) | 1);
        w(m, TDBAL, tx_ring as u32);
        w(m, TDBAH, (tx_ring >> 32) as u32);
        w(m, TDLEN, (N * 16) as u32);
        w(m, TDH, 0);
        w(m, TDT, 0);
        w(m, TXDCTL, r(m, TXDCTL) | (1 << 25));
        if !wait(|| r(m, TXDCTL) & (1 << 25) != 0, 50) {
            return Err("Sendering startet nicht".into());
        }
        // Verbindung
        let mut info = String::new();
        let (link, mbit) = if e610 {
            match aci(m, ACI_GET_VER, [0; 4], None, false) {
                Ok(a) => {
                    let fw = a[6].to_le_bytes();
                    let api = a[7].to_le_bytes();
                    info = format!(" · Firmware {}.{}.{} API {}.{}", fw[1], fw[2], fw[3], api[1], api[2]);
                }
                Err(e) => info = format!(" · {e}"),
            }
            // Verbindung freigeben und Aushandlung neu starten (LINK_RESTART | LINK_ENABLE in Byte 2 der Parameter)
            if let Err(e) = aci(m, ACI_RESTART_AN, [0x06 << 16, 0, 0, 0], None, false) {
                info += &format!(" · {e}");
            }
            let mut st = (false, 0);
            wait(
                || {
                    st = e610_link(m).unwrap_or((false, 0));
                    st.0
                },
                6000, // 10GBASE-T braucht für die Aushandlung einige Sekunden
            );
            if !st.0 {
                // Grund melden: Kabel/Gegenstelle (MEDIA_AVAILABLE, Byte 2 Bit 6) und größte Rahmenlänge als Lebenszeichen der Firmware
                let mut b = [0u8; 56];
                match aci(m, ACI_GET_LINK, [0; 4], Some(&mut b), false) {
                    Ok(_) => {
                        info += &format!(
                            " · {} (Rahmen bis {} Byte)",
                            if b[2] & (1 << 6) != 0 { "Kabel steckt" } else { "kein Kabel/keine Gegenstelle" },
                            u16::from_le_bytes([b[6], b[7]])
                        )
                    }
                    Err(e) => info += &format!(" · {e}"),
                }
            }
            st
        } else {
            let mut st = (false, 0);
            wait(
                || {
                    st = links(m);
                    st.0
                },
                3000,
            );
            st
        };
        let mut nl = NET.lock();
        let name = format!("eth{}", nl.len());
        let mut nic = Nic { name: name.clone(), mmio: m, mac, rx_ring, rx_bufs, tx_ring, tx_bufs, rx_next: 0, tx_next: 0, e610, link_cache: core::sync::atomic::AtomicU32::new(0), link_asked: core::sync::atomic::AtomicU64::new(0) };
        let verb = if link { format!("steht ({mbit} Mbit/s)") } else { format!("fehlt – Schleifentest: {}", nic.schleifentest()) };
        crate::kprintln!(
            "[ixgbe] {name}: {:04x} {} MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}{info} · Verbindung {verb}",
            d.device,
            if e610 { "E610" } else { "82599/X5xx" },
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );
        nl.push(Box::new(nic));
        Ok(())
    }
}

impl Nic {
    /// Sende-/Empfangsweg ohne Kabel prüfen: interne MAC-Schleife (HLREG0.LPBK, MACC.FLU), Rahmen an sich selbst
    fn schleifentest(&mut self) -> String {
        let m = self.mmio;
        let (h0, macc, fctrl) = (r(m, HLREG0), r(m, MACC), r(m, FCTRL));
        w(m, HLREG0, h0 | (1 << 15));
        w(m, MACC, macc | 1);
        w(m, FCTRL, fctrl | (1 << 8) | (1 << 9) | (1 << 10));
        crate::arch::sleep_us(20_000);
        let mut f = [0u8; 128];
        f[0..6].copy_from_slice(&self.mac);
        f[6..12].copy_from_slice(&self.mac);
        f[12] = 0x88;
        f[13] = 0xb5; // lokaler Versuchs-Ethertyp
        for (i, b) in f[14..].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7) ^ 0x5a;
        }
        let mut ergebnis = String::from("kein Rahmen zurück");
        'v: for _ in 0..3 {
            if let Err(e) = self.send(&f) {
                ergebnis = e;
                break;
            }
            let end = crate::arch::uptime_ms() + 200;
            let mut buf = [0u8; 2048];
            while crate::arch::uptime_ms() < end {
                if let Some(n) = self.recv(&mut buf) {
                    if n >= f.len() && buf[..f.len()] == f[..] {
                        ergebnis = String::from("ok (Senden und Empfangen geprüft)");
                        break 'v;
                    }
                    ergebnis = format!("falscher Rahmen ({n} Byte)");
                }
                crate::arch::pause();
            }
        }
        w(m, HLREG0, h0);
        w(m, MACC, macc);
        w(m, FCTRL, fctrl);
        ergebnis
    }
}

impl Nic {
    /// Link and speed; the E610 firmware is asked at most every two seconds.
    fn link_state(&self) -> (bool, u32) {
        use core::sync::atomic::Ordering::Relaxed;
        if !self.e610 {
            return links(self.mmio);
        }
        let now = crate::arch::uptime_ms();
        let asked = self.link_asked.load(Relaxed);
        if asked == 0 || now.saturating_sub(asked) >= 2000 {
            let (up, mbit) = e610_link(self.mmio).unwrap_or((false, 0));
            self.link_cache.store(u32::from(up) << 31 | mbit, Relaxed);
            self.link_asked.store(now.max(1), Relaxed);
        }
        let v = self.link_cache.load(Relaxed);
        (v >> 31 != 0, v & 0x7fff_ffff)
    }
}

impl NetDev for Nic {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn link(&self) -> Option<bool> {
        Some(self.link_state().0)
    }
    fn speed_mbit(&self) -> Option<u32> {
        let (up, mbit) = self.link_state();
        (up && mbit > 0).then_some(mbit)
    }
    fn driver(&self) -> &'static str {
        "gen-ixgbe"
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
            // vorheriger Auftrag in diesem Platz fertig? (Status DD in Byte 12)
            if core::ptr::read_volatile((d + 8) as *const u64) != 0 && core::ptr::read_volatile((d + 12) as *const u8) & 1 == 0 {
                if !wait(|| core::ptr::read_volatile((d + 12) as *const u8) & 1 != 0, 100) {
                    let _ = r(self.mmio, TDH);
                    return Err("Senden hängt".into());
                }
            }
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf as *mut u8, n);
            core::ptr::write_volatile(d as *mut u64, buf);
            core::ptr::write_volatile((d + 12) as *mut u8, 0);
            // Legacy: Länge, CMD = EOP | IFCS | RS (DEXT = 0), Status löschen
            core::ptr::write_volatile((d + 8) as *mut u64, n.max(60) as u64 | ((1u64 | 2 | 8) << 24));
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
            core::ptr::write_volatile(d as *mut u64, self.rx_bufs + (i * BUF) as u64);
            core::ptr::write_volatile((d + 8) as *mut u64, 0);
            core::ptr::write_volatile((d + 12) as *mut u8, 0);
        }
        self.rx_next = (i + 1) % N;
        w(self.mmio, RDT, i as u32);
        Some(n)
    }
}
