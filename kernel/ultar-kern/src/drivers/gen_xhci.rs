//! Ultar-Kern – Treiber „gen-xhci“ (Treiber-Werkstatt).
//! Gerät: USB-3-Hostcontroller (xHCI) · Norm: eXtensible Host Controller Interface 1.2 (Intel, öffentlich),
//! USB 2.0/3.2 Kapitel 9 (Geräteanmeldung), HID 1.11 (Boot-Tastatur), USB Mass Storage BOT 1.0 + SCSI (SPC/SBC).
//! Auftrag: werkstatt/auftraege/xhci.md · Status: ausgeschrieben, in QEMU (qemu-xhci, usb-kbd, usb-storage) geprüft.
//!
//! Abfragebetrieb ohne Interrupts: Befehlsring, ein Ereignisring, Transferring je Endpunkt.
//! Unterstützt: Übergabe vom BIOS, Rücksetzen der Anschlüsse, Geräte am Controller und hinter Hubs
//! (USB 2.0 Kap. 11 / USB 3.2 Kap. 10, bis 5 Ebenen, Übersetzer/TT für Low-/Full-Speed hinter High-Speed-Hubs),
//! Boot-Tastaturen (Zeichen landen in der Konsole) und USB-Sticks/Festplatten (als Datenträger `usbN`).

use crate::driver::{BlockDev, Driver, Result, BLOCK};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub mod bt;
mod hid;
mod hotplug;
mod pointer;
pub use hotplug::hotplug;

pub struct Drv;
pub static DRV: Drv = Drv;

