//! Schnelleinstellungen (Wisch von oben): Funk-Schalter best-effort über rfkill/ip und sysfs.
//! WLAN/Bluetooth/Flugmodus wirken über rfkill; mobile Daten über rmnet (sobald Modem aktiv);
//! Taschenlampe über die Kamera-LED (/sys/class/leds/*torch*|*flash*).

fn rfkill_blocked(kind: &str) -> bool {
    let key: &'static str = match kind {
        "wlan" => "rfk_wlan",
        "bluetooth" => "rfk_bt",
        _ => return rfkill_blocked_raw(kind),
    };
    let k = kind.to_string();
    crate::sys::memo(key, 800, move || rfkill_blocked_raw(&k))
}
fn rfkill_blocked_raw(kind: &str) -> bool {
    if let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") {
        for e in rd.flatten() {
            let p = e.path();
            if std::fs::read_to_string(p.join("type"))
                .unwrap_or_default()
                .trim()
                == kind
            {
                return std::fs::read_to_string(p.join("soft"))
                    .map(|s| s.trim() == "1")
                    .unwrap_or(false);
            }
        }
    }
    false
}

fn rfkill_set(kind: &str, on: bool) {
    let soft = if on { "0" } else { "1" };
    if let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") {
        for e in rd.flatten() {
            let p = e.path();
            if kind == "any"
                || std::fs::read_to_string(p.join("type"))
                    .unwrap_or_default()
                    .trim()
                    == kind
            {
                let _ = std::fs::write(p.join("soft"), soft);
            }
        }
    }
}

// ---- WLAN ----
pub fn wifi_on() -> bool {
    !rfkill_blocked("wlan")
}
pub fn set_wifi(on: bool) {
    crate::sys::memo_clear();
    // ohne Fremdprogramm („ip“ fehlt auf dem Gerät): rfkill + Schnittstelle per ioctl.
    // wpa_supplicant (von ultar-net betrieben) verbindet nach dem Einschalten selbst wieder.
    rfkill_set("wlan", on);
    if let Some(ifc) = crate::sys::wifi_iface() {
        crate::sys::iface_up(&ifc, on);
    }
}

// ---- Bluetooth ----
/// Geräte ohne rfkill (A30): Schalter als Datei, ausgewertet vom Bluetooth-Bildschirm/-Dienst.
const BT_FLAG: &str = "/data/ultar/bt.on";
fn has_rfkill(kind: &str) -> bool {
    let key: &'static str = match kind {
        "wlan" => "has_wlan",
        "bluetooth" => "has_bt",
        _ => return has_rfkill_raw(kind),
    };
    let k = kind.to_string();
    crate::sys::memo(key, 10_000, move || has_rfkill_raw(&k))
}
fn has_rfkill_raw(kind: &str) -> bool {
    std::fs::read_dir("/sys/class/rfkill")
        .map(|rd| {
            rd.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("type"))
                    .unwrap_or_default()
                    .trim()
                    == kind
            })
        })
        .unwrap_or(false)
}
pub fn bt_on() -> bool {
    crate::sys::memo("q_bt_on", 800, bt_on_raw)
}
fn bt_on_raw() -> bool {
    if has_rfkill("bluetooth") {
        !rfkill_blocked("bluetooth")
    } else {
        std::path::Path::new(BT_FLAG).exists()
    }
}
pub fn set_bt(on: bool) {
    crate::sys::memo_clear();
    rfkill_set("bluetooth", on);
    if !has_rfkill("bluetooth") {
        if on {
            let _ = std::fs::write(BT_FLAG, "1");
        } else {
            let _ = std::fs::remove_file(BT_FLAG);
        }
    }
}

