//! Neustart und Ausschalten. Zuerst ACPI (FADT + \_S5 der DSDT, Reset-Register), dann PCI-Reset (0xCF9) und
//! Tastaturcontroller, die Laufzeitdienste der Firmware (ResetSystem) nur als Rückfall.
//!
//! Alles läuft auf der Seitentabelle des Kerns: im Adressraum eines Prozesses ersetzt das Benutzerfenster
//! [4 MiB, 256 MiB) die 1:1-Abbildung – dort liegende Firmware-Laufzeitdienste (VMware: 0xe08_xxxx) oder
//! ACPI-Tabellen wären sonst Benutzerseiten (Absturz „ungültiger Befehl“ in ResetSystem).
//! Eine Ausnahme während des Firmware-Aufrufs endet nicht im Fehlerbild, sondern in [`firmware_fehler`].

use crate::arch::{inb, outb, outl};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

static RSDP: AtomicU64 = AtomicU64::new(0);
/// Datei `ultar-kern/aus-firmware` auf der Startpartition: Firmware zuerst (Diagnose, Rechner ohne ACPI-S5)
static FIRMWARE_ZUERST: AtomicBool = AtomicBool::new(false);
/// 0 = kein Firmware-Aufruf, 1 = Ausschalten, 2 = Neustart läuft gerade in der Firmware
static IN_FIRMWARE: AtomicU8 = AtomicU8::new(0);

pub fn init(rsdp: u64) {
    RSDP.store(rsdp, Ordering::Relaxed);
}

/// nach dem Einhängen der Startpartition
pub fn optionen() {
    if crate::fs::fat_has("ultar-kern/aus-firmware") {
        FIRMWARE_ZUERST.store(true, Ordering::Relaxed);
    }
}

