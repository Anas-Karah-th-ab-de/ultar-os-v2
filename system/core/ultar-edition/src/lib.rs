//! Ultar OS gibt es in drei Ausgaben, gebaut aus demselben Kern:
//!
//! | Ausgabe   | Geräte                         | Oberfläche                                   |
//! |-----------|--------------------------------|----------------------------------------------|
//! | `phone`   | Handys, Tablets (A30, OP15 …)  | ultar-shell (Touch)                          |
//! | `desktop` | Laptops, PCs (x86_64, arm64)   | ultar-shell + Fenstersystem ultar-wm         |
//! | `server`  | Server, Mini-PCs, VMs          | keine; Web-Verwaltung ultar-admin, SSH       |
//!
//! Reihenfolge: Kernel-Parameter `ultar.edition=` > Umgebung `ULTAR_EDITION` > Datei `/etc/ultar/edition`
//! (vom Image-Bau geschrieben) > **Erkennung der Geräteart** > `phone`.
//!
//! Erkennung: Ultar-Kern `/proc/ultar/geraet` (SMBIOS-Gehäuse, ACPI) · Linux `/sys/class/dmi/id/chassis_type`
//! (PC/Laptop → desktop, Server → server, Tablet/Handgerät → phone) · ARM mit Gerätebaum und Akku → phone.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edition {
    Phone,
    Desktop,
    Server,
}

pub const FILE: &str = "/etc/ultar/edition";

impl Edition {
    pub fn parse(s: &str) -> Option<Edition> {
        match s.trim().to_ascii_lowercase().as_str() {
            "phone" | "handy" | "mobile" | "tablet" => Some(Edition::Phone),
            "desktop" | "laptop" | "pc" => Some(Edition::Desktop),
            "server" => Some(Edition::Server),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Edition::Phone => "phone",
            Edition::Desktop => "desktop",
            Edition::Server => "server",
        }
    }
    /// Anzeigename je Sprache (de/en/ar)
    pub fn label(self, lang: &str) -> &'static str {
        match (self, lang) {
            (Edition::Phone, "ar") => "إصدار الهاتف",
            (Edition::Desktop, "ar") => "إصدار الحاسوب",
            (Edition::Server, "ar") => "إصدار الخادم",
            (Edition::Phone, "de") => "Handy-Ausgabe",
            (Edition::Desktop, "de") => "Desktop-/Laptop-Ausgabe",
            (Edition::Server, "de") => "Server-Ausgabe",
            (Edition::Phone, _) => "Phone edition",
            (Edition::Desktop, _) => "Desktop/laptop edition",
            (Edition::Server, _) => "Server edition",
        }
    }
    /// Hat diese Ausgabe eine Oberfläche auf dem Bildschirm?
    pub fn has_ui(self) -> bool {
        self != Edition::Server
    }
    /// Passt ein Eintrag `editions = [...]` eines Dienstes? Leere Liste = alle Ausgaben.
    pub fn matches(self, list: &[String]) -> bool {
        list.is_empty() || list.iter().any(|e| e == "all" || Edition::parse(e) == Some(self))
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// SMBIOS-Gehäusetyp (DMTF SMBIOS Typ 3) → Ausgabe; None = nichtssagend
pub fn from_chassis(t: u8) -> Option<Edition> {
    match t & 0x7f {
        3 | 4 | 5 | 6 | 7 | 13 | 15 | 16 | 24 | 34 | 35 | 36 => Some(Edition::Desktop),
        8 | 9 | 10 | 14 | 31 | 32 => Some(Edition::Desktop),
        17 | 23 | 25 | 28 | 29 => Some(Edition::Server),
        30 | 11 => Some(Edition::Phone),
        _ => None,
    }
}

/// Geräteart erkennen (ohne Einstellung): Ultar-Kern-Angabe, sonst DMI (x86), sonst Gerätebaum + Akku (Handy)
pub fn erkennen(kern: Option<&str>, chassis: Option<&str>, devicetree: bool, akku: bool) -> Option<Edition> {
    if let Some(e) = kern.and_then(|k| k.lines().find_map(|l| l.trim().strip_prefix("ausgabe=")).and_then(Edition::parse)) {
        return Some(e);
    }
    if let Some(e) = chassis.and_then(|c| c.trim().parse::<u8>().ok()).and_then(from_chassis) {
        return Some(e);
    }
    if devicetree && akku {
        return Some(Edition::Phone);
    }
    None
}

/// Ausgabe aus den gegebenen Quellen (testbar ohne echtes System).
pub fn resolve(cmdline: &str, env: Option<&str>, file: Option<&str>) -> Edition {
    resolve_auto(cmdline, env, file, None)
}

/// wie `resolve`, mit erkannter Geräteart als Rückfall vor `phone`
pub fn resolve_auto(cmdline: &str, env: Option<&str>, file: Option<&str>, auto: Option<Edition>) -> Edition {
    if let Some(e) = cmdline.split_whitespace().filter_map(|a| a.strip_prefix("ultar.edition=")).filter_map(Edition::parse).last() {
        return e;
    }
    if let Some(e) = env.and_then(Edition::parse) {
        return e;
    }
    file.and_then(|f| f.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).and_then(Edition::parse))
        .or(auto)
        .unwrap_or(Edition::Phone)
}