// ---- Flugmodus ----
/// Flugmodus-Schalter als Datei: der Funkdienst (ultar-ril) schaltet danach den Mobilfunk aus/ein.
const AIRPLANE_FLAG: &str = "/data/ultar/airplane.on";
pub fn airplane_on() -> bool {
    crate::sys::memo("q_airplane_on", 800, airplane_on_raw)
}
fn airplane_on_raw() -> bool {
    std::path::Path::new(AIRPLANE_FLAG).exists()
        || (has_rfkill("wlan") && rfkill_blocked("wlan") && rfkill_blocked("bluetooth"))
}
pub fn set_airplane(on: bool) {
    crate::sys::memo_clear();
    // an = alle Funkmodule aus (rfkill, wo vorhanden; sonst WLAN-Schnittstelle + Schalterdateien)
    rfkill_set("any", !on);
    if on {
        let _ = std::fs::write(AIRPLANE_FLAG, "1");
    } else {
        let _ = std::fs::remove_file(AIRPLANE_FLAG);
    }
    if !has_rfkill("wlan") {
        if let Some(ifc) = crate::sys::wifi_iface() {
            crate::sys::iface_up(&ifc, !on);
        }
    }
    if on {
        set_bt(false);
    }
    set_mobile(!on);
}

// ---- Mobile Daten ---- (Flag + rmnet; greift sobald Modem/RIL aktiv)
pub fn mobile_on() -> bool {
    crate::sys::memo("q_mobile_on", 800, mobile_on_raw)
}
fn mobile_on_raw() -> bool {
    std::path::Path::new("/run/ultar/mobiledata.on").exists()
}
pub fn set_mobile(on: bool) {
    crate::sys::memo_clear();
    let _ = std::fs::create_dir_all("/run/ultar");
    if on {
        let _ = std::fs::write("/run/ultar/mobiledata.on", "1");
    } else {
        let _ = std::fs::remove_file("/run/ultar/mobiledata.on");
    }
    for ifc in ["rmnet0", "rmnet_data0"] {
        if std::path::Path::new(&format!("/sys/class/net/{ifc}")).exists() {
            crate::sys::iface_up(ifc, on);
        }
    }
}

// ---- Taschenlampe ----
// Samsung (A30): /sys/class/camera/flash/rear_flash (1 = an, 0 = aus). Der LED-Knoten
// /sys/class/leds/fled-s2mu106 darf NICHT beschrieben werden – der Stock-Kern stürzt dabei ab.
const SAMSUNG_TORCH: &str = "/sys/class/camera/flash/rear_flash";
const TORCH_FLAG: &str = "/run/ultar/torch.on";
fn torch_node() -> Option<std::path::PathBuf> {
    let rd = std::fs::read_dir("/sys/class/leds").ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.contains("s2mu") {
            continue;
        }
        if n.contains("torch") || n.contains("flash") {
            return Some(e.path());
        }
    }
    None
}
pub fn torch_on() -> bool {
    crate::sys::memo("q_torch_on", 800, torch_on_raw)
}
fn torch_on_raw() -> bool {
    if std::path::Path::new(SAMSUNG_TORCH).exists() {
        return std::path::Path::new(TORCH_FLAG).exists();
    }
    torch_node()
        .and_then(|p| std::fs::read_to_string(p.join("brightness")).ok())
        .map(|s| s.trim().parse::<i32>().unwrap_or(0) > 0)
        .unwrap_or(false)
}
pub fn set_torch(on: bool) {
    crate::sys::memo_clear();
    if std::path::Path::new(SAMSUNG_TORCH).exists() {
        if std::fs::write(SAMSUNG_TORCH, if on { "1" } else { "0" }).is_ok() {
            if on {
                let _ = std::fs::create_dir_all("/run/ultar");
                let _ = std::fs::write(TORCH_FLAG, "1");
            } else {
                let _ = std::fs::remove_file(TORCH_FLAG);
            }
        }
        return;
    }
    if let Some(p) = torch_node() {
        let max = std::fs::read_to_string(p.join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(1);
        let _ = std::fs::write(
            p.join("brightness"),
            if on { max.to_string() } else { "0".into() },
        );
    }
}
