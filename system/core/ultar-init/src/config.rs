//! Konfiguration: /etc/ultar/init.toml und /etc/ultar/services/*.toml

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const INIT_CONF: &str = "/etc/ultar/init.toml";
pub const SERVICE_DIR: &str = "/etc/ultar/services";
/// Dienste nachinstallierter Apps (auf der Datenpartition)
pub const USER_SERVICE_DIR: &str = "/data/ultar/services";

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InitConfig {
    #[serde(default)]
    pub hostname: Option<String>,
    /// Zusätzliche Mounts nach den Basis-Dateisystemen (z. B. Daten-/Compat-Partition).
    #[serde(default)]
    pub mounts: Vec<MountEntry>,
    /// Unkomprimierte Kernel-Module (.ko), die beim Start geladen werden.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Wartezeit (ms) zwischen SIGTERM und SIGKILL beim Herunterfahren.
    #[serde(default = "default_grace")]
    pub shutdown_grace_ms: u64,
}

fn default_grace() -> u64 {
    3000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountEntry {
    pub source: String,
    pub target: String,
    pub fstype: String,
    #[serde(default)]
    pub options: Option<String>,
    #[serde(default)]
    pub readonly: bool,
    /// Fehlschlag ignorieren (z. B. optionale SD-Karte).
    #[serde(default)]
    pub optional: bool,
    /// So lange (ms) auf die Quelle warten, falls sie noch nicht da ist (USB-Stick, NVMe-Modul lädt noch).
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    #[default]
    Always,
    OnFailure,
    Never,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default)]
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub restart: Restart,
    /// Dienste, die vorher laufen (bzw. bei oneshot erfolgreich beendet sein) müssen.
    #[serde(default)]
    pub after: Vec<String>,
    /// Einmal-Dienst: Abhängige starten erst nach erfolgreichem Ende.
    #[serde(default)]
    pub oneshot: bool,
    /// stdin/stdout/stderr an /dev/console statt Logdatei.
    #[serde(default)]
    pub console: bool,
    /// Im Ubuntu-Kompatibilitätsbereich (/compat) ausführen.
    #[serde(default)]
    pub compat: bool,
    #[serde(default)]
    pub workdir: Option<String>,
    /// Beim Start nicht automatisch starten (nur per ultar-ctl).
    #[serde(default)]
    pub manual: bool,
    /// Nur in diesen Ausgaben laden (`phone`, `desktop`, `server`); leer = in allen.
    #[serde(default)]
    pub editions: Vec<String>,
    /// cgroup v2 Limits, z. B. memory.max = "256M", cpu.weight = "50"
    #[serde(default)]
    pub cgroup: BTreeMap<String, String>,
}

pub fn load_init() -> InitConfig {
    match std::fs::read_to_string(INIT_CONF) {
        Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
            crate::log(&format!("init.toml fehlerhaft, nutze Standard: {e}"));
            InitConfig {
                shutdown_grace_ms: default_grace(),
                ..Default::default()
            }
        }),
        Err(_) => InitConfig {
            shutdown_grace_ms: default_grace(),
            ..Default::default()
        },
    }
}

/// Zusätzliche Modul-Liste (eine .ko-Datei pro Zeile), vom Image-Build erzeugt.
pub const MODULE_LIST: &str = "/etc/ultar/modules.list";

/// Kernel-Parameter `ultar.start=a,b`: diese (manuellen) Dienste beim Booten starten.
pub fn cmdline_start() -> Vec<String> {
    std::fs::read_to_string("/proc/cmdline")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|a| a.strip_prefix("ultar.start="))
        .flat_map(|v| v.split(',').map(String::from).collect::<Vec<_>>())
        .collect()
}

pub fn load_services() -> Vec<ServiceConfig> {
    let mut out = Vec::new();
    let mut paths: Vec<_> = [SERVICE_DIR, USER_SERVICE_DIR]
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .collect();
    paths.sort();
    let start = cmdline_start();
    let edition = ultar_hal_system::current_edition();
    for p in paths {
        if p.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match parse_service(&p) {
            Ok(mut s) => {
                // Dienste anderer Ausgaben gar nicht erst laden (z. B. Modem/RIL auf Server und PC):
                // spart Speicher, Startzeit und Angriffsfläche. Ausnahme: ausdrücklich per ultar.start= verlangt.
                if !edition.matches(&s.editions) && !start.contains(&s.name) {
                    continue;
                }
                if start.contains(&s.name) {
                    s.manual = false;
                }
                out.push(s)
            }
            Err(e) => crate::log(&format!("Dienst {} ignoriert: {e}", p.display())),
        }
    }
    out
}

fn parse_service(p: &Path) -> Result<ServiceConfig, String> {
    let s = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
    let mut c: ServiceConfig = toml::from_str(&s).map_err(|e| e.to_string())?;
    if c.name.is_empty() {
        c.name = p.file_stem().unwrap().to_string_lossy().into_owned();
    }
    Ok(c)
}
