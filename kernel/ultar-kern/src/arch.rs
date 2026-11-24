//! x86_64: Ein-/Ausgabe-Ports, eigene GDT/IDT, Ausnahmebehandlung, Zeitmessung (TSC, an PIT geeicht).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

#[inline]
pub unsafe fn outb(port: u16, v: u8) {
    asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags));
}
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags));
    v
}
#[inline]
pub unsafe fn outl(port: u16, v: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack, preserves_flags));
}
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let v: u32;
    asm!("in eax, dx", out("eax") v, in("dx") port, options(nomem, nostack, preserves_flags));
    v
}

pub fn halt_forever() -> ! {
    loop {
        unsafe { asm!("cli; hlt") };
    }
}

pub fn pause() {
    core::hint::spin_loop();
}

// ------------------------------------------------------------------ GDT

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct Tss {
    _r0: u32,
    rsp: [u64; 3],
    _r1: u64,
    ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    iomap: u16,
}

pub const MAXCPU: usize = 32;
static mut TSS: [Tss; MAXCPU] = [const { Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, iomap: 104 } }; MAXCPU];
static mut GDT: [u64; 6 + 2 * MAXCPU] = [0; 6 + 2 * MAXCPU];
/// eigener Stapel für Doppelfehler (IST 1) je Kern, damit auch ein übergelaufener Stapel noch gemeldet wird
static mut DF_STACK: [[u8; 8192]; MAXCPU] = [[0; 8192]; MAXCPU];

/// TSS-Deskriptor für Kern `cpu` eintragen
unsafe fn set_tss(cpu: usize) {
    let tss_base = core::ptr::addr_of!(TSS[cpu]) as u64;
    let mut ist = [0u64; 7];
    ist[0] = (core::ptr::addr_of!(DF_STACK[cpu]) as u64 + 8192) & !15;
    core::ptr::addr_of_mut!(TSS[cpu].ist).write_unaligned(ist);
    let lim = (core::mem::size_of::<Tss>() - 1) as u64;
    GDT[6 + 2 * cpu] = (lim & 0xffff) | ((tss_base & 0xffffff) << 16) | (0x89u64 << 40) | (((lim >> 16) & 0xf) << 48) | (((tss_base >> 24) & 0xff) << 56);
    GDT[7 + 2 * cpu] = tss_base >> 32;
}

/// TSS des Kerns `cpu` laden (Selektor 0x30 + 16·cpu)
pub unsafe fn load_tss(cpu: usize) {
    set_tss(cpu);
    let sel = (0x30 + 16 * cpu) as u16;
    asm!("ltr ax", in("ax") sel);
}

pub const KCODE: u16 = 0x08;
pub const UDATA: u64 = 0x20 | 3;
pub const UCODE: u64 = 0x28 | 3;

/// Kernstapel für Unterbrechungen aus Benutzerprogrammen (Ring 3 → 0)
pub fn set_kernel_stack(top: u64) {
    let cpu = crate::percpu::id();
    unsafe { core::ptr::addr_of_mut!(TSS[cpu].rsp).write_unaligned([top, 0, 0]) };
}

/// GDT (ohne TSS) erneut laden – für weitere Prozessorkerne
pub unsafe fn load_gdt_ap() {
    let dtr = Dtr { limit: (core::mem::size_of_val(&*core::ptr::addr_of!(GDT)) - 1) as u16, base: core::ptr::addr_of!(GDT) as u64 };
    asm!(
        "lgdt [{d}]",
        "push 0x08",
        "lea {t}, [rip + 2f]",
        "push {t}",
        "retfq",
        "2:",
        "mov ax, 0x10",
        "mov ds, ax", "mov es, ax", "mov ss, ax", "mov fs, ax",
        d = in(reg) &dtr, t = out(reg) _, out("ax") _,
    );
}

pub unsafe fn load_idt_ap() {
    let dtr = Dtr { limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16, base: core::ptr::addr_of!(IDT) as u64 };
    asm!("lidt [{}]", in(reg) &dtr);
}

pub unsafe fn init_gdt() {
    GDT[0] = 0;
    GDT[1] = 0x00af9a000000ffff; // Kern-Code 64 Bit
    GDT[2] = 0x00cf92000000ffff; // Kern-Daten
    GDT[3] = 0x00cffa000000ffff; // Benutzer-Code 32 Bit (nur Platzhalter für STAR)
    GDT[4] = 0x00cff2000000ffff; // Benutzer-Daten
    GDT[5] = 0x00affa000000ffff; // Benutzer-Code 64 Bit
    // ab 6: TSS-Deskriptoren (je 16 Byte) für jeden Kern
    load_gdt_ap();
    load_tss(0);
}

// ------------------------------------------------------------------ IDT

#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    off0: u16,
    sel: u16,
    ist: u8,
    attr: u8,
    off1: u16,
    off2: u32,
    zero: u32,
}

static mut IDT: [Gate; 256] = [Gate { off0: 0, sel: 0, ist: 0, attr: 0, off1: 0, off2: 0, zero: 0 }; 256];

