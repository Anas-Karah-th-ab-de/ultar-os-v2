//! Geräteschalter: findet für jede Grundfunktion (Vibration, USB-Diagnose, Wachhund, Bildschirm, Hintergrundlicht)
//! den Weg, der auf DIESEM Gerät funktioniert, und merkt ihn sich.
//!
//! Erster Start: je Funktion werden alle bekannten Fälle der Reihe nach probiert; der erste, der funktioniert,
//! wird im Geräteprofil gespeichert (`/data/ultar/hw-profile.toml`).
//! Jeder weitere Start: nur der gemerkte Fall wird ausgeführt. Funktioniert er nicht mehr (anderer Kernel,
//! andere Hardware), wird für diese eine Funktion neu gesucht und das Profil berichtigt.
//! Das Profil gilt nur für das Gerät, auf dem es entstanden ist (Kennung aus Gerätebaum/DMI + Kernel).

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};

pub const PROFILE: &str = "/data/ultar/hw-profile.toml";
/// Abbild des Profils für andere Programme (Oberfläche, Diagnose) – auch wenn /data fehlt.
pub const RUNTIME: &str = "/run/ultar/hw.toml";

/// Ein Fall: Name + Probe. Die Probe richtet die Funktion ein und meldet, ob es geklappt hat.
pub struct Case<'a> {
    pub name: &'a str,
    pub probe: &'a dyn Fn() -> bool,
}

#[derive(Default)]
pub struct Profile {
    pub device: String,
    pub map: BTreeMap<String, String>,
    changed: bool,
    known: bool,
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default().trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
}

/// Kennung des Geräts: Modell aus dem Gerätebaum (ARM) oder DMI (PC) plus Kernel-Version.
pub fn device_id() -> String {
    let mut model = read("/proc/device-tree/model");
    if model.is_empty() {
        model = format!("{} {}", read("/sys/class/dmi/id/sys_vendor"), read("/sys/class/dmi/id/product_name")).trim().to_string();
    }
    if model.is_empty() {
        model = "unbekannt".into();
    }
    format!("{model} | {}", read("/proc/sys/kernel/osrelease")).replace('"', "'")
}

impl Profile {
    /// Profil laden. Passt die Gerätekennung nicht (anderes Gerät, anderer Kernel), wird es verworfen.
    pub fn load() -> Self {
        let device = device_id();
        let mut p = Profile { device: device.clone(), ..Default::default() };
        let Ok(text) = fs::read_to_string(PROFILE) else { return p };
        let Ok(t) = text.parse::<toml::Table>() else { return p };
        if t.get("device").and_then(|v| v.as_str()) != Some(device.as_str()) {
            crate::log("Geräteschalter: gespeichertes Profil gehört zu einem anderen Gerät/Kernel – es wird neu gesucht");
            return p;
        }
        for (k, v) in t.iter().filter(|(k, _)| k.as_str() != "device") {
            if let Some(s) = v.as_str() {
                p.map.insert(k.clone(), s.to_string());
            }
        }
        p.known = !p.map.is_empty();
        p
    }

    /// true, wenn dieses Gerät schon bekannt ist (kein erster Start).
    pub fn known(&self) -> bool {
        self.known
    }

    /// Den passenden Fall für eine Funktion wählen und ausführen.
    /// Gemerkter Fall zuerst und allein; nur wenn er versagt (oder nichts gemerkt ist), alle Fälle der Reihe nach.
    /// Passt keiner, wird "none" gemerkt – beim nächsten Start wird dann einmal neu gesucht.
    pub fn choose(&mut self, func: &str, cases: &[Case]) -> String {
        if let Some(prev) = self.map.get(func).cloned() {
            if let Some(c) = cases.iter().find(|c| c.name == prev) {
                if (c.probe)() {
                    crate::log(&format!("Geräteschalter: {func} = {prev} (gemerkt)"));
                    return prev;
                }
                crate::log(&format!("Geräteschalter: {func} = {prev} funktioniert nicht mehr – suche neu"));
            }
        }
        for c in cases {
            if (c.probe)() {
                crate::log(&format!("Geräteschalter: {func} = {} (gefunden)", c.name));
                self.set(func, c.name);
                return c.name.to_string();
            }
        }
        crate::log(&format!("Geräteschalter: {func} = none (kein Fall passt)"));
        self.set(func, "none");
        "none".into()
    }

    pub fn set(&mut self, func: &str, value: &str) {
        if self.map.get(func).map(String::as_str) != Some(value) {
            self.map.insert(func.to_string(), value.to_string());
            self.changed = true;
        }
    }

    fn text(&self) -> String {
        let mut s = format!("# Ultar OS – Geräteprofil (vom Geräteschalter erzeugt; löschen = beim nächsten Start neu suchen)\ndevice = \"{}\"\n", self.device);
        for (k, v) in &self.map {
            s.push_str(&format!("{k} = \"{}\"\n", v.replace('\\', "/").replace('"', "'")));
        }
        s
    }

