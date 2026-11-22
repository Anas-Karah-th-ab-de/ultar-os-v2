//! Datenschutz/Sicherheit – die zentrale Kontrolle des Nutzers über die Fähigkeiten des Geräts.
//!
//! Ein Hauptschalter (alles aus) plus einzelne Gruppen: Netzwerk, Kamera, Mikrofon, Standort,
//! Sensoren, Bluetooth. Jedes Subsystem fragt `allowed(&set, Cap::…)`, bevor es Hardware nutzt.
//! `apply(&set)` setzt den Zustand aktiv durch (Schnittstellen aus, rfkill) und schreibt den
//! Status nach /run/ultar/privacy, damit auch die Hintergrunddienste (ultar-net) ihn lesen.

use crate::sys::Settings;
use std::io::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Network,
    Camera,
    Microphone,
    Location,
    Sensors,
    Bluetooth,
}

impl Cap {
    /// Kurzschlüssel für die Status-Datei.
    pub fn key(self) -> &'static str {
        match self {
            Cap::Network => "net",
            Cap::Camera => "cam",
            Cap::Microphone => "mic",
            Cap::Location => "loc",
            Cap::Sensors => "sensors",
            Cap::Bluetooth => "bt",
        }
    }
}

/// Darf eine Fähigkeit gerade genutzt werden? Hauptschalter UND Einzelschalter müssen an sein.
pub fn allowed(set: &Settings, cap: Cap) -> bool {
    if !set.priv_all {
        return false;
    }
    match cap {
        Cap::Network => set.priv_net,
        Cap::Camera => set.priv_cam,
        Cap::Microphone => set.priv_mic,
        Cap::Location => set.priv_loc,
        Cap::Sensors => set.priv_sensors,
        Cap::Bluetooth => set.priv_bt,
    }
}

/// Anzahl der gerade gesperrten Gruppen (für die Anzeige).
pub fn blocked_count(set: &Settings) -> usize {
    [Cap::Network, Cap::Camera, Cap::Microphone, Cap::Location, Cap::Sensors, Cap::Bluetooth]
        .iter()
        .filter(|c| !allowed(set, **c))
        .count()
}

/// Zustand aktiv durchsetzen: Netz-Schnittstellen aus/an, Bluetooth per rfkill, Status schreiben.
/// Kamera/Mikro/Sensoren/Standort werden an der Quelle über `allowed()` geprüft (kein Dienst nötig).
pub fn apply(set: &Settings) {
    // Status-Datei für Dienste
    let _ = std::fs::create_dir_all("/run/ultar");
    if let Ok(mut f) = std::fs::File::create("/run/ultar/privacy") {
        let line = format!(
            "all={}\nnet={}\ncam={}\nmic={}\nloc={}\nsensors={}\nbt={}\n",
            b(set.priv_all), b(allowed(set, Cap::Network)), b(allowed(set, Cap::Camera)),
            b(allowed(set, Cap::Microphone)), b(allowed(set, Cap::Location)),
            b(allowed(set, Cap::Sensors)), b(allowed(set, Cap::Bluetooth)),
        );
        let _ = f.write_all(line.as_bytes());
    }
    // Netzwerk aktiv durchsetzen: alle echten Schnittstellen hoch/runter
    let net_ok = allowed(set, Cap::Network);
    for ifc in net_ifaces() {
        set_iface_up(&ifc, net_ok);
    }
    // Bluetooth per rfkill
    let bt_ok = allowed(set, Cap::Bluetooth);
    rfkill_set("bluetooth", bt_ok);
    // WLAN zusätzlich per rfkill (falls Netz komplett aus, auch Funk aus)
    rfkill_set("wlan", net_ok);
}

fn b(v: bool) -> u8 {
    if v { 1 } else { 0 }
}

fn net_ifaces() -> Vec<String> {
    std::fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n != "lo")
                .collect()
        })
        .unwrap_or_default()
}

/// Schnittstelle hoch/runter per ioctl (ohne Fremdprogramme).
fn set_iface_up(name: &str, up: bool) {
    use nix::libc;
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return;
        }
        let mut r: libc::ifreq = std::mem::zeroed();
        for (i, c) in name.bytes().take(libc::IFNAMSIZ - 1).enumerate() {
            r.ifr_name[i] = c as libc::c_char;
        }
        if libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut r) >= 0 {
            if up {
                r.ifr_ifru.ifru_flags |= libc::IFF_UP as i16;
            } else {
                r.ifr_ifru.ifru_flags &= !(libc::IFF_UP as i16);
            }
            libc::ioctl(fd, libc::SIOCSIFFLAGS, &r);
        }
        libc::close(fd);
    }
}

/// rfkill: alle Blöcke eines Typs (wlan/bluetooth) soft-blocken/entsperren über /sys/class/rfkill.
fn rfkill_set(kind: &str, enabled: bool) {
    let want_soft = if enabled { "0" } else { "1" };
    if let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") {
        for e in rd.flatten() {
            let p = e.path();
            let ty = std::fs::read_to_string(p.join("type")).unwrap_or_default();
            if ty.trim() == kind {
                let _ = std::fs::write(p.join("soft"), want_soft);
            }
        }
    }
}
