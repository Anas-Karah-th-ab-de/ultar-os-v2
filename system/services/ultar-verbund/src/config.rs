//! Einstellungen und gekoppelte Geräte (verbund.json im Datenordner).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Rolle {
    /// steuern und rechnen
    #[serde(rename = "voll")]
    Voll,
    /// nur Aufgaben rechnen
    #[serde(rename = "rechnen")]
    Rechnen,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Partner {
    pub kennung: String,
    pub pk: String,
    pub name: String,
    pub rolle: Rolle,
    /// woher: "code" (gekoppelt) oder "konto" (gleiches Ultar-Konto)
    #[serde(default)]
    pub quelle: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Verbund an/aus (Wahl des Nutzers; Standard aus bis zur Einrichtung)
    pub aktiv: bool,
    pub name: String,
    /// Relais für Geräte außerhalb des eigenen Netzes
    pub relais: String,
    /// Sitzungstoken des Ultar-Kontos (für Relais und Kontokopplung)
    pub token: String,
    pub steuern_erlauben: bool,
    pub rechnen_erlauben: bool,
    /// Bildschirm teilen und fernbedienen lassen
    pub bildschirm_erlauben: bool,
    /// Mithören/Durchsage zulassen
    pub ton_erlauben: bool,
    /// Dateien annehmen (Weitergeben)
    pub dateien_erlauben: bool,
    /// höchstens so viele Aufgaben gleichzeitig (0 = alle Kerne)
    pub max_kerne: usize,
    pub tcp_port: u16,
    pub udp_port: u16,
    /// zusätzliche Ziele für die Meldung (z. B. in Netzen ohne Rundruf), "ip:port"
    pub ziele: Vec<String>,
    pub partner: Vec<Partner>,
}

impl Default for Config {
    fn default() -> Self {
        let name = std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "ultar".into());
        Config {
            aktiv: false,
            name,
            relais: "wss://ultar.code-craft.tech/v1/verbund/relais".into(),
            token: String::new(),
            steuern_erlauben: true,
            rechnen_erlauben: true,
            bildschirm_erlauben: true,
            ton_erlauben: true,
            dateien_erlauben: true,
            max_kerne: 0,
            tcp_port: crate::TCP_PORT,
            udp_port: crate::UDP_PORT,
            ziele: Vec::new(),
            partner: Vec::new(),
        }
    }
}

pub fn pfad(dir: &Path) -> PathBuf {
    dir.join("verbund.json")
}

impl Config {
    /// nur die Datei (zum Ändern und Speichern) – ohne Kern-Schalter und Umgebung
    pub fn datei(dir: &Path) -> Config {
        std::fs::read_to_string(pfad(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// wirksame Einstellungen: Datei + Kern-Schalter (/proc/ultar/verbund) + ULTAR_VERBUND_AKTIV.
    /// Nie so speichern – sonst würde ein vorübergehendes AUS des Kerns dauerhaft.
    pub fn laden(dir: &Path) -> Config {
        let mut c = Self::datei(dir);
        // auf dem Ultar-Kern entscheidet der Kern-Schalter mit
        if let Ok(s) = std::fs::read_to_string("/proc/ultar/verbund") {
            if s.starts_with("aus") {
                c.aktiv = false;
            }
        }
        if let Ok(v) = std::env::var("ULTAR_VERBUND_AKTIV") {
            c.aktiv = v == "1";
        }
        c
    }

    pub fn speichern(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("verbund.json.neu");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(tmp, pfad(dir))
    }

    pub fn partner(&self, pk: &str) -> Option<&Partner> {
        self.partner.iter().find(|p| p.pk == pk)
    }

    pub fn kerne(&self) -> usize {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if self.max_kerne == 0 {
            n
        } else {
            self.max_kerne.min(n.max(self.max_kerne))
        }
    }

    pub fn hinzufuegen(&mut self, p: Partner) {
        self.partner.retain(|x| x.pk != p.pk);
        self.partner.push(p);
    }
}