    /// Profil sichern: immer nach /run (für andere Programme), dauerhaft nach /data nur bei Änderung.
    pub fn save(&mut self) {
        let text = self.text();
        let _ = fs::create_dir_all("/run/ultar");
        let _ = fs::write(RUNTIME, &text);
        if !self.changed {
            return;
        }
        // Nur schreiben, wenn /data wirklich eine eingehängte Datenpartition ist (nicht ins flüchtige Startabbild)
        let mounted = fs::read_to_string("/proc/mounts").map(|m| m.lines().any(|l| l.split_whitespace().nth(1) == Some("/data"))).unwrap_or(false);
        if !mounted {
            crate::log("Geräteschalter: keine Datenpartition – Profil gilt nur für diesen Start");
            return;
        }
        let _ = fs::create_dir_all("/data/ultar");
        let tmp = format!("{PROFILE}.tmp");
        let ok = fs::File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all())).is_ok() && fs::rename(&tmp, PROFILE).is_ok();
        if ok {
            if let Ok(d) = fs::File::open("/data/ultar") {
                let _ = d.sync_all();
            }
            self.changed = false;
            crate::log(&format!("Geräteschalter: Profil gespeichert ({} Funktionen)", self.map.len()));
        } else {
            crate::log("Geräteschalter: Profil konnte nicht gespeichert werden");
        }
    }
}

// ---------------------------------------------------------------- Partition über ihren Namen finden

/// Name (Label) eines ext2/3/4-Dateisystems lesen. Nur lesend.
fn ext_label(dev: &str) -> Option<String> {
    let mut f = fs::File::open(dev).ok()?;
    let mut sb = [0u8; 264];
    f.seek(SeekFrom::Start(1024)).ok()?;
    f.read_exact(&mut sb).ok()?;
    if sb[56] != 0x53 || sb[57] != 0xEF {
        return None; // kein ext-Dateisystem
    }
    let raw = &sb[120..136];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(16);
    Some(String::from_utf8_lossy(&raw[..end]).into_owned())
}

/// Blockgerät mit dem Dateisystem-Namen `label` suchen (alle Platten und Partitionen, nur lesend).
pub fn find_by_label(label: &str) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir("/sys/class/block").ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.retain(|n| !n.starts_with("loop") && !n.starts_with("ram") && !n.starts_with("zram"));
    names.sort();
    names.into_iter().map(|n| format!("/dev/{n}")).find(|d| ext_label(d).as_deref() == Some(label) || fat_label(d).as_deref() == Some(label))
}

/// Name eines FAT-Dateisystems (EFI-Systempartition) aus dem Bootsektor
fn fat_label(dev: &str) -> Option<String> {
    use std::io::Read;
    let mut b = [0u8; 512];
    fs::File::open(dev).ok()?.read_exact(&mut b).ok()?;
    if b[510] != 0x55 || b[511] != 0xAA {
        return None;
    }
    let raw = if &b[82..87] == b"FAT32" { &b[71..82] } else if &b[54..57] == b"FAT" { &b[43..54] } else { return None };
    let s = String::from_utf8_lossy(raw).trim_end().to_string();
    (!s.is_empty() && s != "NO NAME").then_some(s)
}

/// Gerät mit diesem GPT-Partitionsnamen (PARTNAME aus sysfs)
pub fn find_by_partlabel(name: &str) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir("/sys/class/block").ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names.into_iter().find(|n| fs::read_to_string(format!("/sys/class/block/{n}/uevent")).unwrap_or_default().lines().any(|l| l.strip_prefix("PARTNAME=") == Some(name))).map(|n| format!("/dev/{n}"))
}

/// Quelle eines Mount-Eintrags auflösen. Mehrere Kandidaten mit `|` getrennt, der erste vorhandene gilt:
/// `LABEL=name` (Suche über alle Blockgeräte) oder ein Gerätepfad.
pub fn resolve_source(spec: &str) -> Option<String> {
    for cand in spec.split('|').map(str::trim).filter(|c| !c.is_empty()) {
        if let Some(pl) = cand.strip_prefix("PARTLABEL=") {
            if let Some(d) = find_by_partlabel(pl) {
                return Some(d);
            }
        } else if let Some(label) = cand.strip_prefix("LABEL=") {
            if let Some(d) = find_by_label(label) {
                return Some(d);
            }
        } else if !cand.starts_with("/dev/") || fs::metadata(cand).is_ok() {
            return Some(cand.to_string()); // Pseudo-Dateisysteme (tmpfs …) oder vorhandenes Gerät
        }
    }
    None
}

// ---------------------------------------------------------------- Vibration

const VIB_TIMED: &str = "/sys/class/timed_output/vibrator/enable";
const VIB_LEDS: &str = "/sys/class/leds/vibrator";