fn r32(a: u64) -> u32 {
    unsafe { core::ptr::read_volatile(a as *const u32) }
}
fn w32(a: u64, v: u32) {
    unsafe { core::ptr::write_volatile(a as *mut u32, v) }
}
fn w64(a: u64, v: u64) {
    w32(a, v as u32);
    w32(a + 4, (v >> 32) as u32);
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

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_ADDRESS: u32 = 11;
const TRB_CONFIG_EP: u32 = 12;
const TRB_EVAL_CTX: u32 = 13;
const EV_TRANSFER: u32 = 32;
const EV_CMD: u32 = 33;
const RING: usize = 256;

/// Ring aus TRBs (16 Byte) mit Link-TRB am Ende
struct Ring {
    base: u64,
    idx: usize,
    cycle: u32,
}

impl Ring {
    fn new() -> Option<Ring> {
        Some(Ring { base: alloc_pages(1)?, idx: 0, cycle: 1 })
    }
    /// TRB einreihen; liefert seine Adresse
    fn push(&mut self, param: u64, status: u32, control: u32) -> u64 {
        let a = self.base + self.idx as u64 * 16;
        w64(a, param);
        w32(a + 8, status);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        w32(a + 12, control | self.cycle);
        self.idx += 1;
        if self.idx == RING - 1 {
            let l = self.base + (RING as u64 - 1) * 16;
            w64(l, self.base);
            w32(l + 8, 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            w32(l + 12, (TRB_LINK << 10) | 2 | self.cycle);
            self.idx = 0;
            self.cycle ^= 1;
        }
        a
    }
}

#[derive(Clone, Copy)]
struct Event {
    param: u64,
    status: u32,
    control: u32,
}

impl Event {
    fn typ(&self) -> u32 {
        (self.control >> 10) & 0x3f
    }
    fn code(&self) -> u32 {
        self.status >> 24
    }
    fn slot(&self) -> u32 {
        self.control >> 24
    }
    fn ep(&self) -> u32 {
        (self.control >> 16) & 0x1f
    }
}

struct Kbd {
    slot: u32,
    dci: u32,
    buf: u64,
    last: [u8; 8],
}

struct Hc {
    index: usize,
    /// Root ports.
    ports: u32,
    op: u64,
    rt: u64,
    db: u64,
    ctx: usize,
    dcbaa: u64,
    cmd: Ring,
    ev_base: u64,
    ev_idx: usize,
    ev_cycle: u32,
    /// Transferringe je (Slot, DCI)
    rings: Vec<(u32, u32, Ring)>,
    pending: VecDeque<Event>,
    kbds: Vec<Kbd>,
    keys: VecDeque<char>,
}

/// alle betriebenen Controller (Laptops haben oft mehrere: Chipsatz, Thunderbolt, Grafikkarte)
static HC: Mutex<Vec<Hc>> = Mutex::new(Vec::new());
static NEXT_HC: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

impl Hc {
    fn ring(&mut self, slot: u32, dci: u32) -> &mut Ring {
        let i = self.rings.iter().position(|r| r.0 == slot && r.1 == dci).unwrap();
        &mut self.rings[i].2
    }

    /// nächstes Ereignis vom Ereignisring holen
    fn next_event(&mut self) -> Option<Event> {
        let a = self.ev_base + self.ev_idx as u64 * 16;
        let control = r32(a + 12);
        if control & 1 != self.ev_cycle {
            return None;
        }
        let e = Event { param: r32(a) as u64 | (r32(a + 4) as u64) << 32, status: r32(a + 8), control };
        self.ev_idx += 1;
        if self.ev_idx == RING {
            self.ev_idx = 0;
            self.ev_cycle ^= 1;
        }
        let dq = self.ev_base + self.ev_idx as u64 * 16;
        w64(self.rt + 0x20 + 0x18, dq | 8);
        Some(e)
    }

    /// Ereignisse verarbeiten (Tastatur sofort), andere zurücklegen
    fn pump(&mut self) {
        while let Some(e) = self.next_event() {
            if e.typ() == EV_TRANSFER {
                if let Some(k) = self.kbds.iter().position(|k| k.slot == e.slot() && k.dci == e.ep()) {
                    self.kbd_report(k, e.code());
                    continue;
                }
                if pointer::report(self, (e.slot(), e.ep()), e.code(), e.status & 0xff_ffff) {
                    continue;
                }
                if bt::report(self, (e.slot(), e.ep()), e.param, e.code(), e.status & 0xff_ffff) {
                    continue;
                }
            }
            if e.typ() == EV_TRANSFER || e.typ() == EV_CMD {
                self.pending.push_back(e);
            }
        }
    }

    fn wait_for(&mut self, mut f: impl FnMut(&Event) -> bool, ms: u64) -> Option<Event> {
        let end = crate::arch::uptime_ms() + ms;
        loop {
            self.pump();
            if let Some(i) = self.pending.iter().position(|e| f(e)) {
                return self.pending.remove(i);
            }
            if crate::arch::uptime_ms() > end {
                return None;
            }
            crate::arch::pause();
        }
    }

    fn command(&mut self, param: u64, control: u32) -> Option<Event> {
        let a = self.cmd.push(param, 0, control);
        w32(self.db, 0);
        let e = self.wait_for(|e| e.typ() == EV_CMD && e.param == a, 1000)?;
        (e.code() == 1).then_some(e)
    }

    /// Steuerübertragung auf Endpunkt 0
    fn control(&mut self, slot: u32, rt: u8, req: u8, value: u16, index: u16, len: u16, buf: u64) -> Option<usize> {
        let setup = rt as u64 | (req as u64) << 8 | (value as u64) << 16 | (index as u64) << 32 | (len as u64) << 48;
        let input = rt & 0x80 != 0;
        let trt = if len == 0 { 0 } else if input { 3 } else { 2 };
        let ring = self.ring(slot, 1);
        ring.push(setup, 8, (TRB_SETUP << 10) | (1 << 6) | (trt << 16));
        if len > 0 {
            ring.push(buf, len as u32, (TRB_DATA << 10) | if input { 1 << 16 } else { 0 });
        }
        let st = ring.push(0, 0, (TRB_STATUS << 10) | (1 << 5) | if len == 0 || !input { 1 << 16 } else { 0 });
        w32(self.db + slot as u64 * 4, 1);
        let e = self.wait_for(|e| e.typ() == EV_TRANSFER && e.slot() == slot && (e.param == st || e.code() != 1), 2000)?;
        if e.code() == 6 {
            // STALL (Gerät lehnt die Anfrage ab): Endpunkt 0 wieder freigeben, sonst schlagen alle weiteren Anfragen fehl
            self.reset_ep(slot, 1);
            return None;
        }
        matches!(e.code(), 1 | 13).then_some(len as usize - (e.status & 0xff_ffff) as usize)
    }

    /// Angehaltenen Endpunkt zurücksetzen (Reset Endpoint) und den Ring an die aktuelle Schreibstelle setzen
    fn reset_ep(&mut self, slot: u32, dci: u32) {
        self.command(0, (14 << 10) | (dci << 16) | (slot << 24));
        let r = self.ring(slot, dci);
        let deq = (r.base + r.idx as u64 * 16) | r.cycle as u64;
        self.command(deq, (16 << 10) | (dci << 16) | (slot << 24));
    }

    /// Bulk-/Interrupt-Übertragung (seitenweise verkettet), liefert übertragene Bytes
    fn transfer(&mut self, slot: u32, dci: u32, buf: u64, len: usize, ms: u64) -> Option<usize> {
        let ring = self.ring(slot, dci);
        let mut off = 0usize;
        let mut last;
        loop {
            let a = buf + off as u64;
            let n = (4096 - (a & 0xfff) as usize).min(len - off);
            let end = off + n >= len;
            last = ring.push(a, n as u32, (TRB_NORMAL << 10) | if end { 1 << 5 } else { (1 << 4) | (1 << 2) });
            off += n;
            if end {
                break;
            }
        }
        let _ = last;
        w32(self.db + slot as u64 * 4, dci);
        let e = self.wait_for(|e| e.typ() == EV_TRANSFER && e.slot() == slot && e.ep() == dci, ms)?;
        if !matches!(e.code(), 1 | 13) {
            return None;
        }
        Some(len - (e.status & 0xff_ffff) as usize)
    }

    fn kbd_report(&mut self, k: usize, code: u32) {
        let (slot, dci, buf) = (self.kbds[k].slot, self.kbds[k].dci, self.kbds[k].buf);
        if matches!(code, 1 | 13) {
            let mut rep = [0u8; 8];
            unsafe { core::ptr::copy_nonoverlapping(buf as *const u8, rep.as_mut_ptr(), 8) };
            let last = self.kbds[k].last;
            let (shift, altgr, ctrl) = (rep[0] & 0x22 != 0, rep[0] & 0x40 != 0, rep[0] & 0x11 != 0);
            // a program reads the keyboard's input node: press and release go there, not to the console
            let raw = crate::input::usb_report(self.index, slot, &rep, crate::eingabe::roh());
            for &u in rep[2..8].iter().filter(|_| !raw) {
                if u > 3 && !last[2..8].contains(&u) {
                    for c in hid_chars(u, shift, altgr, ctrl) {
                        self.keys.push_back(c);
                    }
                }
            }
            self.kbds[k].last = rep;
        }
        // nächsten Bericht anfordern
        let ring = self.ring(slot, dci);
        ring.push(buf, 8, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
        w32(self.db + slot as u64 * 4, dci);
    }
}

/// HID-Verwendung → Zeichen (deutsch/US wie die PS/2-Tastatur)
fn hid_chars(u: u8, shift: bool, altgr: bool, ctrl: bool) -> Vec<char> {
    let de = crate::drivers::ps2kbd::GERMAN.load(core::sync::atomic::Ordering::Relaxed);
    let mut v = Vec::new();
    let c = match u {
        4..=29 => {
            let mut b = b'a' + (u - 4);
            if de && b == b'y' {
                b = b'z';
            } else if de && b == b'z' {
                b = b'y';
            }
            if ctrl {
                v.push((b - b'a' + 1) as char);
                return v;
            }
            if altgr && de && b == b'q' {
                '@'
            } else if shift {
                (b - 32) as char
            } else {
                b as char
            }
        }
        30..=39 => {
            let n = (u - 30 + 1) % 10;
            if altgr && de {
                match n {
                    7 => '{',
                    8 => '[',
                    9 => ']',
                    0 => '}',
                    2 => '²',
                    _ => return v,
                }
            } else if shift {
                if de {
                    ['=', '!', '"', '§', '$', '%', '&', '/', '(', ')'][n as usize]
                } else {
                    [')', '!', '@', '#', '$', '%', '^', '&', '*', '('][n as usize]
                }
            } else {
                (b'0' + n) as char
            }
        }
        40 => '\r',
        41 => '\x1b',
        42 => '\x08',
        43 => '\t',
        44 => ' ',
        45 => if de { if altgr { '\\' } else if shift { '?' } else { 'ß' } } else if shift { '_' } else { '-' },
        46 => if de { if shift { '`' } else { '´' } } else if shift { '+' } else { '=' },
        47 => if de { if shift { 'Ü' } else { 'ü' } } else if shift { '{' } else { '[' },
        48 => if de { if altgr { '~' } else if shift { '*' } else { '+' } } else if shift { '}' } else { ']' },
        49 | 50 => if de { if shift { '\'' } else { '#' } } else if shift { '|' } else { '\\' },
        51 => if de { if shift { 'Ö' } else { 'ö' } } else if shift { ':' } else { ';' },
        52 => if de { if shift { 'Ä' } else { 'ä' } } else if shift { '"' } else { '\'' },
        53 => if de { if shift { '°' } else { '^' } } else if shift { '~' } else { '`' },
        54 => if shift { if de { ';' } else { '<' } } else { ',' },
        55 => if shift { if de { ':' } else { '>' } } else { '.' },
        56 => if de { if shift { '_' } else { '-' } } else if shift { '?' } else { '/' },
        100 => if de { if altgr { '|' } else if shift { '>' } else { '<' } } else { '\\' },
        79 | 80 | 81 | 82 => {
            v.extend(['\x1b', '[', ['C', 'D', 'B', 'A'][(u - 79) as usize]]);
            return v;
        }
        76 => {
            v.extend(['\x1b', '[', '3', '~']);
            return v;
        }
        74 | 77 => {
            v.extend(['\x1b', '[', if u == 74 { 'H' } else { 'F' }]);
            return v;
        }
        _ => return v,
    };
    v.push(c);
    v
}

/// Zeichen von USB-Tastaturen (für Konsole und Terminal)
pub fn poll_key() -> Option<char> {
    let mut g = HC.try_lock()?;
    for hc in g.iter_mut() {
        if hc.keys.is_empty() && (!hc.kbds.is_empty() || pointer::any(hc.index) || bt::any(hc.index)) {
            hc.pump();
        }
        if let Some(c) = hc.keys.pop_front() {
            return Some(c);
        }
    }
    None
}

// ------------------------------------------------------------------ Geräte anmelden

struct Desc {
    class: u8,
    sub: u8,
    proto: u8,
    iface: u8,
    config: u8,
    eps: Vec<(u8, u8, u16, u8)>, // (Adresse, Attribute, max. Paket, Intervall)
}

/// Kontext-Zeiger: Eingabekontext hat einen Steuerkontext vor dem Slot-Kontext
fn ictx(base: u64, ctx: usize, idx: usize) -> u64 {
    base + (idx * ctx) as u64
}

fn enable_port(hc: &Hc, port: u32) -> Option<u32> {
    let pr = hc.op + 0x400 + (port as u64 - 1) * 0x10;
    let sc = r32(pr);
    if sc & 1 == 0 {
        return None;
    }
    // RW1C-Bits nicht versehentlich löschen: nur PP behalten
    let keep = sc & 0x0e00_c3e0 & !(1 << 1) & !(0x1f << 17);
    if sc & 2 == 0 {
        w32(pr, keep | (1 << 4)); // Rücksetzen
        wait(|| r32(pr) & (1 << 21) != 0 || r32(pr) & 2 != 0, 500);
        crate::arch::sleep_us(20_000);
    }
    let sc = r32(pr);
    w32(pr, (sc & 0x0e00_c3e0 & !(1 << 1)) | (0x7f << 17)); // Änderungsbits quittieren
    if sc & 2 == 0 {
        return None;
    }
    Some((sc >> 10) & 0xf)
}

/// Lage eines Geräts im USB-Baum: Anschluss am Controller, Route (4 Bit je Hub-Ebene), Tiefe,
/// Übersetzer (TT: Slot und Anschluss des High-Speed-Hubs) für Low-/Full-Speed-Geräte
#[derive(Clone, Copy)]
struct Lage {
    root: u32,
    route: u32,
    depth: u32,
    tt_slot: u32,
    tt_port: u32,
}

impl Lage {
    fn wurzel(port: u32) -> Lage {
        Lage { root: port, route: 0, depth: 0, tt_slot: 0, tt_port: 0 }
    }
    fn name(&self) -> String {
        let mut s = format!("{}", self.root);
        for i in 0..self.depth {
            s += &format!(".{}", (self.route >> (4 * i)) & 0xf);
        }
        s
    }
}

/// Hub: Anschlüsse mit Strom versorgen, belegte zurücksetzen und die Geräte dahinter anmelden
fn hub_ports(hc: &mut Hc, hub: u32, ports: u32, pwr_ms: u64, ss: bool, speed: u32, lage: Lage) {
    let Some(buf) = alloc_pages(1) else { return };
    if lage.depth >= 5 {
        return crate::kprintln!("[xhci] Hub {}: zu tief verschachtelt", lage.name());
    }
    for p in 1..=ports {
        hc.control(hub, 0x23, 3, 8, p as u16, 0, 0); // SET_FEATURE PORT_POWER
    }
    crate::arch::sleep_us((pwr_ms.max(20) + 100) * 1000);
    let status = |hc: &mut Hc, p: u32| -> Option<(u32, u32)> {
        hc.control(hub, 0xA3, 0, 0, p as u16, 4, buf)?;
        let v = r32(buf);
        Some((v & 0xffff, v >> 16))
    };
    for p in 1..=ports {
        let Some((s, _)) = status(hc, p) else {
            crate::kprintln!("[xhci] Hub {} Anschluss {p}: Status nicht lesbar", lage.name());
            continue;
        };
        if s & 1 == 0 {
            continue;
        }
        hc.control(hub, 0x23, 3, 4, p as u16, 0, 0); // SET_FEATURE PORT_RESET
        let mut ok = false;
        for _ in 0..50 {
            crate::arch::sleep_us(10_000);
            if let Some((s, c)) = status(hc, p) {
                if s & (1 << 4) == 0 && (c & (1 << 4) != 0 || s & 2 != 0) {
                    ok = s & 2 != 0;
                    break;
                }
            }
        }
        // gesetzte Änderungsbits quittieren (nur diese – unbekannte Merkmale lehnt ein Hub mit STALL ab):
        // C_PORT_CONNECTION, C_PORT_ENABLE, C_PORT_RESET, (USB 3) C_BH_PORT_RESET, C_PORT_LINK_STATE, C_PORT_CONFIG_ERROR
        let c = status(hc, p).map(|x| x.1).unwrap_or(0);
        for (bit, f) in [(0u32, 16u16), (1, 17), (4, 20), (5, 29), (6, 25), (7, 26)] {
            if c & (1 << bit) != 0 && (ss || bit < 5) {
                hc.control(hub, 0x23, 1, f, p as u16, 0, 0);
            }
        }
        let mut kind = lage;
        kind.route = lage.route | (p.min(15) << (4 * lage.depth));
        kind.depth = lage.depth + 1;
        if !ok {
            crate::kprintln!("[xhci] Port {}: Rücksetzen am Hub fehlgeschlagen", kind.name());
            continue;
        }
        let s = status(hc, p).map(|x| x.0).unwrap_or(0);
        // Geschwindigkeit: USB-3-Hub → SuperSpeed; USB-2-Hub: Bit 9 Low, Bit 10 High, sonst Full
        let cs = if ss { 4 } else if s & (1 << 9) != 0 { 2 } else if s & (1 << 10) != 0 { 3 } else { 1 };
        if cs <= 2 && speed == 3 {
            kind.tt_slot = hub;
            kind.tt_port = p;
        }
        crate::arch::sleep_us(10_000); // Erholzeit nach dem Rücksetzen (USB 2.0 7.1.7.3)
        match attach(hc, kind, cs) {
            Ok(w) => crate::kprintln!("[xhci] Port {}: {w}", kind.name()),
            Err(e) => crate::kprintln!("[xhci] Port {}: {e}", kind.name()),
        }
    }
}

fn attach(hc: &mut Hc, lage: Lage, speed: u32) -> core::result::Result<String, String> {
    let port = lage.root;
    let ev = hc.command(0, TRB_ENABLE_SLOT << 10).ok_or("Slot freigeben fehlgeschlagen")?;
    let slot = ev.slot();
    let ctx = hc.ctx;
    let out = alloc_pages(1).ok_or("kein Speicher")?;
    let inp = alloc_pages(1).ok_or("kein Speicher")?;
    w64(hc.dcbaa + slot as u64 * 8, out);
    let ep0 = Ring::new().ok_or("kein Speicher")?;
    let ep0_base = ep0.base;
    hc.rings.push((slot, 1, ep0));
    let mps: u32 = match speed {
        2 => 8,
        1 => 8,
        3 => 64,
        _ => 512,
    };
    // Eingabe-Steuerkontext: Slot (A0) und EP0 (A1) hinzufügen
    w32(inp + 4, 0b11);
    let sc = ictx(inp, ctx, 1);
    w32(sc, (1 << 27) | (speed << 20) | (lage.route & 0xf_ffff));
    w32(sc + 4, port << 16);
    w32(sc + 8, lage.tt_slot | (lage.tt_port << 8));
    let e0 = ictx(inp, ctx, 2);
    w32(e0 + 4, (3 << 1) | (4 << 3) | (mps << 16));
    w64(e0 + 8, ep0_base | 1);
    w32(e0 + 16, 8);
    hc.command(inp, (TRB_ADDRESS << 10) | (slot << 24)).ok_or("Adresse vergeben fehlgeschlagen")?;
    let buf = alloc_pages(1).ok_or("kein Speicher")?;
    // Gerätebeschreibung (erst 8 Byte für die Paketgröße von EP0)
    hc.control(slot, 0x80, 6, 0x0100, 0, 8, buf).ok_or("Gerätebeschreibung fehlt")?;
    let real = unsafe { *((buf + 7) as *const u8) } as u32;
    if speed <= 2 && real != mps && real != 0 {
        unsafe { core::ptr::write_bytes(inp as *mut u8, 0, 4096) };
        w32(inp + 4, 0b10);
        w32(e0 + 4, (3 << 1) | (4 << 3) | (real << 16));
        hc.command(inp, (TRB_EVAL_CTX << 10) | (slot << 24));
    }
    hc.control(slot, 0x80, 6, 0x0100, 0, 18, buf).ok_or("Gerätebeschreibung fehlt")?;
    let dd: Vec<u8> = unsafe { core::slice::from_raw_parts(buf as *const u8, 18) }.to_vec();
    let (vid, pid) = (u16::from_le_bytes([dd[8], dd[9]]), u16::from_le_bytes([dd[10], dd[11]]));
    // Konfiguration lesen
    hc.control(slot, 0x80, 6, 0x0200, 0, 9, buf).ok_or("Konfiguration fehlt")?;
    let total = unsafe { core::ptr::read_unaligned((buf + 2) as *const u16) }.min(4096);
    hc.control(slot, 0x80, 6, 0x0200, 0, total, buf).ok_or("Konfiguration fehlt")?;
    let cd: Vec<u8> = unsafe { core::slice::from_raw_parts(buf as *const u8, total as usize) }.to_vec();
    let cfg_val = cd[5];
    let mut ifs: Vec<Desc> = Vec::new();
    let mut o = 0usize;
    while o + 2 <= cd.len() {
        let l = cd[o] as usize;
        if l < 2 || o + l > cd.len() {
            break;
        }
        match cd[o + 1] {
            4 if l >= 9 => ifs.push(Desc { class: cd[o + 5], sub: cd[o + 6], proto: cd[o + 7], iface: cd[o + 2], config: cfg_val, eps: Vec::new() }),
            5 if l >= 7 => {
                if let Some(d) = ifs.last_mut() {
                    d.eps.push((cd[o + 2], cd[o + 3], u16::from_le_bytes([cd[o + 4], cd[o + 5]]) & 0x7ff, cd[o + 6]));
                }
            }
            _ => {}
        }
        o += l;
    }
    hc.control(slot, 0x00, 9, cfg_val as u16, 0, 0, 0).ok_or("Konfiguration setzen fehlgeschlagen")?;
    let mut what = format!("{vid:04x}:{pid:04x}");
    let mut hub: Option<(u32, u64, bool)> = None;
    let mut bt_bound = false;
    for d in &ifs {
        if d.class == 9 && hub.is_none() {
            // Hub (USB 2.0 Kap. 11 / USB 3.2 Kap. 10)
            let ss = speed >= 4;
            let typ: u16 = if ss { 0x2A } else { 0x29 };
            hc.control(slot, 0xA0, 6, typ << 8, 0, 12, buf).ok_or("Hub-Beschreibung fehlt")?;
            let nports = unsafe { *((buf + 2) as *const u8) } as u32;
            let chars = unsafe { core::ptr::read_unaligned((buf + 3) as *const u16) } as u32;
            let pwr = unsafe { *((buf + 5) as *const u8) } as u64 * 2;
            let ttt = if ss { 0 } else { (chars >> 5) & 3 };
            let mtt = !ss && d.proto == 2;
            let eps: Vec<(u32, u32, u32, u8)> = d
                .eps
                .iter()
                .filter(|e| e.0 & 0x80 != 0 && e.1 & 3 == 3)
                .map(|&(a, _, mp, iv)| ((a as u32 & 0xf) * 2 + 1, 7, mp as u32, iv))
                .take(1)
                .collect();
            config_ep_x(hc, slot, inp, out, speed, &eps, Some((nports, mtt, ttt)))?;
            if ss {
                hc.control(slot, 0x20, 12, lage.depth as u16, 0, 0, 0); // SET_HUB_DEPTH
            }
            what = format!("Hub mit {nports} Anschlüssen {what}");
            hub = Some((nports, pwr, ss));
        } else if d.class == 3 && d.sub == 1 && d.proto == 1 {
            // Boot-Tastatur
            let Some(&(addr, _, mp, interval)) = d.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 3) else { continue };
            let dci = (addr as u32 & 0xf) * 2 + 1;
            config_ep(hc, slot, inp, out, speed, &[(dci, 7, mp as u32, interval)])?;
            hc.control(slot, 0x21, 0x0B, 0, d.iface as u16, 0, 0); // SET_PROTOCOL: Boot
            hc.control(slot, 0x21, 0x0A, 0, d.iface as u16, 0, 0); // SET_IDLE
            let kb = alloc_pages(1).ok_or("kein Speicher")?;
            hc.kbds.push(Kbd { slot, dci, buf: kb, last: [0; 8] });
            let ring = hc.ring(slot, dci);
            ring.push(kb, 8, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
            w32(hc.db + slot as u64 * 4, dci);
            what = format!("Tastatur {what}");
        } else if d.class == 3 {
            // mouse or tablet (report descriptor with X and Y)
            if let Some(kind) = pointer::attach(hc, slot, (inp, out, speed), d)? {
                what = format!("{kind} {what}");
            }
        } else if bt::is_bt(d) && !bt_bound {
            // Bluetooth primary controller: interface 0 (events, ACL); the SCO interface has no
            // interrupt/bulk endpoints and is left alone
            match bt::attach(hc, slot, (inp, out, speed), d) {
                Ok(n) => {
                    bt_bound = true;
                    what = format!("Bluetooth hci{n} {what}");
                }
                Err(e) if d.eps.iter().any(|e| e.1 & 3 == 3) => crate::kprintln!("[xhci] {e}"),
                Err(_) => {}
            }
        } else if d.class == 8 && d.sub == 6 && d.proto == 0x50 {
            // Massenspeicher (Bulk-Only, SCSI)
            let Some(&(ina, _, inmp, _)) = d.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 2) else { continue };
            let Some(&(outa, _, outmp, _)) = d.eps.iter().find(|e| e.0 & 0x80 == 0 && e.1 & 3 == 2) else { continue };
            let (din, dout) = ((ina as u32 & 0xf) * 2 + 1, (outa as u32 & 0xf) * 2);
            config_ep(hc, slot, inp, out, speed, &[(din, 6, inmp as u32, 0), (dout, 2, outmp as u32, 0)])?;
            let _ = d.config;
            let disk = UsbDisk::new(hc, slot, din, dout)?;
            what = format!("Datenträger {} {} MB ({what})", disk.name, disk.sectors * disk.bsize as u64 >> 20);
            BLOCK.lock().push(Box::new(disk));
        }
    }
    hotplug::register(hc, &lage, slot, speed, &dd, &ifs);
    if let Some((nports, pwr, ss)) = hub {
        crate::kprintln!("[xhci] Port {}: {what}", lage.name());
        hub_ports(hc, slot, nports, pwr, ss, speed, lage);
        return Ok(format!("Hub {vid:04x}:{pid:04x} fertig"));
    }
    Ok(what)
}

/// Endpunkte einrichten: (DCI, Typ, max. Paket, Intervall)
fn config_ep(hc: &mut Hc, slot: u32, inp: u64, out: u64, speed: u32, eps: &[(u32, u32, u32, u8)]) -> core::result::Result<(), String> {
    config_ep_x(hc, slot, inp, out, speed, eps, None)
}

/// wie `config_ep`, für Hubs zusätzlich Slot-Kontext: Hub-Bit, MTT, Zahl der Anschlüsse, TT-Denkzeit
fn config_ep_x(
    hc: &mut Hc,
    slot: u32,
    inp: u64,
    out: u64,
    speed: u32,
    eps: &[(u32, u32, u32, u8)],
    hub: Option<(u32, bool, u32)>,
) -> core::result::Result<(), String> {
    let ctx = hc.ctx;
    unsafe { core::ptr::write_bytes(inp as *mut u8, 0, 4096) };
    let mut add = 1u32;
    let mut maxdci = 1;
    for &(dci, _, _, _) in eps {
        add |= 1 << dci;
        maxdci = maxdci.max(dci);
    }
    w32(inp + 4, add);
    // Slot-Kontext aus dem Ausgabekontext übernehmen, Zahl der Kontexte anpassen
    let sc = ictx(inp, ctx, 1);
    for i in 0..4 {
        w32(sc + i * 4, r32(out + i * 4));
    }
    w32(sc, (r32(sc) & !(0x1f << 27)) | (maxdci.max((r32(out) >> 27) & 0x1f) << 27));
    if let Some((nports, mtt, ttt)) = hub {
        w32(sc, r32(sc) | (1 << 26) | if mtt { 1 << 25 } else { 0 });
        w32(sc + 4, (r32(sc + 4) & 0x00ff_ffff) | (nports.min(255) << 24));
        w32(sc + 8, (r32(sc + 8) & !(3 << 16)) | (ttt << 16));
    }
    for &(dci, typ, mp, interval) in eps {
        let ring = Ring::new().ok_or("kein Speicher")?;
        let base = ring.base;
        hc.rings.push((slot, dci, ring));
        let ec = ictx(inp, ctx, dci as usize + 1);
        let iv = if typ == 7 {
            // Intervall in 125-µs-Einheiten (2^n): Voll-/Niedriggeschwindigkeit in ms → ×8
            let us = if speed <= 2 { (interval.max(1) as u32) * 8 } else { 1 << (interval.clamp(1, 16) - 1) };
            31 - us.max(1).leading_zeros()
        } else {
            0
        };
        w32(ec, iv << 16);
        w32(ec + 4, (3 << 1) | (typ << 3) | ((mp & 0x7ff) << 16));
        w64(ec + 8, base | 1);
        // interrupt: Max ESIT Payload (bits 31:16, one packet times the high-speed multiplier) and
        // the average TRB length; a periodic endpoint without its payload gets no bandwidth on
        // some host controllers
        let esit = (mp & 0x7ff) * (((mp >> 11) & 3) + 1);
        w32(ec + 16, if typ == 7 { (esit << 16) | (mp & 0x7ff).min(8) } else { 1024 });
    }
    hc.command(inp, (TRB_CONFIG_EP << 10) | (slot << 24)).ok_or("Endpunkte einrichten fehlgeschlagen")?;
    Ok(())
}

// ------------------------------------------------------------------ USB-Massenspeicher

struct UsbDisk {
    name: String,
    hc: usize,
    slot: u32,
    din: u32,
    dout: u32,
    sectors: u64,
    bsize: usize,
    buf: u64,
    tag: u32,
}

impl UsbDisk {
    fn new(hc: &mut Hc, slot: u32, din: u32, dout: u32) -> core::result::Result<UsbDisk, String> {
        let buf = alloc_pages(17).ok_or("kein Speicher")?;
        let n = BLOCK.lock().len();
        let mut d = UsbDisk { name: format!("usb{n}"), hc: hc.index, slot, din, dout, sectors: 0, bsize: 512, buf, tag: 1 };
        // Gerät bereit? (TEST UNIT READY, ggf. mehrfach)
        for _ in 0..10 {
            if d.scsi(hc, &[0u8; 6], 0, false).is_ok() {
                break;
            }
            crate::arch::sleep_us(100_000);
        }
        let mut cap = [0u8; 16];
        cap[0] = 0x25;
        d.scsi(hc, &cap[..10], 8, true).map_err(|e| format!("Kapazität: {e}"))?;
        let b = unsafe { core::slice::from_raw_parts(d.buf as *const u8, 8) };
        let last = u32::from_be_bytes(b[0..4].try_into().unwrap()) as u64;
        d.bsize = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
        d.sectors = last + 1;
        if d.bsize == 0 || d.bsize > 4096 {
            return Err(format!("Blockgröße {}", d.bsize));
        }
        Ok(d)
    }

    /// SCSI-Befehl über Bulk-Only: CBW → Daten (im Puffer) → CSW
    fn scsi(&mut self, hc: &mut Hc, cb: &[u8], len: usize, input: bool) -> core::result::Result<usize, String> {
        let cbw = self.buf + 64 * 1024;
        unsafe { core::ptr::write_bytes(cbw as *mut u8, 0, 64) };
        self.tag = self.tag.wrapping_add(1);
        w32(cbw, 0x4342_5355);
        w32(cbw + 4, self.tag);
        w32(cbw + 8, len as u32);
        unsafe {
            *((cbw + 12) as *mut u8) = if input { 0x80 } else { 0 };
            *((cbw + 14) as *mut u8) = cb.len() as u8;
            core::ptr::copy_nonoverlapping(cb.as_ptr(), (cbw + 15) as *mut u8, cb.len());
        }
        hc.transfer(self.slot, self.dout, cbw, 31, 2000).ok_or("CBW")?;
        let mut got = 0;
        if len > 0 {
            got = hc.transfer(self.slot, if input { self.din } else { self.dout }, self.buf, len, 5000).ok_or("Daten")?;
        }
        let csw = cbw + 512;
        hc.transfer(self.slot, self.din, csw, 13, 2000).ok_or("CSW")?;
        if r32(csw) != 0x5342_5355 {
            return Err("CSW ungültig".into());
        }
        let status = unsafe { *((csw + 12) as *const u8) };
        if status != 0 {
            return Err(format!("SCSI-Status {status}"));
        }
        Ok(got)
    }

    fn rw(&mut self, write: bool, lba: u64, n: usize) -> core::result::Result<(), String> {
        let mut cb = [0u8; 10];
        cb[0] = if write { 0x2A } else { 0x28 };
        cb[2..6].copy_from_slice(&(lba as u32).to_be_bytes());
        cb[7..9].copy_from_slice(&(n as u16).to_be_bytes());
        let mut g = HC.lock();
        let hc = g.iter_mut().find(|h| h.index == self.hc).ok_or("kein Controller")?;
        self.scsi(hc, &cb, n * self.bsize, !write).map(|_| ())
    }
}

impl BlockDev for UsbDisk {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn sectors(&self) -> u64 {
        self.sectors * self.bsize as u64 / 512
    }
    fn sector_size(&self) -> usize {
        512
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        // Kern rechnet in 512-Byte-Sektoren; Gerät evtl. größer
        let per = self.bsize / 512;
        for (i, chunk) in buf.chunks_mut(32 * 1024).enumerate() {
            let s = lba + (i * 64) as u64;
            if per <= 1 {
                self.rw(false, s, chunk.len() / 512)?;
                unsafe { core::ptr::copy_nonoverlapping(self.buf as *const u8, chunk.as_mut_ptr(), chunk.len()) };
            } else {
                let first = s / per as u64;
                let last = (s + (chunk.len() / 512) as u64).div_ceil(per as u64);
                self.rw(false, first, (last - first) as usize)?;
                let off = ((s % per as u64) * 512) as usize;
                unsafe { core::ptr::copy_nonoverlapping((self.buf + off as u64) as *const u8, chunk.as_mut_ptr(), chunk.len()) };
            }
        }
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<()> {
        if self.bsize != 512 {
            return Err("Schreiben nur bei 512-Byte-Sektoren".into());
        }
        for (i, chunk) in buf.chunks(32 * 1024).enumerate() {
            unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.buf as *mut u8, chunk.len()) };
            self.rw(true, lba + (i * 64) as u64, chunk.len() / 512)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ Controller

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-xhci"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x0c && d.subclass == 0x03 && d.prog_if == 0x30
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?;
        d.enable();
        let base = bar.addr;
        let caplen = (r32(base) & 0xff) as u64;
        let hcs1 = r32(base + 4);
        let hcs2 = r32(base + 8);
        let hcc1 = r32(base + 0x10);
        let op = base + caplen;
        let db = base + (r32(base + 0x14) & !3) as u64;
        let rt = base + (r32(base + 0x18) & !0x1f) as u64;
        let max_slots = (hcs1 & 0xff).min(32);
        let ports = hcs1 >> 24;
        let ctx = if hcc1 & 4 != 0 { 64 } else { 32 };
        // Übergabe vom BIOS (erweiterte Fähigkeit 1)
        let mut x = ((hcc1 >> 16) as u64) << 2;
        while x != 0 {
            let cap = base + x;
            let v = r32(cap);
            if v & 0xff == 1 {
                w32(cap, v | (1 << 24));
                wait(|| r32(cap) & (1 << 16) == 0, 1000);
                w32(cap + 4, r32(cap + 4) & 0xffff_e000);
            }
            let next = (v >> 8) & 0xff;
            x = if next == 0 { 0 } else { x + ((next as u64) << 2) };
        }
        // anhalten, zurücksetzen
        w32(op, r32(op) & !1);
        wait(|| r32(op + 4) & 1 != 0, 100);
        w32(op, 2);
        if !wait(|| r32(op) & 2 == 0 && r32(op + 4) & (1 << 11) == 0, 1000) {
            return Err("Rücksetzen hängt".into());
        }
        w32(op + 0x38, max_slots);
        let dcbaa = alloc_pages(1).ok_or("kein Speicher")?;
        let sp = ((hcs2 >> 27) & 0x1f) | (((hcs2 >> 21) & 0x1f) << 5);
        if sp > 0 {
            let arr = alloc_pages(1).ok_or("kein Speicher")?;
            for i in 0..sp.min(512) as u64 {
                w64(arr + i * 8, alloc_pages(1).ok_or("kein Speicher")?);
            }
            w64(dcbaa, arr);
        }
        w64(op + 0x30, dcbaa);
        let cmd = Ring::new().ok_or("kein Speicher")?;
        w64(op + 0x18, cmd.base | 1);
        // Ereignisring mit einem Segment
        let ev = alloc_pages(1).ok_or("kein Speicher")?;
        let erst = alloc_pages(1).ok_or("kein Speicher")?;
        w64(erst, ev);
        w32(erst + 8, RING as u32);
        let ir = rt + 0x20;
        w32(ir + 8, 1);
        w64(ir + 0x18, ev);
        w64(ir + 0x10, erst);
        w32(ir, r32(ir) & !2); // keine Interrupts
        w32(op, 1); // starten
        if !wait(|| r32(op + 4) & 1 == 0, 1000) {
            return Err("startet nicht".into());
        }
        crate::kprintln!("[xhci] Controller {:04x}:{:04x}: {} Port(s), {} Slots, Kontext {} Byte", d.vendor, d.device, ports, max_slots, ctx);
        let index = NEXT_HC.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let mut hc = Hc { index, ports, op, rt, db, ctx, dcbaa, cmd, ev_base: ev, ev_idx: 0, ev_cycle: 1, rings: Vec::new(), pending: VecDeque::new(), kbds: Vec::new(), keys: VecDeque::new() };
        // Anschlüsse mit Strom versorgen (falls der Controller das verlangt, HCCPARAMS1.PPC)
        if hcc1 & 8 != 0 {
            for p in 1..=ports {
                let pr = op + 0x400 + (p as u64 - 1) * 0x10;
                let sc = r32(pr);
                if sc & (1 << 9) == 0 {
                    w32(pr, (sc & 0x0e00_c3e0 & !(1 << 1)) | (1 << 9));
                }
            }
            crate::arch::sleep_us(20_000);
        }
        // Geräte an den Anschlüssen anmelden (USB-3-Verbindungen brauchen etwas Zeit zum Aufbau)
        crate::arch::sleep_us(300_000);
        for p in 1..=ports {
            if let Some(speed) = enable_port(&hc, p) {
                match attach(&mut hc, Lage::wurzel(p), speed) {
                    Ok(w) => crate::kprintln!("[xhci] Port {p}: {w}"),
                    Err(e) => crate::kprintln!("[xhci] Port {p}: {e}"),
                }
            }
        }
        HC.lock().push(hc);
        Ok(())
    }
}