#[repr(C)]
pub struct Frame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub fn set_gate(i: usize, h: u64) {
    set(i, h, 0)
}

fn set(i: usize, h: u64, ist: u8) {
    unsafe {
        IDT[i] = Gate { off0: h as u16, sel: KCODE, ist, attr: 0x8e, off1: (h >> 16) as u16, off2: (h >> 32) as u32, zero: 0 };
    }
}

fn fault(v: u64, f: &Frame, code: Option<u64>) -> ! {
    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2) };
    crate::power::firmware_fehler();
    crate::sprache::ausnahme_zeile(v, f.rip, f.rsp, code, cr2);
    halt_forever()
}

macro_rules! handler {
    ($n:ident, $txt:expr) => {
        extern "x86-interrupt" fn $n(f: Frame) {
            fault($txt, &f, None)
        }
    };
    ($n:ident, $txt:expr, code) => {
        extern "x86-interrupt" fn $n(f: Frame, c: u64) {
            fault($txt, &f, Some(c))
        }
    };
}
handler!(e_de, 0);
handler!(e_ud, 6);
handler!(e_nm, 7);
handler!(e_df, 8, code);
handler!(e_ts, 10, code);
handler!(e_np, 11, code);
handler!(e_ss, 12, code);
handler!(e_gp, 13, code);
handler!(e_pf, 14, code);
handler!(e_mf, 16);
handler!(e_ac, 17, code);
handler!(e_xm, 19);

extern "x86-interrupt" fn e_bp(_f: Frame) {
    crate::kprintln!("[kern] Haltepunkt");
}

pub unsafe fn init_idt() {
    set(0, e_de as u64, 0);
    set(3, e_bp as u64, 0);
    set(6, e_ud as u64, 0);
    set(7, e_nm as u64, 0);
    set(8, e_df as u64, 1);
    set(10, e_ts as u64, 0);
    set(11, e_np as u64, 0);
    set(12, e_ss as u64, 0);
    set(13, e_gp as u64, 0);
    set(14, e_pf as u64, 0);
    set(16, e_mf as u64, 0);
    set(17, e_ac as u64, 0);
    set(19, e_xm as u64, 0);
    let dtr = Dtr { limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16, base: core::ptr::addr_of!(IDT) as u64 };
    asm!("lidt [{}]", in(reg) &dtr);
    // alten 8259-Interruptcontroller stumm schalten (wir arbeiten mit Abfrage; APIC folgt)
    outb(0x21, 0xff);
    outb(0xa1, 0xff);
}

// ------------------------------------------------------------------ Zeit

static TSC_KHZ: AtomicU64 = AtomicU64::new(0);
static TSC_START: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// TSC-Takt in kHz, wie ihn der Prozessor selbst nennt (CPUID 0x15 TSC/Quarz-Verhältnis, sonst 0x16 Grundtakt).
/// Moderne Intel-Rechner (z. B. Core Ultra / Arrow Lake) schalten den alten PIT oft ab – dann wäre die PIT-Eichung falsch.
unsafe fn tsc_khz_cpuid() -> u64 {
    use core::arch::x86_64::__cpuid;
    let max = __cpuid(0).eax;
    if max < 0x15 {
        return 0;
    }
    let r = __cpuid(0x15);
    if r.eax == 0 || r.ebx == 0 {
        return 0;
    }
    let quarz = r.ecx as u64;
    if quarz == 0 && max >= 0x16 {
        // Quarz unbekannt: aus dem Grundtakt zurückrechnen
        let basis_mhz = __cpuid(0x16).eax as u64 & 0xffff;
        if basis_mhz != 0 {
            return basis_mhz * 1000;
        }
    }
    quarz * r.ebx as u64 / r.eax as u64 / 1000
}

/// TSC eichen: zuerst laut Prozessor (CPUID), sonst an PIT-Kanal 2 (10 ms Fenster), zuletzt Grundtakt
pub unsafe fn calibrate_tsc() {
    let khz = tsc_khz_cpuid();
    if (100_000..20_000_000).contains(&khz) {
        TSC_KHZ.store(khz, Ordering::Relaxed);
        TSC_START.store(rdtsc(), Ordering::Relaxed);
        TSC_QUELLE.store(1, Ordering::Relaxed);
        return;
    }
    calibrate_tsc_pit();
}

/// 0 = PIT, 1 = CPUID, 2 = Grundtakt (PIT lief nicht)
pub static TSC_QUELLE: AtomicU64 = AtomicU64::new(0);