fn w(path: &str, v: &str) -> bool {
    fs::write(path, v).is_ok()
}

/// Mit dem gewählten Fall vibrieren (Millisekunden).
pub fn vibrate(case: &str, ms: u32) {
    match case {
        "timed_output" => {
            w(VIB_TIMED, &ms.to_string());
        }
        "leds" => {
            w(&format!("{VIB_LEDS}/duration"), &ms.to_string());
            w(&format!("{VIB_LEDS}/activate"), "1");
        }
        _ => {}
    }
}

pub fn pick_vibrator(p: &mut Profile) -> String {
    p.choose(
        "vibrator",
        &[
            Case { name: "timed_output", probe: &|| w(VIB_TIMED, "150") },
            Case { name: "leds", probe: &|| w(&format!("{VIB_LEDS}/duration"), "150") && w(&format!("{VIB_LEDS}/activate"), "1") },
        ],
    )
}

// ---------------------------------------------------------------- Bildschirm und Hintergrundlicht (nur Feststellung)

pub fn pick_display(p: &mut Profile) -> String {
    let has = |path: &'static str| move || fs::metadata(path).is_ok();
    p.choose(
        "display",
        &[
            Case { name: "drm", probe: &has("/dev/dri/card0") },
            Case { name: "fbdev", probe: &has("/dev/fb0") },
            Case { name: "fbdev_graphics", probe: &has("/dev/graphics/fb0") },
        ],
    )
}

/// Erstes Hintergrundlicht suchen; ist es aus (0), auf halbe Helligkeit stellen. Gemerkt wird der Name.
pub fn pick_backlight(p: &mut Profile) -> String {
    let on = |name: &str| -> bool {
        let b = format!("/sys/class/backlight/{name}");
        if fs::metadata(format!("{b}/brightness")).is_err() {
            return false;
        }
        let cur: u32 = read(&format!("{b}/brightness")).parse().unwrap_or(0);
        let max: u32 = read(&format!("{b}/max_brightness")).parse().unwrap_or(255);
        if cur == 0 {
            w(&format!("{b}/brightness"), &(max / 2).max(1).to_string());
        }
        w(&format!("{b}/bl_power"), "0");
        true
    };
    if let Some(prev) = p.map.get("backlight").cloned() {
        if prev != "none" && on(&prev) {
            crate::log(&format!("Geräteschalter: backlight = {prev} (gemerkt)"));
            return prev;
        }
    }
    let mut names: Vec<String> = fs::read_dir("/sys/class/backlight").map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    let found = names.into_iter().find(|n| on(n)).unwrap_or_else(|| "none".into());
    crate::log(&format!("Geräteschalter: backlight = {found} ({})", if found == "none" { "kein Fall passt" } else { "gefunden" }));
    p.set("backlight", &found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_text_roundtrip() {
        let mut p = Profile { device: "Test | 1.0".into(), ..Default::default() };
        p.set("usb", "configfs");
        p.set("vibrator", "none");
        let t = p.text().parse::<toml::Table>().unwrap();
        assert_eq!(t["device"].as_str(), Some("Test | 1.0"));
        assert_eq!(t["usb"].as_str(), Some("configfs"));
    }

    #[test]
    fn choose_prefers_remembered_then_searches() {
        let calls = std::cell::RefCell::new(Vec::new());
        let mut p = Profile::default();
        let a = || {
            calls.borrow_mut().push("a");
            false
        };
        let b = || {
            calls.borrow_mut().push("b");
            true
        };
        let c = || {
            calls.borrow_mut().push("c");
            true
        };
        let cases = [Case { name: "a", probe: &a }, Case { name: "b", probe: &b }, Case { name: "c", probe: &c }];
        // Erster Start: der Reihe nach, erster Treffer gilt
        assert_eq!(p.choose("f", &cases), "b");
        assert_eq!(*calls.borrow(), vec!["a", "b"]);
        // Nächster Start: nur der gemerkte Fall
        calls.borrow_mut().clear();
        assert_eq!(p.choose("f", &cases), "b");
        assert_eq!(*calls.borrow(), vec!["b"]);
        // Gemerkter Fall versagt: neu suchen und berichtigen
        p.map.insert("f".into(), "a".into());
        calls.borrow_mut().clear();
        assert_eq!(p.choose("f", &cases), "b");
        assert_eq!(*calls.borrow(), vec!["a", "a", "b"]);
    }

    #[test]
    fn resolve_source_candidates() {
        assert_eq!(resolve_source("tmpfs").as_deref(), Some("tmpfs"));
        assert_eq!(resolve_source("/dev/gibt-es-nicht|/dev/null").as_deref(), Some("/dev/null"));
        assert_eq!(resolve_source("/dev/gibt-es-nicht"), None);
    }
}
