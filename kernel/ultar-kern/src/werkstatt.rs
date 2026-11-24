//! Treiber-Werkstatt, Kern-Seite: erkennt Geräte ohne Treiber und beschreibt sie so, dass das Werkzeug
//! `tools/treiber-werkstatt` daraus einen Treiber erzeugen kann (Bauplan). Ausgabe als Zeilen
//! `WERKSTATT {json}` auf der seriellen Schnittstelle bzw. in der Kern-Konsole.

use crate::driver::DEVICES;
use alloc::format;
use alloc::string::String;

/// Welche offene Norm beschreibt dieses Gerät? (Grundlage für die Erzeugung)
pub fn norm(class: u8, sub: u8, pif: u8, vendor: u16) -> (&'static str, &'static str) {
    match (class, sub, pif) {
        (0x01, 0x06, 0x01) => ("ahci", "Serial ATA AHCI 1.3.1 (Intel)"),
        (0x01, 0x08, 0x02) => ("nvme", "NVM Express Base 2.0"),
        (0x0c, 0x03, 0x30) => ("xhci", "eXtensible Host Controller Interface 1.2 (Intel)"),
        (0x0c, 0x03, 0x20) => ("ehci", "Enhanced Host Controller Interface 1.0"),
        (0x04, 0x03, _) => ("hda", "High Definition Audio 1.0a (Intel)"),
        (0x03, _, _) if vendor == 0x8086 => ("intel-xe-anzeige", "Intel Graphics PRM (öffentlich): Display Engine – Pipes, Planes, Transcoder, DDI (eDP/HDMI/DP), GTT"),
        (0x03, _, _) if vendor == 0x1002 => ("amd-dcn-anzeige", "AMD Display Core Next – Registerdoku (teilweise öffentlich)"),
        (0x03, 0x00, 0x00) => ("vga-fb", "VGA/Bochs-Bildspeicher (VBE-DISPI) bzw. GOP-Übernahme"),
        (0x02, 0x00, _) if vendor == 0x8086 => ("intel-eth", "Intel-Ethernet-Datenblatt (e1000/e1000e/igc, öffentlich)"),
        (0x02, 0x00, _) if vendor == 0x10ec => ("realtek-eth", "Realtek RTL8111/8168-Datenblatt"),
        (0x02, 0x80, _) => ("wlan", "Herstellerspezifisch (Intel iwlwifi, MediaTek, Realtek) – Firmware nötig"),
        (0x0c, 0x05, _) => ("smbus", "SMBus 3.0 / Intel PCH-Datenblatt"),
        (0x06, _, _) => ("bruecke", "PCI-Brücke – kein eigener Treiber nötig"),
        _ if vendor == 0x1af4 => ("virtio", "virtio 1.2 (OASIS)"),
        _ => ("unbekannt", "keine offene Norm bekannt – Registerbeschreibung/Datenblatt nötig"),
    }
}

/// Geräte ohne Treiber (ohne Brücken/Host-Brücken, die keinen brauchen)
pub fn missing() -> alloc::vec::Vec<String> {
    let devs = DEVICES.lock();
    let mut out = alloc::vec::Vec::new();
    for b in devs.iter().filter(|b| b.driver.is_none()) {
        let d = &b.dev;
        let (kind, spec) = norm(d.class, d.subclass, d.prog_if, d.vendor);
        if kind == "bruecke" {
            continue;
        }
        let bars: String = d
            .bars
            .iter()
            .enumerate()
            .filter_map(|(i, x)| x.map(|b| format!("{{\"bar\":{i},\"adresse\":\"{:#x}\",\"groesse\":{},\"io\":{}}}", b.addr, b.size, b.io)))
            .collect::<alloc::vec::Vec<_>>()
            .join(",");
        let caps: String = d.caps().iter().map(|(_, id)| format!("{id}")).collect::<alloc::vec::Vec<_>>().join(",");
        out.push(format!(
            "{{\"pci\":\"{:02x}:{:02x}.{}\",\"hersteller\":\"{:04x}\",\"geraet\":\"{:04x}\",\"subsystem\":\"{:04x}:{:04x}\",\"klasse\":\"{:02x}{:02x}{:02x}\",\"rev\":{},\"art\":\"{}\",\"bauplan\":\"{kind}\",\"norm\":\"{spec}\",\"bars\":[{bars}],\"faehigkeiten\":[{caps}],\"fehler\":{}}}",
            d.bus, d.dev, d.func, d.vendor, d.device, d.subsys_vendor, d.subsys, d.class, d.subclass, d.prog_if, d.rev,
            d.class_name(),
            b.error.as_ref().map(|e| format!("\"{e}\"")).unwrap_or_else(|| "null".into())
        ));
    }
    out
}

pub fn report() {
    let m = missing();
    if m.is_empty() {
        crate::kprintln!("[werkstatt] alle Geräte haben einen Treiber");
        return;
    }
    crate::kprintln!("[werkstatt] {} Gerät(e) ohne Treiber – Baupläne:", m.len());
    for j in m {
        crate::kprintln!("WERKSTATT {j}");
    }
}
