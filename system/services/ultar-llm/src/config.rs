//! Einstellungen. Reihenfolge (spätere gewinnen): Gerätevorgaben → /etc/ultar/llm.toml →
//! /data/ultar/llm.toml → ~/.config/ultar/llm.toml (Desktop) → Umgebung ULTAR_LLM_MODEL / ULTAR_LLM_LISTEN.

use crate::platform::{Kind, Profile};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    /// Modelldatei (.gguf) oder leer = automatisch aus `model_dirs` wählen. "mock:" = Test-Engine.
    pub model: String,
    pub model_dirs: Vec<String>,
    /// TCP-Adresse der OpenAI-kompatiblen Schnittstelle, leer = aus. Nur Loopback, außer `allow_remote`.
    pub listen: String,
    /// Unix-Socket (gleiches HTTP-Protokoll), leer = aus.
    pub socket: String,
    pub allow_remote: bool,
    /// Pflicht-Schlüssel (Authorization: Bearer …) für TCP; leer = keiner. Der Unix-Socket schützt über Dateirechte.
    pub api_key: String,
    /// Kontextlänge in Token, 0 = nach Gerät
    pub ctx: u32,
    /// Threads fürs Erzeugen / Einlesen, 0 = nach Gerät
    pub threads: i32,
    pub threads_batch: i32,
    pub batch: u32,
    /// Schichten auf der Grafikkarte (nur mit Feature gpu-*)
    pub gpu_layers: u32,
    /// KV-Cache als Q8_0 (halber Speicher, minimal ungenauer)
    pub kv_q8: bool,
    /// Nur auf den schnellen Kernen rechnen
    pub big_cores_only: bool,
    /// Modell nach so vielen Sekunden ohne Anfrage entladen, 0 = nie
    pub idle_unload_secs: u64,
    /// Modell gleich beim Start laden
    pub preload: bool,
    /// Unter diesem Akkustand (und ohne Ladegerät) keine Anfragen, 0 = aus
    pub min_battery: u8,
    /// Über dieser CPU-Temperatur (°C) keine neuen Anfragen, 0 = aus
    pub max_temp_c: f32,
    /// Bei so viel Speicherdruck (PSI avg10 %) im Leerlauf entladen, 0 = aus
    pub unload_pressure: f32,
    /// So viel Speicher muss nach dem Laden frei bleiben (MB)
    pub mem_reserve_mb: u64,
    pub default_max_tokens: u32,
    /// Prozesspriorität (nice), damit die Oberfläche flüssig bleibt
    pub nice: i32,
}

impl Config {
    pub fn defaults(p: &Profile) -> Self {
        let mobile = p.kind == Kind::Mobile;
        let mut dirs = vec![
            "/data/ultar/models".to_string(),
            "/data/ultar/agent".to_string(),
        ];
        if !mobile {
            if let Some(h) = std::env::var_os("HOME") {
                dirs.push(format!("{}/.local/share/ultar/models", h.to_string_lossy()));
            }
            dirs.push("/var/lib/ultar/models".into());
        }
        Self {
            model: String::new(),
            model_dirs: dirs,
            listen: "127.0.0.1:8080".into(),
            socket: if Path::new("/run").is_dir() && unsafe { libc::geteuid() } == 0 {
                "/run/ultar/llm.sock".into()
            } else {
                runtime_socket()
            },
            allow_remote: false,
            api_key: String::new(),
            ctx: 0,
            threads: 0,
            threads_batch: 0,
            batch: if mobile { 256 } else { 512 },
            gpu_layers: if cfg!(any(feature = "gpu-vulkan", feature = "gpu-cuda")) {
                999
            } else {
                0
            },
            kv_q8: mobile,
            big_cores_only: false,
            idle_unload_secs: if mobile { 300 } else { 1800 },
            preload: false,
            min_battery: if mobile { 10 } else { 0 },
            max_temp_c: if mobile { 75.0 } else { 0.0 },
            unload_pressure: if mobile { 10.0 } else { 40.0 },
            mem_reserve_mb: if mobile { 500 } else { 1024 },
            default_max_tokens: if mobile { 512 } else { 2048 },
            nice: if mobile { 5 } else { 0 },
        }
    }

