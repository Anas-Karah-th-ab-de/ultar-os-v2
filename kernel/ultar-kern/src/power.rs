//! Neustart und Ausschalten ohne die Laufzeitdienste der Firmware (die in manchen VMs nach dem Start
//! des Kerns abstürzen): ACPI-Reset-Register, PCI-Reset (0xCF9), Tastaturcontroller, ACPI S5.

use crate::arch::{inb, outb};
use core::sync::atomic::{AtomicU64, Ordering};

static RSDP: AtomicU64 = AtomicU64::new(0);

pub fn init(rsdp: u64) {
    RSDP.store(rsdp, Ordering::Relaxed);
}

unsafe fn outw(port: u16, v: u16) {
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags));
}

unsafe fn r8(a: u64) -> u8 {
    core::ptr::read_volatile(a as *const u8)
}
unsafe fn r32(a: u64) -> u32 {
    core::ptr::read_unaligned(a as *const u32)
}
unsafe fn r64(a: u64) -> u64 {
    core::ptr::read_unaligned(a as *const u64)
}

fn fadt() -> Option<u64> {
    unsafe { crate::apic::find_table(RSDP.load(Ordering::Relaxed), b"FACP") }
}

fn warte(ms: u64) {
    let t = crate::arch::uptime_ms();
    while crate::arch::uptime_ms().wrapping_sub(t) < ms {
        core::hint::spin_loop();
    }
}

/// Neustart: erst ACPI, dann PCI-Reset, Tastaturcontroller, zuletzt Dreifachfehler
pub fn neustart() -> ! {
    unsafe {
        core::arch::asm!("cli");
        if let Some(f) = fadt() {
            let len = r32(f + 4);
            let flags = if len > 116 { r32(f + 112) } else { 0 };
            if len >= 129 && flags & (1 << 10) != 0 {
                let space = r8(f + 116);
                let addr = r64(f + 120);
                let val = r8(f + 128);
                match space {
                    1 => outb(addr as u16, val),
                    0 => core::ptr::write_volatile(addr as *mut u8, val),
                    _ => {}
                }
                warte(200);
            }
        }
        outb(0xcf9, 0x02);
        warte(5);
        outb(0xcf9, 0x06);
        warte(200);
        for _ in 0..100_000 {
            if inb(0x64) & 2 == 0 {
                break;
            }
        }
        outb(0x64, 0xfe);
        warte(200);
        // Dreifachfehler: leere Unterbrechungstabelle + Ausnahme
        let idt: [u64; 2] = [0, 0];
        core::arch::asm!("lidt [{}]", "int3", in(reg) idt.as_ptr(), options(nostack));
    }
    crate::arch::halt_forever()
}

/// SLP_TYPa/b für S5 aus der DSDT (Paket \_S5_)
unsafe fn s5_typ(dsdt: u64) -> Option<(u16, u16)> {
    let len = r32(dsdt + 4) as u64;
    let mut p = dsdt + 36;
    while p + 4 < dsdt + len {
        if r8(p) == b'_' && r8(p + 1) == b'S' && r8(p + 2) == b'5' && r8(p + 3) == b'_' {
            let mut q = p + 4;
            if r8(q) != 0x12 {
                p += 1;
                continue; // kein Paket
            }
            q += 1;
            let pkglen_bytes = (r8(q) >> 6) as u64;
            q += 1 + pkglen_bytes;
            q += 1; // Anzahl Elemente
            let mut wert = |q: &mut u64| -> u16 {
                let b = r8(*q);
                match b {
                    0x0a => {
                        let v = r8(*q + 1) as u16;
                        *q += 2;
                        v
                    }
                    0x00 => {
                        *q += 1;
                        0
                    }
                    0x01 => {
                        *q += 1;
                        1
                    }
                    0xff => {
                        *q += 1;
                        0xff
                    }
                    v => {
                        *q += 1;
                        v as u16
                    }
                }
            };
            let a = wert(&mut q);
            let b = wert(&mut q);
            return Some((a & 7, b & 7));
        }
        p += 1;
    }
    None
}

/// Ausschalten über ACPI S5; scheitert das, bleibt der Kern stehen
pub fn aus() -> ! {
    unsafe {
        core::arch::asm!("cli");
        if let Some(f) = fadt() {
            let len = r32(f + 4);
            let pm1a = r32(f + 64) as u16;
            let pm1b = r32(f + 68) as u16;
            let dsdt = if len >= 148 && r64(f + 140) != 0 { r64(f + 140) } else { r32(f + 40) as u64 };
            if pm1a != 0 && dsdt != 0 {
                let (a, b) = s5_typ(dsdt).unwrap_or((0, 0));
                outw(pm1a, (a << 10) | (1 << 13));
                if pm1b != 0 {
                    outw(pm1b, (b << 10) | (1 << 13));
                }
                warte(500);
            }
        }
        // QEMU-/Bochs-Sonderanschlüsse
        outw(0x604, 0x2000);
        outw(0xb004, 0x2000);
        outw(0x4004, 0x3400);
    }
    crate::arch::halt_forever()
}
