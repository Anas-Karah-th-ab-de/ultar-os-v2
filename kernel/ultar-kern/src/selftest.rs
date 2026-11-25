//! Selbsttest auf echter Hardware: läuft beim Start, wenn auf einer FAT-Partition (z. B. dem USB-Stick)
//! die Datei `ultar-kern/selbsttest.txt` liegt. Prüft Prozessor, Speicher, Geräte, Datenträger, Netz und
//! Programme in Ring 3 und schreibt das ganze Kernprotokoll in diese Datei (an Ort und Stelle, ohne die
//! FAT-Verwaltung zu ändern). Auf den Datenträgern des Rechners wird nichts geschrieben.

use crate::driver::{BLOCK, DEVICES, NET};
use crate::{kprintln, shell};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

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

/// One more program, named by the test medium in `ultar/testprogramm`: the first line is the
/// command line, every further line `NAME=value` for its environment, the word `spur` (log its
/// system calls) or `abgewiesen` (log the requests the graphics node refuses). Runs after the
/// graphics tests, e.g. a Vulkan probe from a package disk.
fn extra_program() {
    let Some(text) = ["/ultar/testprogramm", "/boot/efi/ultar/testprogramm"].iter().find_map(|p| crate::fs::read_all(p)) else { return };
    let text = String::from_utf8_lossy(&text).into_owned();
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let argv: Vec<String> = lines.next().unwrap_or("").split_whitespace().map(String::from).collect();
    let Some(path) = argv.first().cloned() else { return };
    let mut env = Vec::new();
    let mut trace = false;
    for l in lines {
        if l == "spur" {
            trace = true;
        } else if let Some(n) = l.strip_prefix("gpu-pat=").and_then(|n| n.parse::<u8>().ok()) {
            crate::drivers::intel_display::gt_exec::FORCE_PAT.store(n, core::sync::atomic::Ordering::Relaxed);
            kprintln!("[test] GPU cache attribute forced to {n} (measuring only)");
        } else if l == "gpu-wc-all" {
            crate::drivers::intel_display::gt_exec::WC_ALL.store(true, core::sync::atomic::Ordering::Relaxed);
            kprintln!("[test] every GPU buffer write-combining, not coherent (measuring only)");
        } else if l == "abgewiesen" {
            crate::svcdev::dri::LOG_REFUSED.store(true, core::sync::atomic::Ordering::Relaxed);
        } else {
            env.push(String::from(l));
        }
    }
    kprintln!("[test] Zusatzprogramm {path}:");
    crate::syscall::TRACE.store(trace, core::sync::atomic::Ordering::Relaxed);
    match crate::proc::spawn(&path, argv, env) {
        Ok(_) => crate::proc::run_all(),
        Err(e) => kprintln!("  Fehler: {e}"),
    }
    crate::syscall::TRACE.store(false, core::sync::atomic::Ordering::Relaxed);
    kprintln!("[test] Zusatzprogramm beendet");
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
    // ECAM: jedes Gerät muss über das Speicherfenster dieselbe Kennung liefern wie über die Ports
    if crate::pci::ecam_available() {
        let mut gleich = 0;
        let mut fehler = 0;
        let mut ext = 0;
        for b in DEVICES.lock().iter() {
            let d = &b.dev;
            if d.r32x(0) == Some(d.r32(0)) {
                gleich += 1;
            } else {
                fehler += 1;
                kprintln!("  ECAM-Abweichung bei {}", d.id());
            }
            ext += d.ext_caps().len();
        }
        kprintln!("[test] ECAM: {gleich} Geräte gleich, {fehler} abweichend, {ext} erweiterte Fähigkeiten {}", if fehler == 0 { "OK" } else { "FEHLER" });
    } else {
        kprintln!("[test] ECAM: nicht verfügbar");
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
            match crate::net::ping(crate::net::active_nic(), c.gw) {
                Some(ms) => kprintln!("[test] Ping Router: {ms} ms OK"),
                None => kprintln!("[test] Ping Router: keine Antwort"),
            }
        }
    }
    // Programme in Ring 3 (vom Stick): Fäden, Signale, Sockets, epoll, glibc; zelltest = escape tests of the isolation;
    // grenztest = resource groups, limits and the attacks that must not stop the machine;
    // ultar-compositor-testclient kerncheck = compositor on fb0 + two app processes (verdict COMPOSITOR-CHECK:)
    for (p, args) in [("kerntest", vec!["kerntest"]), ("ereignis", vec!["ereignis"]), ("hallo-glibc", vec!["hallo-glibc"]), ("busck", vec!["busck"]), ("zelltest", vec!["zelltest"]), ("rechtetest", vec!["rechtetest"]), ("geraetetest", vec!["geraetetest"]), ("weckertest", vec!["weckertest"]), ("spawntest", vec!["spawntest"]), ("fenstertest", vec!["fenstertest"]), ("grenztest", vec!["grenztest"]), ("ultar-compositor-testclient", vec!["ultar-compositor-testclient", "/tmp/ultar-ui", "kerncheck", "/bin/ultar-compositor"])] {
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
    // Intel display: takeover, flips with CRC proof, vblank rate, hot-plug, EDID (INTELTEST: …)
    crate::drivers::intel_display::selftest::run();
    // Intel GT (render/compute side): identification, engines, GuC image and signature (GTTEST: …)
    crate::drivers::intel_display::gt::selftest();
    // a program on the GPU render node the GT test published (DRITEST: …); only on the test medium
    if let Some(path) = programm("dritest") {
        kprintln!("[test] Programm dritest:");
        match crate::proc::spawn(&path, vec![String::from("dritest")], vec![String::from("PATH=/bin")]) {
            Ok(_) => crate::proc::run_all(),
            Err(e) => kprintln!("  Fehler: {e}"),
        }
    }
    extra_program();
    // Intel NPU: clock fuses, power-up to a running IP block, power-down (NPUTEST: …)
    crate::drivers::intel_npu::selftest();
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
        crate::power::aus()
    }
    kprintln!("[test] fertig – `aus` schaltet aus, `neustart` startet neu");
}
