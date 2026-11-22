//! Zellen-Profil (`<wurzel>/<name>/zelle.toml`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Speicher {
    /// Höchstgrenze des eigenen Ordners in GB (0 = dynamisch: wächst, solange die Platte Platz hat)
    pub max_gb: u64,
    /// Arbeitsspeicher: 0 = dynamisch (nimmt, was frei ist, gibt bei Bedarf ab), sonst feste Grenze in MB
    pub ram_mb: u64,
    /// garantierter Arbeitsspeicher in MB (wird anderen Zellen zuletzt weggenommen)
    pub ram_garantiert_mb: u64,
}

impl Default for Speicher {
    fn default() -> Self {
        Speicher {
            max_gb: 0,
            ram_mb: 0,
            ram_garantiert_mb: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Netz {
    /// "frei" | "aus" | "ueber" (über eine VPN-Zelle) | "wireguard" (eigener Tunnel in der Zelle)
    pub art: String,
    /// bei "ueber": Name der VPN-Zelle
    pub ueber: String,
    /// bei "wireguard": WireGuard-Konfiguration (wg setconf-Format) + Adresse
    pub wg_conf: String,
    pub wg_adresse: String,
    /// bei "wireguard": Endpunkt ip:port (einziges Ziel, das am Tunnel vorbei darf)
    pub wg_endpunkt: String,
    /// diese Zelle leitet anderer Zellen Verkehr weiter (VPN-Zelle); Ausgang nur über `vpn_schnittstelle`
    pub vpn_zelle: bool,
    pub vpn_schnittstelle: String,
}

impl Default for Netz {
    fn default() -> Self {
        Netz {
            art: "frei".into(),
            ueber: String::new(),
            wg_conf: String::new(),
            wg_adresse: String::new(),
            wg_endpunkt: String::new(),
            vpn_zelle: false,
            vpn_schnittstelle: "wg0".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Privat {
    pub standort: bool,
    pub geraeteinfo: bool,
    pub kamera: bool,
    pub mikrofon: bool,
    pub lautsprecher: bool,
    pub usb: bool,
    /// Rechnername in der Zelle
    pub rechnername: String,
    /// vorgegebener Ort „Breite,Länge“ (leer = keiner); die App sieht nur diesen
    pub ort: String,
    /// vorgegebene Zeitzone, z. B. „Europe/Berlin“ (leer = die des Geräts)
    pub zeitzone: String,
    /// vorgegebene Sprache/Locale, z. B. „ar“ oder „en_US.UTF-8“ (leer = die des Geräts)
    pub sprache: String,
}

impl Default for Privat {
    fn default() -> Self {
        Privat {
            standort: false,
            geraeteinfo: false,
            kamera: false,
            mikrofon: false,
            lautsprecher: true,
            usb: false,
            rechnername: "ultar".into(),
            ort: String::new(),
            zeitzone: String::new(),
            sprache: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Protokoll {
    /// jede Verbindung (Ziel, Name, Bytes hoch/runter, Dauer) und jede DNS-Frage
    pub verbindungen: bool,
    /// zusätzlich den Anfang unverschlüsselter Uploads (HTTP) festhalten
    pub uploads: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profil {
    pub name: String,
    /// installierte App (uapp-Kennung) – oder `befehl`
    pub app: String,
    pub befehl: Vec<String>,
    pub umgebung: Vec<(String, String)>,
    /// Wurzel des Dateisystems in der Zelle ("/" oder "/compat" für Ubuntu-/Windows-Apps)
    pub wurzel: String,
    pub speicher: Speicher,
    pub netz: Netz,
    pub privat: Privat,
    pub protokoll: Protokoll,
    /// "auto" | "alle" | "aus" | Nummer der Karte
    pub gpu: String,
    /// verknüpfte Zellen: gemeinsamer Ordner + gegenseitig im Netz erreichbar
    pub verknuepft: Vec<String>,
}

impl Default for Profil {
    fn default() -> Self {
        Profil {
            name: String::new(),
            app: String::new(),
            befehl: Vec::new(),
            umgebung: Vec::new(),
            wurzel: "/".into(),
            speicher: Speicher::default(),
            netz: Netz::default(),
            privat: Privat::default(),
            protokoll: Protokoll::default(),
            gpu: "auto".into(),
            verknuepft: Vec::new(),
        }
    }
}

pub fn ordner(name: &str) -> PathBuf {
    crate::wurzel().join(name)
}

pub fn gueltiger_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-')
}

impl Profil {
    pub fn laden(name: &str) -> Result<Profil, String> {
        let s = std::fs::read_to_string(ordner(name).join("zelle.toml"))
            .map_err(|_| format!("Zelle „{name}“ gibt es nicht"))?;
        toml::from_str(&s).map_err(|e| format!("zelle.toml: {e}"))
    }

    pub fn speichern(&self) -> Result<(), String> {
        if !gueltiger_name(&self.name) {
            return Err("Name: nur a-z, 0-9 und -, höchstens 32 Zeichen".into());
        }
        let d = ordner(&self.name);
        std::fs::create_dir_all(d.join("daten")).map_err(|e| e.to_string())?;
        std::fs::write(
            d.join("zelle.toml"),
            toml::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn alle() -> Vec<Profil> {
        let mut v: Vec<Profil> = std::fs::read_dir(crate::wurzel())
            .map(|d| {
                d.flatten()
                    .filter_map(|e| Profil::laden(&e.file_name().to_string_lossy()).ok())
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Benutzerkennung der Zelle (eigene, damit sie fremde Dateien nicht lesen kann)
    pub fn uid(&self) -> u32 {
        60_000 + crate::kurzhash(&self.name) % 5000
    }

    /// Belegung des eigenen Ordners in Bytes
    pub fn belegt(&self) -> u64 {
        fn groesse(p: &std::path::Path) -> u64 {
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => std::fs::read_dir(p)
                    .map(|d| d.flatten().map(|e| groesse(&e.path())).sum())
                    .unwrap_or(0),
                Ok(m) => {
                    use std::os::unix::fs::MetadataExt;
                    m.blocks() * 512
                }
                Err(_) => 0,
            }
        }
        groesse(&ordner(&self.name).join("daten"))
    }
}