unsafe fn calibrate_tsc_pit() {
    // Kanal 2, Modus 0, 1193182 Hz / 100 = 11932 Takte = 10 ms
    let gate = inb(0x61);
    outb(0x61, (gate & !0x02) | 0x01);
    outb(0x43, 0b1011_0000);
    let count: u16 = 11932;
    outb(0x42, count as u8);
    outb(0x42, (count >> 8) as u8);
    let t0 = rdtsc();
    let mut guard = 0u64;
    while inb(0x61) & 0x20 == 0 {
        guard += 1;
        if guard > 50_000_000 {
            break;
        }
    }
    let t1 = rdtsc();
    let mut khz = ((t1 - t0) / 10).max(1000);
    if guard > 50_000_000 {
        // PIT zählt nicht (abgeschaltet): Grundtakt des Prozessors nehmen, sonst 2 GHz annehmen
        let max = core::arch::x86_64::__cpuid(0).eax;
        let basis = if max >= 0x16 { core::arch::x86_64::__cpuid(0x16).eax as u64 & 0xffff } else { 0 };
        khz = if basis != 0 { basis * 1000 } else { 2_000_000 };
        TSC_QUELLE.store(2, Ordering::Relaxed);
    }
    TSC_KHZ.store(khz, Ordering::Relaxed);
    TSC_START.store(rdtsc(), Ordering::Relaxed);
}

pub fn tsc_khz() -> u64 {
    TSC_KHZ.load(Ordering::Relaxed).max(1)
}

/// Millisekunden seit Kernstart
pub fn uptime_ms() -> u64 {
    (rdtsc() - TSC_START.load(Ordering::Relaxed)) / tsc_khz()
}

/// Nanosekunden seit Kernstart
pub fn uptime_ns() -> u64 {
    let t = rdtsc() - TSC_START.load(Ordering::Relaxed);
    (t as u128 * 1_000_000 / tsc_khz() as u128) as u64
}

pub fn sleep_us(us: u64) {
    let end = rdtsc() + us * tsc_khz() / 1000;
    while rdtsc() < end {
        pause();
    }
}

/// CPU-Name über CPUID
pub fn cpu_name(out: &mut [u8; 48]) -> &str {
    unsafe {
        for (i, leaf) in [0x8000_0002u32, 0x8000_0003, 0x8000_0004].iter().enumerate() {
            let r = core::arch::x86_64::__cpuid(*leaf);
            for (j, v) in [r.eax, r.ebx, r.ecx, r.edx].iter().enumerate() {
                out[i * 16 + j * 4..i * 16 + j * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    let n = out.iter().position(|&b| b == 0).unwrap_or(48);
    core::str::from_utf8(&out[..n]).unwrap_or("?").trim()
}

/// Neustart über den Reset-Port des Chipsatzes (Rückfall, wenn die Firmware nicht mag)
pub fn reboot_hw() -> ! {
    unsafe {
        outb(0xcf9, 0x02);
        outb(0xcf9, 0x06);
        outb(0x64, 0xfe);
    }
    halt_forever()
}

// ------------------------------------------------------------------ Härtung und Zufall

/// Schutzbits des Prozessors setzen (falls vorhanden): SMEP (Kern führt nie Benutzercode aus),
/// SMAP (Kern liest/schreibt Benutzerspeicher nie direkt – nur über die Seitentabelle des Prozesses),
/// UMIP (Programme lesen keine Deskriptortabellen aus). Liefert die gesetzten Namen.
pub unsafe fn haerten() -> &'static str {
    let l7 = core::arch::x86_64::__cpuid_count(7, 0);
    let (smep, smap, umip) = (l7.ebx & (1 << 7) != 0, l7.ebx & (1 << 20) != 0, l7.ecx & (1 << 2) != 0);
    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4);
    if smep {
        cr4 |= 1 << 20;
    }
    if smap {
        cr4 |= 1 << 21;
    }
    if umip {
        cr4 |= 1 << 11;
    }
    core::arch::asm!("mov cr4, {}", in(reg) cr4);
    let name = match (smep, smap, umip) {
        (true, true, true) => "SMEP SMAP UMIP",
        (true, true, false) => "SMEP SMAP",
        (true, false, _) => "SMEP",
        _ => "–",
    };
    *SCHUTZ.lock() = name;
    name
}

/// gesetzte Prozessor-Schutzbits (für Anzeige und /proc/ultar/sicherheit)
pub static SCHUTZ: spin::Mutex<&'static str> = spin::Mutex::new("–");

static ZUSTAND: AtomicU64 = AtomicU64::new(0);

/// Zufallszahl: RDRAND (Hardware), sonst xorshift aus TSC-Rauschen. Für TCP-Folgenummern und Ports –
/// nicht vorhersagbar für Angreifer im Netz.
pub fn zufall() -> u64 {
    let mut x = ZUSTAND.load(Ordering::Relaxed) ^ rdtsc().rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    ZUSTAND.store(x, Ordering::Relaxed);
    if core::arch::x86_64::__cpuid(1).ecx & (1 << 30) != 0 {
        for _ in 0..10 {
            let mut v = 0u64;
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut v) } == 1 {
                return v ^ x;
            }
        }
    }
    x
}

/// Zufall von außen (virtio-rng, Hardware-Quellen) in den Vorrat mischen
pub fn zufall_mischen(v: u64) {
    let alt = ZUSTAND.load(Ordering::Relaxed);
    ZUSTAND.store((alt ^ v).rotate_left(29).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ v, Ordering::Relaxed);
}
