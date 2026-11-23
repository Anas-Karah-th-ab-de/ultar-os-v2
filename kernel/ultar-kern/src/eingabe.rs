//! Geräte für Oberflächenprogramme (wie Linux): Bildspeicher `/dev/fb0` (fbdev) und Eingaben
//! `/dev/input/event0` (Maus, absolute Achsen in Bildpunkten) und `/dev/input/event1` (Tastatur, Linux-Tastencodes).
//!
//! Damit läuft die Ultar-Oberfläche (ultar-shell, statisches Linux-Programm) unverändert auf dem Ultar-Kern.
//! Solange ein Programm den Bildspeicher offen hat, schreibt die Konsole nur seriell; solange eine Eingabe offen ist,
//! gehen Tasten roh an das Programm statt an die Konsole.

use crate::arch::inb;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use spin::Mutex;

pub const MAUS: u8 = 0;
pub const TASTATUR: u8 = 1;

/// Zahl offener Eingabe-Deskriptoren (> 0: Tasten gehen roh an Programme)
static ROH: AtomicUsize = AtomicUsize::new(0);
/// Zahl offener Bildspeicher-Deskriptoren
static FB_OFFEN: AtomicUsize = AtomicUsize::new(0);
static QUEUES: Mutex<[VecDeque<[u8; 24]>; 2]> = Mutex::new([VecDeque::new(), VecDeque::new()]);
static E0: AtomicBool = AtomicBool::new(false);
/// letzter gemeldeter Mauszustand
static MX: AtomicI32 = AtomicI32::new(-1);
static MY: AtomicI32 = AtomicI32::new(-1);
static MT: AtomicI32 = AtomicI32::new(0);
static MRAD: AtomicI32 = AtomicI32::new(0);
static MEREIGNIS: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Hat gerade ein Programm den Bildspeicher offen?
pub fn fb_offen() -> bool {
    FB_OFFEN.load(Ordering::Relaxed) > 0
}

pub fn roh() -> bool {
    ROH.load(Ordering::Relaxed) > 0
}

/// Besitz eines Eingabe-Deskriptors (beim Schließen wieder Konsole)
/// .1: liest (Tasten gehen roh an Programme); false = nur schreibend (Ereignisse einspeisen)
pub struct EingabeFd(pub u8, bool);

impl EingabeFd {
    /// nur zum Einspeisen geöffnet (O_WRONLY): Konsole bleibt, wie sie ist
    pub fn schreibend(art: u8) -> EingabeFd {
        EingabeFd(art, false)
    }
    pub fn neu(art: u8) -> EingabeFd {
        if ROH.fetch_add(1, Ordering::SeqCst) == 0 {
            QUEUES.lock().iter_mut().for_each(|q| q.clear());
            crate::input::clear();
            MEREIGNIS.store(usize::MAX, Ordering::Relaxed);
        }
        if art == MAUS {
            if let Some(fb) = crate::console::framebuffer() {
                crate::drivers::ps2maus::init(fb.width as usize, fb.height as usize);
            }
        }
        EingabeFd(art, true)
    }
}

