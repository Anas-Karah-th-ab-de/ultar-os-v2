//! PS/2-Maus (i8042, Hilfsanschluss, Abfragebetrieb): VMs (VMware, QEMU) und viele Laptops (Touchpad im PS/2-Modus).
//! Die Tastaturabfrage (`ps2kbd::poll`) reicht Bytes des Hilfsanschlusses hierher weiter.

use crate::arch::{inb, outb};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};

static AKTIV: AtomicBool = AtomicBool::new(false);
static X: AtomicI32 = AtomicI32::new(0);
static Y: AtomicI32 = AtomicI32::new(0);
static MAX_X: AtomicI32 = AtomicI32::new(1023);
static MAX_Y: AtomicI32 = AtomicI32::new(767);
static TASTEN: AtomicU8 = AtomicU8::new(0);
/// Mausrad (vmmouse), seit der letzten Abfrage
static RAD: AtomicI32 = AtomicI32::new(0);
/// Zähler für Ereignisse (Bewegung/Klick), damit die Oberfläche weiß, wann sie neu zeichnen muss
static EREIGNIS: AtomicUsize = AtomicUsize::new(0);

/// absolute Maus (vmmouse) aktiv: Zeiger folgt genau dem Mauszeiger des Gastgebers
static ABSOLUT: AtomicBool = AtomicBool::new(false);

static mut PAKET: [u8; 3] = [0; 3];

const VMW_MAGIC: u32 = 0x564D_5868;
const VMW_PORT: u16 = 0x5658;

/// VMware-Hintertür: (eax, ebx, ecx, edx)
fn hintertuer(cmd: u32, arg: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u64, u32, u32);
    unsafe {
        core::arch::asm!(
            "xchg {t}, rbx",
            "in eax, dx",
            "xchg {t}, rbx",
            t = inout(reg) arg as u64 => b,
            inout("eax") VMW_MAGIC => a,
            inout("ecx") cmd => c,
            inout("edx") VMW_PORT as u32 => d,
            // die Hintertür (QEMU vmport, VMware) schreibt auch ESI/EDI zurück
            out("rsi") _,
            out("rdi") _,
            options(nostack),
        );
    }
    (a, b as u32, c, d)
}

fn ist_vmware() -> bool {
    // nur auf x86-Gästen mit Hypervisor-Bit sinnvoll – sonst löst `in` auf echter Hardware nichts Schlimmes aus,
    // liefert aber keine Kennung
    let hv = unsafe { core::arch::x86_64::__cpuid(1).ecx } & (1 << 31) != 0;
    hv && hintertuer(10, !VMW_MAGIC).1 == VMW_MAGIC
}

fn vmmouse_an() -> bool {
    if !ist_vmware() {
        return false;
    }
    hintertuer(41, 0x4541_4552); // ENABLE
    let st = hintertuer(40, 0).0;
    if st == 0xffff_0000 || st & 0xffff == 0 {
        return false;
    }
    let id = hintertuer(39, 1).0;
    if id != 0x3442_554a {
        return false;
    }
    hintertuer(41, 0x5342_4152); // REQUEST_ABSOLUTE
    true
}