unsafe fn outw(port: u16, v: u16) {
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags));
}
unsafe fn inw(port: u16) -> u16 {
    let v: u16;
    core::arch::asm!("in ax, dx", in("dx") port, out("ax") v, options(nomem, nostack, preserves_flags));
    v
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

/// Unterbrechungen aus, Seitentabelle des Kerns (volle 1:1-Abbildung) laden
unsafe fn vorbereiten() {
    core::arch::asm!("cli");
    // the data key does not survive the power-off or restart in memory
    crate::crypt::wipe_keys();
    let k = crate::paging::kernel_pml4();
    if k != 0 {
        core::arch::asm!("mov cr3, {}", in(reg) k);
    }
}

fn warte(ms: u64) {
    crate::arch::sleep_us(ms * 1000);
}

/// FADT mit Länge
unsafe fn fadt() -> Option<(u64, u32)> {
    let f = crate::apic::find_table(RSDP.load(Ordering::Relaxed), b"FACP")?;
    Some((f, r32(f + 4)))
}

/// Adresse aus einer Generic Address Structure (Port-Raum) oder der alten 32-Bit-Angabe
unsafe fn port(f: u64, len: u32, alt: u64, gas: u64) -> u16 {
    let a = r32(f + alt) as u16;
    if a != 0 {
        return a;
    }
    if len as u64 >= gas + 12 && r8(f + gas) == 1 {
        return r64(f + gas + 4) as u16;
    }
    0
}

/// Ausnahme während ResetSystem: kein Fehlerbild, sondern eine klare Meldung (bzw. Neustart erzwingen)
pub fn firmware_fehler() -> bool {
    match IN_FIRMWARE.swap(0, Ordering::Relaxed) {
        0 => false,
        2 => unsafe { dreifachfehler() },
        _ => {
            crate::kprintln!("\n{}", crate::sprache::t("aus_von_hand"));
            crate::arch::halt_forever()
        }
    }
}

/// UEFI ResetSystem (0 = kalt neu starten, 2 = aus), korrekt aufgerufen: Kern-Seitentabelle, Unterbrechungen aus
unsafe fn firmware(kind: u32) {
    let rt = crate::efi::runtime();
    if rt.is_null() {
        return;
    }
    IN_FIRMWARE.store(if kind == 2 { 1 } else { 2 }, Ordering::Relaxed);
    crate::efi::reset(rt, kind);
    // die Firmware kehrte zurück (nicht unterstützt): weiter mit dem nächsten Weg
    IN_FIRMWARE.store(0, Ordering::Relaxed);
}

unsafe fn dreifachfehler() -> ! {
    let idt: [u64; 2] = [0, 0];
    core::arch::asm!("lidt [{}]", "int3", in(reg) idt.as_ptr(), options(nostack));
    crate::arch::halt_forever()
}

/// Reset-Register der FADT (ACPI 2+, Flags-Bit 10 RESET_REG_SUP)
unsafe fn acpi_reset() {
    let Some((f, len)) = fadt() else { return };
    let flags = if len >= 116 { r32(f + 112) } else { 0 };
    if len < 129 || flags & (1 << 10) == 0 {
        return;
    }
    let space = r8(f + 116);
    let addr = r64(f + 120);
    let val = r8(f + 128);
    match space {
        1 => outb(addr as u16, val),
        0 if addr != 0 => core::ptr::write_volatile(addr as *mut u8, val),
        2 => {
            // PCI-Konfigurationsraum, Bus 0: Gerät (Bits 32–47), Funktion (16–31), Versatz (0–15)
            let dev = (addr >> 32) & 0x1f;
            let func = (addr >> 16) & 7;
            let off = addr & 0xff;
            outl(0xcf8, 0x8000_0000 | (dev << 11) as u32 | (func << 8) as u32 | (off & 0xfc) as u32);
            outb(0xcfc + (off & 3) as u16, val);
        }
        _ => return,
    }
    warte(200);
}

/// Neustart: ACPI-Reset-Register, PCI-Reset (0xCF9), Tastaturcontroller, Firmware, zuletzt Dreifachfehler
pub fn neustart() -> ! {
    unsafe {
        vorbereiten();
        if FIRMWARE_ZUERST.load(Ordering::Relaxed) {
            firmware(0);
        }
        acpi_reset();
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
        firmware(0);
        dreifachfehler()
    }
}

/// Ganzzahl eines AML-Paketelements; `None` bei unbekannter Kodierung
unsafe fn aml_zahl(q: &mut u64) -> Option<u16> {
    let b = r8(*q);
    let (v, n) = match b {
        0x00 => (0, 1),                                            // ZeroOp
        0x01 => (1, 1),                                            // OneOp
        0xff => (0xff, 1),                                         // OnesOp
        0x0a => (r8(*q + 1) as u16, 2),                            // BytePrefix
        0x0b => (r8(*q + 1) as u16 | (r8(*q + 2) as u16) << 8, 3), // WordPrefix
        0x0c => (r32(*q + 1) as u16, 5),                           // DWordPrefix
        _ => return None,
    };
    *q += n;
    Some(v)
}

/// SLP_TYPa/b für S5 aus der DSDT (`Name (_S5, Package () {a, b, …})`)
unsafe fn s5_typ(dsdt: u64) -> Option<(u16, u16)> {
    let len = r32(dsdt + 4) as u64;
    if &[r8(dsdt), r8(dsdt + 1), r8(dsdt + 2), r8(dsdt + 3)] != b"DSDT" || !(36..(64 << 20)).contains(&len) {
        return None;
    }
    let ende = dsdt + len;
    let mut p = dsdt + 36;
    while p + 12 < ende {
        // NameOp (0x08), optional „\“, dann _S5_ und PackageOp (0x12)
        let name_davor = r8(p - 1) == 0x08 || (r8(p - 1) == b'\\' && r8(p - 2) == 0x08);
        if name_davor && r8(p) == b'_' && r8(p + 1) == b'S' && r8(p + 2) == b'5' && r8(p + 3) == b'_' && r8(p + 4) == 0x12 {
            let mut q = p + 5;
            q += 1 + (r8(q) >> 6) as u64; // PkgLength
            let anzahl = r8(q);
            q += 1;
            if anzahl >= 1 {
                let a = aml_zahl(&mut q)?;
                let b = if anzahl >= 2 { aml_zahl(&mut q).unwrap_or(0) } else { 0 };
                return Some((a & 7, b & 7));
            }
        }
        p += 1;
    }
    None
}

/// ACPI S5 (Soft-Off); kehrt zurück, wenn die Maschine danach noch läuft
unsafe fn acpi_aus() {
    let Some((f, len)) = fadt() else { return };
    let dsdt = if len >= 148 && r64(f + 140) != 0 { r64(f + 140) } else { r32(f + 40) as u64 };
    let (a, b) = if dsdt != 0 { s5_typ(dsdt).unwrap_or((0, 0)) } else { (0, 0) };
    let flags = if len >= 116 { r32(f + 112) } else { 0 };
    // hardware-reduziert (Flags-Bit 20): Sleep Control Register (Versatz 244)
    if flags & (1 << 20) != 0 && len >= 256 {
        let sc = r64(f + 248);
        let v = ((a as u8 & 7) << 2) | (1 << 5);
        match r8(f + 244) {
            1 => outb(sc as u16, v),
            0 if sc != 0 => core::ptr::write_volatile(sc as *mut u8, v),
            _ => {}
        }
        warte(500);
        return;
    }
    let pm1a = port(f, len, 64, 172);
    let pm1b = port(f, len, 68, 184);
    if pm1a == 0 {
        return;
    }
    // ACPI-Modus einschalten, falls die Firmware ihn nicht schon gesetzt hat (SCI_EN)
    let smi = r32(f + 48) as u16;
    let enable = r8(f + 52);
    if inw(pm1a) & 1 == 0 && smi != 0 && enable != 0 {
        outb(smi, enable);
        for _ in 0..300 {
            if inw(pm1a) & 1 != 0 {
                break;
            }
            warte(10);
        }
    }
    // Wake-Status löschen (PM1-Statusregister, Bit 15)
    let evt_a = port(f, len, 56, 148);
    if evt_a != 0 {
        outw(evt_a, 1 << 15);
    }
    let setzen = |p: u16, typ: u16| {
        let v = (inw(p) & !(0x3c00)) | (typ << 10);
        outw(p, v);
        outw(p, v | (1 << 13));
    };
    if pm1b != 0 {
        setzen(pm1b, b);
    }
    setzen(pm1a, a);
    warte(1000);
}

/// Ausschalten: ACPI S5, QEMU-/Bochs-/VirtualBox-Anschlüsse, Firmware; sonst Meldung und anhalten
pub fn aus() -> ! {
    unsafe {
        vorbereiten();
        if FIRMWARE_ZUERST.load(Ordering::Relaxed) {
            firmware(2);
        }
        acpi_aus();
        outw(0x604, 0x2000);
        outw(0xb004, 0x2000);
        outw(0x4004, 0x3400);
        warte(200);
        firmware(2);
    }
    crate::kprintln!("\n{}", crate::sprache::t("aus_von_hand"));
    crate::arch::halt_forever()
}