/// Ausgabe des laufenden Systems (einmal bestimmt, danach aus dem Zwischenspeicher).
pub fn current() -> Edition {
    static CUR: std::sync::OnceLock<Edition> = std::sync::OnceLock::new();
    *CUR.get_or_init(detect)
}

/// Ausgabe neu bestimmen (ohne Zwischenspeicher).
pub fn detect() -> Edition {
    let cmd = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let env = std::env::var("ULTAR_EDITION").ok();
    let file = std::fs::read_to_string(std::env::var("ULTAR_EDITION_FILE").unwrap_or_else(|_| FILE.into())).ok();
    let kern = std::fs::read_to_string("/proc/ultar/geraet").ok();
    let chassis = std::fs::read_to_string("/sys/class/dmi/id/chassis_type").ok();
    let dt = std::path::Path::new("/proc/device-tree/compatible").exists();
    let akku = std::fs::read_dir("/sys/class/power_supply")
        .map(|d| d.flatten().any(|e| std::fs::read_to_string(e.path().join("type")).map(|t| t.trim() == "Battery").unwrap_or(false)))
        .unwrap_or(false);
    let auto = erkennen(kern.as_deref(), chassis.as_deref(), dt, akku);
    resolve_auto(&cmd, env.as_deref(), file.as_deref(), auto)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order() {
        assert_eq!(resolve("", None, None), Edition::Phone);
        assert_eq!(resolve("", None, Some("# x\nserver\n")), Edition::Server);
        assert_eq!(resolve("", Some("desktop"), Some("server")), Edition::Desktop);
        assert_eq!(resolve("quiet ultar.edition=laptop", Some("server"), Some("phone")), Edition::Desktop);
        assert_eq!(resolve("ultar.edition=bogus", None, Some("server")), Edition::Server);
    }
    #[test]
    fn erkennung() {
        // server3 (Beelink GTi15 Ultra): Mini-PC
        assert_eq!(erkennen(None, Some("35\n"), false, false), Some(Edition::Desktop));
        assert_eq!(erkennen(None, Some("10"), false, true), Some(Edition::Desktop));
        assert_eq!(erkennen(None, Some("23"), false, false), Some(Edition::Server));
        assert_eq!(erkennen(None, Some("30"), false, true), Some(Edition::Phone));
        assert_eq!(erkennen(None, Some("2"), false, false), None);
        // Handy (A30): Gerätebaum + Akku
        assert_eq!(erkennen(None, None, true, true), Some(Edition::Phone));
        assert_eq!(erkennen(None, None, true, false), None);
        // Ultar-Kern meldet selbst
        assert_eq!(erkennen(Some("art=server\nausgabe=server\n"), Some("35"), false, false), Some(Edition::Server));
        // Einstellung schlägt Erkennung
        assert_eq!(resolve_auto("", None, Some("phone"), Some(Edition::Desktop)), Edition::Phone);
        assert_eq!(resolve_auto("", None, None, Some(Edition::Desktop)), Edition::Desktop);
        assert_eq!(resolve_auto("", None, None, None), Edition::Phone);
    }
    #[test]
    fn matching() {
        assert!(Edition::Server.matches(&[]));
        assert!(Edition::Server.matches(&["desktop".into(), "server".into()]));
        assert!(!Edition::Phone.matches(&["desktop".into(), "server".into()]));
        assert!(Edition::Phone.matches(&["all".into()]));
    }
}
