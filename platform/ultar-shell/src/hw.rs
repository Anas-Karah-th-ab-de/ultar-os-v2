//! Hardware-Erkennung und Treibersuche.
//!
//! Geräte ohne Treiber werden über sysfs gefunden (PCI, USB). Die Online-Suche holt das zum
//! laufenden Kernel passende Modulpaket aus den Paketquellen in den Ubuntu-Bereich und lädt
//! die Treiber von dort (gleicher Kernel, daher direkt nutzbar). Firmware wird aus
//! /compat/lib/firmware geladen. Erfolgreiche Treiber stehen in /data/ultar/drivers.list und
//! werden beim Start vom Dienst `drivers` wieder geladen.

use crate::sys::{self, Bg, Msg, COMPAT};

#[derive(Clone, Debug)]
pub struct HwDev {
    pub bus: &'static str,
    pub id: String,
    /// Schlüssel für die Übersetzung
    pub kind: &'static str,
    pub modalias: String,
    pub driver: Option<String>,
    /// Gerät braucht üblicherweise keinen eigenen Treiber (Brücken usw.)
    pub passive: bool,
}

fn rd(p: &std::path::Path, f: &str) -> String {
    std::fs::read_to_string(p.join(f)).unwrap_or_default().trim().to_string()
}

fn driver(p: &std::path::Path) -> Option<String> {
    std::fs::read_link(p.join("driver")).ok().and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
}

pub fn scan() -> Vec<HwDev> {
    let mut v = Vec::new();
    if let Ok(rd_) = std::fs::read_dir("/sys/bus/pci/devices") {
        for e in rd_.flatten() {
            let p = e.path();
            let class = u32::from_str_radix(rd(&p, "class").trim_start_matches("0x"), 16).unwrap_or(0);
            let base = class >> 16;
            let kind = match base {
                0x01 => "Storage controller",
                0x02 => "Network adapter",
                0x03 => "Graphics",
                0x04 => "Audio / video",
                0x07 => "Modem / serial",
                0x09 => "Input device",
                0x0c => "USB / bus controller",
                0x0d => "Wireless adapter",
                0x06 => "Bridge",
                _ => "Other device",
            };
            let id = format!("{}:{}", rd(&p, "vendor").trim_start_matches("0x"), rd(&p, "device").trim_start_matches("0x"));
            v.push(HwDev { bus: "PCI", id, kind, modalias: rd(&p, "modalias"), driver: driver(&p), passive: matches!(base, 0x05 | 0x06 | 0x08) });
        }
    }
    if let Ok(rd_) = std::fs::read_dir("/sys/bus/usb/devices") {
        for e in rd_.flatten() {
            let p = e.path();
            // nur Schnittstellen (Name enthält ':'), sie tragen den Treiber
            if !e.file_name().to_string_lossy().contains(':') {
                continue;
            }
            let class = u32::from_str_radix(&rd(&p, "bInterfaceClass"), 16).unwrap_or(0);
            let kind = match class {
                0x01 => "Audio / video",
                0x02 | 0x0a => "Modem / serial",
                0x03 => "Input device",
                0x08 => "Storage controller",
                0x0e => "Camera",
                0xe0 => "Wireless adapter",
                0x09 => "USB / bus controller",
                _ => "Other device",
            };
            let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
            let dev = std::fs::canonicalize(&p).ok().and_then(|c| c.parent().map(|x| x.to_path_buf())).unwrap_or(parent);
            let id = format!("{}:{}", rd(&dev, "idVendor"), rd(&dev, "idProduct"));
            v.push(HwDev { bus: "USB", id, kind, modalias: rd(&p, "modalias"), driver: driver(&p), passive: false });
        }
    }
    v.sort_by(|a, b| (a.driver.is_some(), a.bus, &a.id).cmp(&(b.driver.is_some(), b.bus, &b.id)));
    v
}

pub fn missing(devs: &[HwDev]) -> Vec<&HwDev> {
    devs.iter().filter(|d| d.driver.is_none() && !d.passive && !d.modalias.is_empty()).collect()
}