impl Drop for EingabeFd {
    fn drop(&mut self) {
        if self.1 {
            ROH.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Besitz des Bildspeichers
/// .0: Besitzer (Konsole schreibt nur noch seriell); false = nur lesend (Bildschirmfoto)
pub struct FbFd(bool);

impl FbFd {
    pub fn neu() -> Option<FbFd> {
        crate::console::framebuffer()?;
        if FB_OFFEN.fetch_add(1, Ordering::SeqCst) == 0 {
            crate::console::grafik(true);
        }
        Some(FbFd(true))
    }
    /// nur lesend (O_RDONLY): Bildschirm bleibt, wie er ist
    pub fn lesend() -> Option<FbFd> {
        crate::console::framebuffer()?;
        Some(FbFd(false))
    }
}

impl Drop for FbFd {
    fn drop(&mut self) {
        if self.0 && FB_OFFEN.fetch_sub(1, Ordering::SeqCst) == 1 {
            crate::console::grafik(false);
        }
    }
}

fn ereignis(art: u8, typ: u16, code: u16, wert: i32) {
    let us = crate::arch::uptime_ms() * 1000;
    let mut e = [0u8; 24];
    e[0..8].copy_from_slice(&(us / 1_000_000).to_le_bytes());
    e[8..16].copy_from_slice(&(us % 1_000_000).to_le_bytes());
    e[16..18].copy_from_slice(&typ.to_le_bytes());
    e[18..20].copy_from_slice(&code.to_le_bytes());
    e[20..24].copy_from_slice(&wert.to_le_bytes());
    let mut q = QUEUES.lock();
    let q = &mut q[art as usize];
    if q.len() < 4096 {
        q.push_back(e);
    }
}

fn taste(code: u16, gedrueckt: bool) {
    ereignis(TASTATUR, 1, code, gedrueckt as i32);
    ereignis(TASTATUR, 0, 0, 0);
}

/// Zeichen einer USB-Tastatur (US-Belegung) → Linux-Tastencode (+ Umschalt)
fn zeichen_code(c: char) -> Option<(u16, bool)> {
    const REIHEN: [(u16, &str, &str); 4] = [(2, "1234567890-=", "!@#$%^&*()_+"), (16, "qwertyuiop[]", "QWERTYUIOP{}"), (30, "asdfghjkl;'`", "ASDFGHJKL:\"~"), (44, "zxcvbnm,./", "ZXCVBNM<>?")];
    match c {
        '\n' | '\r' => return Some((28, false)),
        ' ' => return Some((57, false)),
        '\u{8}' | '\u{7f}' => return Some((14, false)),
        '\t' => return Some((15, false)),
        '\u{1b}' => return Some((1, false)),
        '\\' => return Some((43, false)),
        '|' => return Some((43, true)),
        crate::drivers::ps2kbd::HOCH => return Some((103, false)),
        crate::drivers::ps2kbd::RUNTER => return Some((108, false)),
        crate::drivers::ps2kbd::LINKS => return Some((105, false)),
        crate::drivers::ps2kbd::RECHTS => return Some((106, false)),
        _ => {}
    }
    for (start, lo, hi) in REIHEN {
        if let Some(i) = lo.chars().position(|x| x == c) {
            return Some((start + i as u16, false));
        }
        if let Some(i) = hi.chars().position(|x| x == c) {
            return Some((start + i as u16, true));
        }
    }
    None
}

/// Hardware abfragen und Ereignisse einreihen (aus read/poll der Eingabe-Deskriptoren)
pub fn pumpen() {
    if !roh() {
        return;
    }
    // USB-Tastaturen liefern Zeichen: als Drücken/Loslassen melden
    for _ in 0..16 {
        let Some(c) = crate::drivers::gen_xhci::poll_key() else { break };
        if let Some((code, shift)) = zeichen_code(c) {
            if shift {
                taste(42, true);
            }
            taste(code, true);
            taste(code, false);
            if shift {
                taste(42, false);
            }
        }
    }
    // i8042: Tastatur roh (Satz 1 = Linux-Tastencode), Bytes der Maus an den Maustreiber
    for _ in 0..64 {
        let st = unsafe { inb(0x64) };
        if st & 1 == 0 {
            break;
        }
        let b = unsafe { inb(0x60) };
        if st & 0x20 != 0 {
            crate::drivers::ps2maus::byte(b);
            continue;
        }
        if b == 0xe0 {
            E0.store(true, Ordering::Relaxed);
            continue;
        }
        let e0 = E0.swap(false, Ordering::Relaxed);
        let los = b & 0x80 != 0;
        let sc = (b & 0x7f) as u16;
        let code = if e0 {
            match sc {
                0x1c => 96,  // KP Enter
                0x1d => 97,  // Strg rechts
                0x38 => 100, // AltGr
                0x47 => 102, // Pos1
                0x48 => 103, // hoch
                0x49 => 104, // Bild hoch
                0x4b => 105, // links
                0x4d => 106, // rechts
                0x4f => 107, // Ende
                0x50 => 108, // runter
                0x51 => 109, // Bild runter
                0x52 => 110, // Einfg
                0x53 => 111, // Entf
                0x5b => 125, // Windows-Taste
                0x35 => 98,  // KP /
                _ => continue,
            }
        } else {
            sc
        };
        taste(code, !los);
    }
    // Maus
    crate::drivers::ps2maus::abfragen();
    let (x, y, _, n) = crate::drivers::ps2maus::zustand();
    let rad = crate::drivers::ps2maus::rad();
    if n != MEREIGNIS.swap(n, Ordering::Relaxed) || rad != 0 {
        let t = crate::drivers::ps2maus::tasten() as i32;
        if x != MX.swap(x, Ordering::Relaxed) {
            ereignis(MAUS, 3, 0, x);
        }
        if y != MY.swap(y, Ordering::Relaxed) {
            ereignis(MAUS, 3, 1, y);
        }
        let alt = MT.swap(t, Ordering::Relaxed);
        for (bit, code) in [(1, 0x110u16), (2, 0x111), (4, 0x112)] {
            if (t ^ alt) & bit != 0 {
                ereignis(MAUS, 1, code, (t & bit != 0) as i32);
            }
        }
        if rad != 0 {
            ereignis(MAUS, 2, 8, -rad); // REL_WHEEL: hoch = positiv
        }
        ereignis(MAUS, 0, 0, 0);
    }
}

/// Ereignisse lesen (24 Byte je Ereignis); None = nichts da
pub fn lesen(art: u8, max: usize) -> Option<alloc::vec::Vec<u8>> {
    pumpen();
    let mut q = QUEUES.lock();
    let q = &mut q[art as usize];
    if q.is_empty() {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    while out.len() + 24 <= max {
        let Some(e) = q.pop_front() else { break };
        out.extend_from_slice(&e);
    }
    Some(out)
}

pub fn bereit(art: u8) -> bool {
    if art >= crate::input::FIRST {
        return crate::input::ready(art);
    }
    pumpen();
    !QUEUES.lock()[art as usize].is_empty()
}

/// ioctl auf einem Eingabe-Deskriptor; None = unbekannt
pub fn ioctl(art: u8, req: u64) -> Option<alloc::vec::Vec<u8>> {
    if art >= crate::input::FIRST {
        return crate::input::ioctl(art, req);
    }
    let fb = crate::console::framebuffer()?;
    // EVIOCGABS(ABS_X/ABS_Y): input_absinfo {value, min, max, fuzz, flat, resolution}
    if art == MAUS && (req == 0x8018_4540 || req == 0x8018_4541) {
        let max = if req & 1 == 0 { fb.width as i32 - 1 } else { fb.height as i32 - 1 };
        let v = if req & 1 == 0 { MX.load(Ordering::Relaxed).max(0) } else { MY.load(Ordering::Relaxed).max(0) };
        let mut b = alloc::vec![0u8; 24];
        for (i, x) in [v, 0, max, 0, 0, 0].iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
        return Some(b);
    }
    // EVIOCGNAME(len)
    if req & 0xffff == 0x4506 && req >> 30 == 2 {
        let name: &[u8] = if art == MAUS { b"Ultar Maus\0" } else { b"Ultar Tastatur\0" };
        return Some(name.to_vec());
    }
    crate::input::ioctl(art, req)
}

/// ioctl auf dem Bildspeicher (FBIOGET_VSCREENINFO/FSCREENINFO); None = unbekannt
pub fn fb_ioctl(req: u64) -> Option<alloc::vec::Vec<u8>> {
    let fb = crate::console::framebuffer()?;
    match req {
        0x4600 => {
            let mut v = [0u32; 40];
            v[0] = fb.width;
            v[1] = fb.height;
            v[2] = fb.width;
            v[3] = fb.height;
            v[6] = 32;
            // Farbkanäle {offset, länge, msb_rechts}: bgr = Speicher B,G,R,X → Rot bei Bit 16
            let (r, b) = if fb.bgr { (16, 0) } else { (0, 16) };
            v[8] = r;
            v[9] = 8;
            v[11] = 8;
            v[12] = 8;
            v[14] = b;
            v[15] = 8;
            v[17] = 24;
            v[18] = 0;
            v[22] = u32::MAX; // height in mm unbekannt
            v[23] = u32::MAX;
            Some(v.iter().flat_map(|x| x.to_le_bytes()).collect())
        }
        0x4602 => {
            let mut f = alloc::vec![0u8; 80];
            f[..9].copy_from_slice(b"ultar-fb\0");
            f[16..24].copy_from_slice(&fb.base.to_le_bytes());
            f[24..28].copy_from_slice(&(fb.stride * fb.height * 4).to_le_bytes());
            f[32..36].copy_from_slice(&2u32.to_le_bytes()); // FB_VISUAL_TRUECOLOR
            f[48..52].copy_from_slice(&(fb.stride * 4).to_le_bytes());
            Some(f)
        }
        // FBIOGET_VBLANK: struct fb_vblank, with FB_VBLANK_HAVE_COUNT and the count of the simulated blanks
        0x8020_4612 => {
            let mut v = alloc::vec![0u8; 32];
            v[0..4].copy_from_slice(&0x10u32.to_le_bytes());
            v[4..8].copy_from_slice(&(crate::shm::vblank_count() as u32).to_le_bytes());
            Some(v)
        }
        0x4601 | 0x4606 | 0x4611 => Some(alloc::vec::Vec::new()),
        _ => None,
    }
}

/// in den Bildspeicher schreiben (pwrite/write ab Versatz)
pub fn fb_schreiben(off: u64, daten: &[u8]) -> i64 {
    let Some(fb) = crate::console::framebuffer() else { return -5 };
    let groesse = (fb.stride as u64) * (fb.height as u64) * 4;
    if off >= groesse {
        return 0;
    }
    let n = (daten.len() as u64).min(groesse - off) as usize;
    unsafe { core::ptr::copy_nonoverlapping(daten.as_ptr(), (fb.base + off) as *mut u8, n) };
    n as i64
}

/// aus dem Bildspeicher lesen (pread ab Versatz) – Bildschirmfoto für den Entwickler-Zugang und den System-Assistenten
pub fn fb_lesen(off: u64, n: usize) -> alloc::vec::Vec<u8> {
    let Some(fb) = crate::console::framebuffer() else { return alloc::vec::Vec::new() };
    let groesse = (fb.stride as u64) * (fb.height as u64) * 4;
    if off >= groesse {
        return alloc::vec::Vec::new();
    }
    let n = (n as u64).min(groesse - off) as usize;
    let mut v = alloc::vec![0u8; n];
    unsafe { core::ptr::copy_nonoverlapping((fb.base + off) as *const u8, v.as_mut_ptr(), n) };
    v
}

/// Ereignisse einspeisen (write auf /dev/input/eventN wie unter Linux): Fernsteuerung über den Entwickler-Zugang.
/// Je 24 Byte ein `struct input_event`; die Zeit setzt der Kern.
pub fn einspeisen(art: u8, daten: &[u8]) -> usize {
    if art >= crate::input::FIRST {
        crate::input::inject(art, daten);
        return daten.len() / 24 * 24;
    }
    let mut n = 0;
    for e in daten.chunks_exact(24) {
        let typ = u16::from_le_bytes([e[16], e[17]]);
        let code = u16::from_le_bytes([e[18], e[19]]);
        let wert = i32::from_le_bytes([e[20], e[21], e[22], e[23]]);
        if art == MAUS && typ == 3 {
            if code == 0 { MX.store(wert, Ordering::Relaxed) } else if code == 1 { MY.store(wert, Ordering::Relaxed) }
        }
        ereignis(art, typ, code, wert);
        n += 24;
    }
    crate::socket::bump();
    n
}
