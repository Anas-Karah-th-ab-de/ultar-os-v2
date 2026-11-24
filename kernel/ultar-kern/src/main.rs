//! Ultar-Kern – eigener Rust-Kernel von KM Code Craft.
//!
//! Startet als UEFI-Programm, holt sich Bildschirm, Speicherkarte und ACPI von der Firmware, verlässt sie
//! (ExitBootServices) und betreibt die Maschine danach allein: eigene GDT/IDT, eigener Speicher, eigene
//! Treiber (PCI, virtio, NVMe, Tastatur), eigener Netzwerkstapel – ohne Linux.
//! Geräte ohne Treiber meldet die Treiber-Werkstatt als Bauplan (siehe werkstatt.rs, tools/treiber-werkstatt).

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod apic;
mod bootslot;
mod buddy;
mod arch;
mod console;
mod cred;
mod devices;
mod input;
mod memfd;
mod scm;
mod shm;
mod driver;
mod drivers;
mod firewall;
mod flock;
mod fs;
mod geraet;
mod grafik;
mod groups;
mod heap;
mod install;
mod isolation;
mod power;
mod sprache;
mod startseite;
mod svcdev;
mod efi;
mod eingabe;
mod ext4;
mod klog;
mod mem;
mod net;
mod oom;
mod paging;
mod percpu;
mod pci;
mod perm;
mod pipe;
mod proc;
mod procfs;
mod rawpart;
mod shell;
mod selftest;
mod signal;
mod slab;
mod socket;
mod syscall;
mod trap;
mod trybuf;
mod uds;
mod verbund;
mod werkstatt;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kprintln!("\n{}: {info}", sprache::t("panik"));
    // a counted attempt of a new slot restarts, so the loader can fall back
    bootslot::start_failed("kernel panic");
    arch::halt_forever()
}