/// absolute Mausdaten abholen (vmmouse); ruft die Oberfläche regelmäßig auf
pub fn abfragen() {
    if !ABSOLUT.load(Ordering::Relaxed) {
        return;
    }
    loop {
        let st = hintertuer(40, 0).0;
        if st == 0xffff_0000 {
            // Gerät neu einschalten
            ABSOLUT.store(vmmouse_an(), Ordering::Relaxed);
            return;
        }
        if st & 0xffff < 4 {
            return;
        }
        let (fl, x, y, z) = hintertuer(39, 4);
        RAD.fetch_add(z as i8 as i32, Ordering::Relaxed);
        let mx = MAX_X.load(Ordering::Relaxed) + 1;
        let my = MAX_Y.load(Ordering::Relaxed) + 1;
        X.store(
            ((x as i64 & 0xffff) * mx as i64 / 0x10000) as i32,
            Ordering::Relaxed,
        );
        Y.store(
            ((y as i64 & 0xffff) * my as i64 / 0x10000) as i32,
            Ordering::Relaxed,
        );
        let mut t = 0u8;
        if fl & 0x20 != 0 {
            t |= 1;
        }
        if fl & 0x10 != 0 {
            t |= 2;
        }
        if fl & 0x08 != 0 {
            t |= 4;
        }
        TASTEN.store(t, Ordering::Relaxed);
        EREIGNIS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn absolut() -> bool {
    ABSOLUT.load(Ordering::Relaxed)
}
static mut POS: usize = 0;

fn warte_schreiben() -> bool {
    for _ in 0..200_000 {
        if unsafe { inb(0x64) } & 2 == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn warte_lesen() -> Option<u8> {
    for _ in 0..400_000 {
        if unsafe { inb(0x64) } & 1 != 0 {
            return Some(unsafe { inb(0x60) });
        }
        core::hint::spin_loop();
    }
    None
}

fn befehl(cmd: u8) -> bool {
    unsafe {
        warte_schreiben();
        outb(0x64, 0xd4);
        warte_schreiben();
        outb(0x60, cmd);
    }
    warte_lesen() == Some(0xfa)
}

/// Maus einschalten (einmalig); Bildschirmgröße für die Begrenzung des Zeigers
pub fn init(breite: usize, hoehe: usize) -> bool {
    MAX_X.store(breite as i32 - 1, Ordering::Relaxed);
    MAX_Y.store(hoehe as i32 - 1, Ordering::Relaxed);
    if AKTIV.load(Ordering::Relaxed) {
        return true;
    }
    X.store(breite as i32 / 2, Ordering::Relaxed);
    Y.store(hoehe as i32 / 2, Ordering::Relaxed);
    unsafe {
        // kein Tastaturcontroller (Port liest 0xFF, z. B. Beelink GTi15 ohne i8042): Maus nur über USB
        if inb(0x64) == 0xff {
            return false;
        }
        // liegengebliebene Bytes verwerfen (begrenzt, falls der Controller hängt)
        for _ in 0..64 {
            if inb(0x64) & 1 == 0 {
                break;
            }
            inb(0x60);
        }
        warte_schreiben();
        outb(0x64, 0xa8); // Hilfsanschluss an
        warte_schreiben();
        outb(0x64, 0x20); // Steuerbyte lesen
        let Some(mut c) = warte_lesen() else {
            return false;
        };
        c &= !0x20; // Maus-Takt nicht sperren
        c &= !0x02; // kein IRQ12 (Abfragebetrieb)
        warte_schreiben();
        outb(0x64, 0x60);
        warte_schreiben();
        outb(0x60, c);
    }
    // Standardwerte, 200 Meldungen/s, 8 Zählschritte/mm, Daten senden
    let ok =
        befehl(0xf6) && befehl(0xf3) && befehl(200) && befehl(0xe8) && befehl(3) && befehl(0xf4);
    let abs = vmmouse_an();
    ABSOLUT.store(abs, Ordering::Relaxed);
    if abs {
        crate::kprintln!("[maus] absolute Maus (VMware-Schnittstelle) aktiv");
    }
    unsafe {
        POS = 0;
    }
    AKTIV.store(ok, Ordering::Relaxed);
    if ok {
        crate::kprintln!("[maus] PS/2-Maus bereit");
    }
    ok
}

pub fn aktiv() -> bool {
    AKTIV.load(Ordering::Relaxed)
}

/// Ein Byte vom Hilfsanschluss (aus der Tastaturabfrage)
pub fn byte(b: u8) {
    if !AKTIV.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let p = &mut *core::ptr::addr_of_mut!(PAKET);
        let pos = &mut *core::ptr::addr_of_mut!(POS);
        if *pos == 0 && b & 0x08 == 0 {
            return; // nicht synchron: erstes Byte hat immer Bit 3
        }
        p[*pos] = b;
        *pos += 1;
        if *pos < 3 {
            return;
        }
        *pos = 0;
        let f = p[0];
        if f & 0xc0 != 0 {
            return; // Überlauf
        }
        if ABSOLUT.load(Ordering::Relaxed) {
            return; // Position kommt aus der vmmouse-Schnittstelle
        }
        // Beschleunigung: langsame Bewegungen fein, schnelle weit
        let beschl = |d: i32| {
            if d.abs() > 12 {
                d * 3
            } else if d.abs() > 4 {
                d * 2
            } else {
                d
            }
        };
        let dx = beschl(p[1] as i32 - (((f as i32) << 4) & 0x100));
        let dy = beschl(p[2] as i32 - (((f as i32) << 3) & 0x100));
        let x = (X.load(Ordering::Relaxed) + dx).clamp(0, MAX_X.load(Ordering::Relaxed));
        let y = (Y.load(Ordering::Relaxed) - dy).clamp(0, MAX_Y.load(Ordering::Relaxed));
        X.store(x, Ordering::Relaxed);
        Y.store(y, Ordering::Relaxed);
        TASTEN.store(f & 7, Ordering::Relaxed);
        EREIGNIS.fetch_add(1, Ordering::Relaxed);
    }
}

/// (x, y, linke Taste gedrückt, Ereigniszähler)
pub fn zustand() -> (i32, i32, bool, usize) {
    (
        X.load(Ordering::Relaxed),
        Y.load(Ordering::Relaxed),
        TASTEN.load(Ordering::Relaxed) & 1 != 0,
        EREIGNIS.load(Ordering::Relaxed),
    )
}

/// Mausrad-Schritte seit dem letzten Aufruf (positiv = nach unten)
pub fn rad() -> i32 {
    RAD.swap(0, Ordering::Relaxed)
}

/// gedrückte Tasten (Bit 0 links, 1 rechts, 2 Mitte)
pub fn tasten() -> u8 {
    TASTEN.load(Ordering::Relaxed)
}