    /// Alle Konfigurationsdateien der Reihe nach einlesen.
    pub fn load(p: &Profile, explicit: Option<&Path>) -> Result<Self, String> {
        let mut v = toml::Value::try_from(Self::defaults(p)).map_err(|e| e.to_string())?;
        let mut files: Vec<PathBuf> =
            vec!["/etc/ultar/llm.toml".into(), "/data/ultar/llm.toml".into()];
        if let Some(c) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        {
            files.push(c.join("ultar/llm.toml"));
        }
        if let Some(e) = explicit {
            files.push(e.to_path_buf());
        }
        for f in files {
            let Ok(s) = std::fs::read_to_string(&f) else {
                if explicit.map(|e| e == f).unwrap_or(false) {
                    return Err(format!("{}: nicht lesbar", f.display()));
                }
                continue;
            };
            let over: toml::Value =
                toml::from_str(&s).map_err(|e| format!("{}: {e}", f.display()))?;
            merge(&mut v, over);
        }
        let mut c: Config = v.try_into().map_err(|e| e.to_string())?;
        if let Ok(m) = std::env::var("ULTAR_LLM_MODEL") {
            c.model = m;
        }
        if let Ok(l) = std::env::var("ULTAR_LLM_LISTEN") {
            c.listen = l;
        }
        if let Ok(s) = std::env::var("ULTAR_LLM_SOCKET") {
            c.socket = s;
        }
        Ok(c)
    }

    /// Fehlende Werte nach Gerät auflösen.
    pub fn resolved(mut self, p: &Profile) -> Self {
        let (t, tb) = p.auto_threads();
        if self.threads <= 0 {
            self.threads = if self.big_cores_only {
                p.big_cpus.len() as i32
            } else {
                t
            };
        }
        if self.threads_batch <= 0 {
            self.threads_batch = if self.big_cores_only {
                p.big_cpus.len() as i32
            } else {
                tb
            };
        }
        if self.ctx == 0 {
            self.ctx = p.auto_ctx();
        }
        self.batch = self.batch.clamp(32, 4096);
        self
    }
}

fn runtime_socket() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => format!("{d}/ultar-llm.sock"),
        _ => format!("/tmp/ultar-llm-{}.sock", unsafe { libc::geteuid() }),
    }
}

fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) => merge(bv, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

/// Gefundene Modelldatei mit Größe.
#[derive(Clone, Debug, Serialize)]
pub struct ModelFile {
    pub id: String,
    pub path: String,
    pub bytes: u64,
}

/// Alle .gguf-Dateien in den Modellordnern und im Ordner, den die Person gewählt hat
/// (`assistant.model_place`, siehe [`crate::place`]); Teil-Dateien "-00002-of-…" nur über die erste.
pub fn list_models(c: &Config) -> Vec<ModelFile> {
    let mut out: Vec<ModelFile> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |path: PathBuf| {
        let Ok(canon) = path.canonicalize() else {
            return;
        };
        if !seen.insert(canon.clone()) {
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.ends_with(".gguf")
            || name.contains("-of-") && !name.contains("-00001-of-")
            || name.starts_with("mmproj")
        {
            return;
        }
        let bytes = std::fs::metadata(&canon).map(|m| m.len()).unwrap_or(0);
        let mut id = name.trim_end_matches(".gguf").to_string();
        if id == "model" {
            // /data/ultar/agent/model.gguf – Name aus dem Ziel einer Verknüpfung, sonst „local"
            id = canon
                .file_name()
                .map(|n| n.to_string_lossy().trim_end_matches(".gguf").to_string())
                .filter(|n| n != "model")
                .unwrap_or_else(|| "local".into());
        }
        out.push(ModelFile {
            id,
            path: path.to_string_lossy().into_owned(),
            bytes,
        });
    };
    if !c.model.is_empty() && !c.model.starts_with("mock:") {
        add(PathBuf::from(&c.model));
    }
    let place = crate::place::read(Path::new(crate::place::PLACE_FILE), &crate::place::ROOTS);
    for d in c.model_dirs.iter().map(PathBuf::from).chain(place) {
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            v.sort();
            for p in v {
                add(p);
            }
        }
    }
    out
}

/// Standardmodell: ausdrücklich gesetzt, sonst model.gguf, sonst das größte, das in den Speicher passt.
pub fn pick_model(c: &Config, mem_total: u64) -> Option<ModelFile> {
    if c.model.starts_with("mock:") {
        return Some(ModelFile {
            id: "mock".into(),
            path: c.model.clone(),
            bytes: 0,
        });
    }
    let all = list_models(c);
    if !c.model.is_empty() {
        return all.into_iter().find(|m| m.path == c.model);
    }
    if let Some(m) = all.iter().find(|m| m.path.ends_with("/model.gguf")) {
        return Some(m.clone());
    }
    let budget = (mem_total as f64 * 0.55) as u64;
    let mut fit: Vec<&ModelFile> = all.iter().filter(|m| m.bytes <= budget).collect();
    fit.sort_by_key(|m| m.bytes);
    fit.last()
        .map(|m| (*m).clone())
        .or_else(|| all.iter().min_by_key(|m| m.bytes).cloned())
}
