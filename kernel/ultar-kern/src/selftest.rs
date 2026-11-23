//! Selbsttest auf echter Hardware: läuft beim Start, wenn auf einer FAT-Partition (z. B. dem USB-Stick)
//! die Datei `ultar-kern/selbsttest.txt` liegt. Prüft Prozessor, Speicher, Geräte, Datenträger, Netz und
//! Programme in Ring 3 und schreibt das ganze Kernprotokoll in diese Datei (an Ort und Stelle, ohne die
//! FAT-Verwaltung zu ändern). Auf den Datenträgern des Rechners wird nichts geschrieben.

use crate::driver::{BLOCK, DEVICES, NET};
use crate::{kprintln, shell};
use alloc::string::String;
use alloc::vec;

const DATEI: &str = "ultar-kern/selbsttest.txt";

pub fn wanted() -> bool {
    #[cfg(feature = "remote-test")]
    return true;
    #[allow(unreachable_code)]
    crate::fs::fat_has(DATEI)
}

fn programm(name: &str) -> Option<String> {
    ["/bin/", "/boot/efi/bin/"].iter().map(|d| alloc::format!("{d}{name}")).find(|p| matches!(crate::fs::stat(p), Some((0, _))))
}

pub fn run() {
    // Only for proving the restart deadline in the emulator: hang on purpose.
    if option_env!("ULTAR_TEST_HANG").is_some() {
        kprintln!("[test] hanging on purpose");
        loop {
            core::hint::spin_loop();
        }
    }
    let t0 = crate::arch::uptime_ms();
    kprintln!("=== Ultar-Kern Selbsttest (Version {}) ===", env!("CARGO_PKG_VERSION"));
    kprintln!("Zeit (RTC, UTC): {} s seit 1970", crate::syscall::epoch_secs());
    shell::info();
    let (on, all) = crate::apic::cpu_count();
    kprintln!("[test] Prozessorkerne: {on} von {all} gestartet {}", if on as usize == all { "OK" } else { "– TEILWEISE" });
    kprintln!("[test] PCI-Geräte:");
    for b in DEVICES.lock().iter() {
        let d = &b.dev;
        kprintln!("  {}  {:<28} {}", d.id(), d.class_name(), b.driver.unwrap_or(if b.error.is_some() { "FEHLER" } else { "-" }));
        if let Some(e) = &b.error {
            kprintln!("      Fehler: {e}");
        }
    }
    kprintln!("[test] Datenträger (nur lesen):");
    let n = BLOCK.lock().len();
    for i in 0..n {
        let r = {
            let mut bl = BLOCK.lock();
            let d = &mut bl[i];
            let mut buf = vec![0u8; d.sector_size().max(512)];
            let t = crate::arch::uptime_ms();
            let ok = d.read(0, &mut buf).is_ok();
            (d.name(), d.sectors() * d.sector_size() as u64 >> 20, ok, crate::arch::uptime_ms() - t)
        };
        kprintln!("  {}: {} MB, Sektor 0 lesen {} ({} ms)", r.0, r.1, if r.2 { "OK" } else { "FEHLER" }, r.3);
        shell::partitions(i);
    }
    kprintln!("[test] Dateisysteme:");
    for (p, t) in crate::fs::mounts() {
        kprintln!("  {p} ({t})");
    }
    if NET.lock().is_empty() {
        kprintln!("[test] Netz: kein Netzwerkgerät mit Treiber");
    } else {
        let cfg = *crate::net::CONFIG.lock();
        match cfg {
            Some(c) => kprintln!("[test] Netz: DHCP OK, IP {}.{}.{}.{}", c.ip[0], c.ip[1], c.ip[2], c.ip[3]),
            None => kprintln!("[test] Netz: DHCP ohne Antwort (Kabel gesteckt?)"),
        }
        if let Some(c) = cfg {
            match crate::net::ping(0, c.gw) {
                Some(ms) => kprintln!("[test] Ping Router: {ms} ms OK"),
                None => kprintln!("[test] Ping Router: keine Antwort"),
            }
        }
    }
    // Programme in Ring 3 (vom Stick): Fäden, Signale, Sockets, epoll, glibc; zelltest = escape tests of the isolation;
    // grenztest = resource groups, limits and the attacks that must not stop the machine
    for (p, args) in [("kerntest", vec!["kerntest"]), ("ereignis", vec!["ereignis"]), ("hallo-glibc", vec!["hallo-glibc"]), ("busck", vec!["busck"]), ("zelltest", vec!["zelltest"]), ("rechtetest", vec!["rechtetest"]), ("grenztest", vec!["grenztest"])] {
        match programm(p) {
            Some(path) => {
                kprintln!("[test] Programm {p}:");
                let argv = args.into_iter().map(String::from).collect();
                match crate::proc::spawn(&path, argv, vec![String::from("PATH=/bin")]) {
                    Ok(_) => crate::proc::run_all(),
                    Err(e) => kprintln!("  Fehler: {e}"),
                }
            }
            None => kprintln!("[test] Programm {p}: nicht auf dem Stick"),
        }
    }
    crate::werkstatt::report();
    kprintln!("[test] Dauer des Selbsttests: {} ms, Kern läuft seit {} ms", crate::arch::uptime_ms() - t0, crate::arch::uptime_ms());
    // Protokoll schreiben
    let mut log = alloc::vec::Vec::new();
    crate::klog::dump(|b| log.extend_from_slice(b));
    match crate::fs::fat_overwrite(DATEI, &log) {
        Some(p) => kprintln!("[test] Protokoll geschrieben: {p} – Stick unter Windows öffnen und Datei schicken"),
        None => kprintln!("[test] Protokoll konnte nicht geschrieben werden (Bildschirmfoto machen)"),
    }
    #[cfg(feature = "remote-test")]
    {
        kprintln!("[test] remote-test: restarting in 3 s");
        crate::arch::sleep_us(3_000_000);
        crate::power::neustart();
    }
    #[allow(unreachable_code)]
    if crate::fs::fat_has("ultar-kern/auto-aus") {
        kprintln!("[test] auto-aus: schalte in 5 s aus");
        crate::arch::sleep_us(5_000_000);
        unsafe { crate::efi::reset(crate::efi::runtime(), 2) }
    }
    kprintln!("[test] fertig – `aus` schaltet aus, `neustart` startet neu");
}