#[no_mangle]
pub extern "efiapi" fn efi_main(image: efi::Handle, st: *mut efi::SystemTable) -> efi::Status {
    // A/B start: the slot the loader chose (load options), before anything else depends on it
    let mut options = [0u16; ultar_slots::layout::OPTIONS_MAX];
    // SAFETY: still in the firmware phase; `image`/`st` are what the firmware passed.
    let n = unsafe { efi::load_options(image, st, &mut options) };
    // the display driver's opt-in word is not one of the A/B chain's (which refuses unknown ones)
    let n = drivers::intel_display::take_boot_option(&mut options, n);
    let n = drivers::nvidia_display::take_boot_option(&mut options, n);
    bootslot::init(&options[..n]);
    let mut schalter = [0u8; 8];
    // SAFETY: still in the firmware phase; `image`/`st` are what the firmware passed.
    // SAFETY: firmware image, system table and file buffer remain valid in the boot phase.
    if let Some(k) = unsafe { efi::datei_lesen(image, st, ultar_intel_display::optin::FILE_PATH, &mut schalter) } {
        drivers::intel_display::opt_in_file(&schalter[..k]);
    }
    // SAFETY: firmware image, system table and file buffer remain valid in the boot phase.
    if let Some(k) = unsafe { efi::datei_lesen(image, st, drivers::nvidia_display::FILE_PATH, &mut schalter) } {
        drivers::nvidia_display::opt_in_file(&schalter[..k]);
    }
    // Systemsprache von der Startpartition (installiertes System): gilt ab dem ersten Bild
    if let Some(l) = unsafe { efi::sprache_lesen(image, st) } {
        sprache::set(l);
        sprache::FEST.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    efi::print(st, concat!(ultar_brand::product!(), " ...\n"));
    // Installationsmedium mit bereits installierter Platte: installiertes System starten (Taste I = Installation)
    unsafe { efi::kettenstart(image, st) };
    let boot = unsafe { efi::take_over(image, st) };
    efi::set_runtime(boot.runtime);
    // Ab hier gehört die Maschine dem Ultar-Kern
    unsafe { core::arch::asm!("cli") };
    console::serial_init();
    console::screen_init(boot.fb);
    // installiertes System: ruhiger Start (Protokoll nur seriell und über `protokoll`), Startbild statt Textflut
    let ruhig = sprache::FEST.load(core::sync::atomic::Ordering::Relaxed);
    console::grafik(ruhig);
    unsafe {
        percpu::init(0);
        arch::init_gdt();
        arch::init_idt();
        trap::init();
        arch::calibrate_tsc();
        bootslot::report();
        let fb = boot.fb.map(|f| (f.base, f.stride as u64 * f.height as u64 * 4));
        // Prozessor-Schutz an: SMEP/SMAP/UMIP (der Kern greift auf Benutzerspeicher nur über die Seitentabelle zu)
        arch::haerten();
        paging::init(mem::top(&boot), fb);
        mem::init(&boot);
        apic::init();
        #[cfg(feature = "remote-test")]
        apic::arm_deadline(match option_env!("ULTAR_DEADLINE_MS") {
            Some(ms) => ms.parse().unwrap_or(180_000),
            None => 180_000,
        });
        // freie Seite unter 1 MB für den Startcode der weiteren Kerne (bevorzugt 0x8000)
        let free = |a: u64| boot.regions().any(|d| crate::efi::usable(d.typ) && d.phys <= a && d.phys + d.pages * 4096 >= a + 0x1000);
        // zwei Seiten: Startcode + Kopie der obersten Seitentabelle (muss unter 4 GB liegen)
        let ok2 = |a: u64| free(a) && free(a + 0x1000);
        let tramp = if ok2(0x8000) { 0x8000 } else { (0x1000..0x9e000u64).step_by(0x1000).find(|&a| ok2(a)).unwrap_or(0) };
        if tramp == 0 {
            for d in boot.regions().filter(|d| d.phys < 0x10_0000) {
                kprintln!("[smp] Speicher unter 1 MB: {:#x} +{} Seiten Typ {}", d.phys, d.pages, d.typ);
            }
        }
        apic::start_aps(boot.rsdp, tramp);
    }
    startseite::startbild(15, "p_kern");
    kprintln!(concat!("Ultar-Kern {} – eigener Kernel von ", ultar_brand::vendor!(), " (Rust, ohne Linux)"), env!("CARGO_PKG_VERSION"));
    kprintln!("[schutz] Prozessor: {}, NX, Netz standardmäßig zu (nichts von außen erreichbar)", *arch::SCHUTZ.lock());
    if let Some(f) = boot.fb {
        kprintln!("[kern] Bildschirm {}x{} ({})", f.width, f.height, if f.bgr { "BGR" } else { "RGB" });
    }
    kprintln!("[kern] ACPI-Wurzel {:#x}", boot.rsdp);
    power::init(boot.rsdp);
    grafik::init_gop(boot.fb);
    geraet::init(boot.smbios, boot.rsdp);
    shell::info();

    startseite::startbild(35, "p_hw");
    match pci::init_ecam(boot.rsdp) {
        0 => kprintln!("[pci] kein ECAM (MCFG fehlt): erweiterter Konfigurationsraum nicht verfügbar"),
        b => kprintln!("[pci] ECAM bei {:#x}: erweiterter Konfigurationsraum verfügbar", b),
    }
    let devs = pci::scan();
    kprintln!("[pci] {} Gerät(e) gefunden", devs.len());
    driver::bind_all(devs);
    startseite::startbild(60, "p_net");
    if !driver::NET.lock().is_empty() {
        shell::dhcp();
    }
    let n = driver::BLOCK.lock().len();
    for i in 0..n {
        shell::partitions(i);
    }
    startseite::startbild(80, "p_disk");
    rawpart::scan();
    fs::mount();
    perm::standard_dirs();
    sprache::laden();
    power::optionen();
    firewall::laden();
    syscall::init_clock();
    proc::check_window();
    shell::entwickler_starten();
    if selftest::wanted() {
        selftest::run();
    } else {
        werkstatt::report();
    }
    kprintln!("{}", sprache::t("bereit").replace("{}", &alloc::format!("{}", arch::uptime_ms())));
    // Installationsmedium (Startpartition ULTAR-INSTALL): grafische Installation direkt starten
    if install::startmedium_ist_installer() {
        if !install::sitzung_starten() {
            install::run(boot.runtime);
        }
    } else if !selftest::wanted() && (ruhig || fs::read_all("/etc/ultar/sprache").is_some()) {
        // installiertes System: Oberfläche in der Systemsprache; „Terminal“ führt zur Konsole
        startseite::startbild(100, "p_fertig");
        // Desktop-Oberfläche ohne Linux (ultar-shell als Programm auf dem Ultar-Kern); ohne sie die Startseite
        // endet sie (oder fehlt sie), folgt die Startseite des Kerns
        // with `/etc/ultar/session` = init the service manager owns the session (services, compositor)
        if !shell::init_starten() {
            let _ = shell::desktop_starten();
        }
        startseite::run();
    }
    console::grafik(false);
    shell::run(boot.runtime)
}

/// Angriffsfläche auf einen Blick (/proc/ultar/sicherheit, Konsole `sicherheit`)
pub fn sicherheit_text() -> alloc::string::String {
    alloc::format!(
        "prozessor {}\nnx an\nnetz von aussen: {}\nping-antwort {}\nverbund {}\nverworfen {}\ntcp-folgenummern zufaellig (rdrand)\nports zufaellig\ngrenzen: sockets 4096, halboffen 64/port, empfangspuffer 256k, arp 256\n",
        *arch::SCHUTZ.lock(),
        { let t = firewall::text(); if t.contains("offen ") { "nur geoeffnete ports" } else { "alles zu" } },
        if firewall::PING.load(core::sync::atomic::Ordering::Relaxed) { "an" } else { "aus" },
        if verbund::aktiv() { "an (47800/47801)" } else { "aus" },
        firewall::VERWORFEN.load(core::sync::atomic::Ordering::Relaxed)
    )
}