/// Vom Kernel vergeblich angeforderte Firmware-Dateien (aus dem Kernel-Protokoll).
pub fn missing_firmware() -> Vec<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut out = Vec::new();
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(0o4000).open("/dev/kmsg") else { return out };
    let mut buf = [0u8; 8192];
    while let Ok(n) = f.read(&mut buf) {
        if n == 0 {
            break;
        }
        let l = String::from_utf8_lossy(&buf[..n]);
        if l.contains("firmware") && l.contains("failed") {
            if let Some(name) = l.split("firmware load for ").nth(1).and_then(|r| r.split_whitespace().next()) {
                if !out.iter().any(|o| o == name) {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn has_pkg(name: &str) -> bool {
    !sys::capture(COMPAT, &["run", "--", "/usr/bin/apt-cache", "show", "--no-all-versions", name]).trim().is_empty()
}

/// Online nach Treibern suchen und sie laden. Läuft im Hintergrund; Fortschritt als JobLine.
pub fn fix_online(bg: &Bg) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let say = |s: String| bg.send(Msg::JobLine(s));
        let kver = sys::read_trim("/proc/sys/kernel/osrelease");
        // Hersteller-Pfad (/vendor/firmware, von ultar-init gesetzt) nicht überschreiben – sonst finden Video/Kamera nichts mehr
        if sys::read_trim("/sys/module/firmware_class/parameters/path").is_empty() {
            let _ = std::fs::write("/sys/module/firmware_class/parameters/path", "/compat/lib/firmware");
        }
        let before = missing(&scan()).len();
        say(format!("Kernel {kver}: {before} device(s) without driver"));
        let code = bg.stream(COMPAT, &args(&["apt", "update"]), Msg::JobLine);
        if code != 0 {
            bg.send(Msg::JobDone(code));
            return;
        }
        let mut pkgs = vec![format!("linux-modules-{kver}"), format!("linux-modules-extra-{kver}")];
        pkgs.retain(|p| has_pkg(p));
        if pkgs.is_empty() {
            say(format!("No driver package for kernel {kver} in the package sources."));
            bg.send(Msg::JobDone(2));
            return;
        }
        let mut fixed: Vec<String> = Vec::new();
        for (i, pkg) in pkgs.iter().enumerate() {
            if missing(&scan()).is_empty() && i > 0 {
                break;
            }
            let code = bg.stream(COMPAT, &args(&["apt", "install", "--no-install-recommends", "kmod", pkg]), Msg::JobLine);
            if code != 0 {
                say(format!("{pkg}: installation failed (code {code})"));
                continue;
            }
            bg.stream(COMPAT, &args(&["run", "--", "/usr/sbin/depmod", &kver]), Msg::JobLine);
            for d in missing(&scan()) {
                let code = bg.stream(COMPAT, &args(&["run", "--", "/usr/sbin/modprobe", "-b", &d.modalias]), Msg::JobLine);
                say(format!("{} {} ({}): {}", d.bus, d.id, d.kind, if code == 0 { "driver loaded" } else { "no driver found" }));
                if code == 0 {
                    fixed.push(d.modalias.clone());
                }
            }
        }
        let fw = missing_firmware();
        if !fw.is_empty() {
            say(format!("Missing firmware: {}", fw.join(", ")));
            let free = sys::disk("/compat").map(|d| d.1).unwrap_or(0);
            if !has_pkg("linux-firmware") {
                say("Package linux-firmware not available.".into());
            } else if free < 900 << 20 {
                say(format!("Not enough space in the Ubuntu area for linux-firmware ({} free).", sys::human(free)));
            } else {
                bg.stream(COMPAT, &args(&["apt", "install", "--no-install-recommends", "linux-firmware"]), Msg::JobLine);
            }
        }
        // für den nächsten Start merken
        if !fixed.is_empty() {
            let path = format!("{}/drivers.list", sys::data_dir());
            let mut all: Vec<String> = std::fs::read_to_string(&path).unwrap_or_default().lines().map(String::from).collect();
            for f in fixed {
                if !all.contains(&f) {
                    all.push(f);
                }
            }
            let _ = std::fs::create_dir_all(sys::data_dir());
            let _ = std::fs::write(&path, all.join("\n") + "\n");
        }
        let after = missing(&scan()).len();
        say(format!("{} of {before} device(s) now have a driver.", before - after.min(before)));
        bg.send(Msg::HwChanged);
        bg.send(Msg::JobDone(if after == 0 { 0 } else { 1 }));
    });
}
