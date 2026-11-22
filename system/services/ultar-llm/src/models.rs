//! Modellverwaltung: Katalog geprüfter Modelle, Empfehlung nach Gerät (Speicher, Prozessor, Grafik),
//! Herunterladen von **Hugging Face** (GGUF-Datei + optional Bild-Projektor `mmproj`) oder aus der
//! **Ollama**-Bibliothek (Registry, Blob = GGUF). Alles reines Rust (ureq + rustls), fortsetzbar (Range),
//! mit SHA-256-Prüfung, wo die Quelle sie liefert. Kein Python, kein Ubuntu-Bereich.

use crate::platform::{Kind, Profile};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Ein Modell im Katalog
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    /// Wofür: Gespräch/Werkzeuge bzw. Bilder
    pub role: &'static str,
    pub vision: bool,
    /// Hugging Face: Repo und Datei (GGUF), Projektor-Datei für Bilder
    pub hf_repo: &'static str,
    pub hf_file: &'static str,
    pub hf_mmproj: &'static str,
    /// Ollama-Bibliothek, z. B. "qwen3:1.7b" (leer = nicht dort)
    pub ollama: &'static str,
    /// Größe der Modelldatei (ungefähr, MB) und benötigter Arbeitsspeicher insgesamt (MB)
    pub size_mb: u64,
    pub ram_mb: u64,
    pub langs: &'static str,
    pub note: &'static str,
}

/// Geprüfte Modelle (Dateinamen am 22.11.2026 auf Hugging Face/Ollama nachgesehen).
/// Qwen3: sehr gut bei Werkzeugaufrufen (JSON) und mehrsprachig (Arabisch/Deutsch/Englisch).
/// Bilder: Projektor (mmproj) aus ggml-org, passend zu llama.cpp-mtmd.
pub const CATALOG: &[Entry] = &[
    Entry {
        id: "qwen3-0.6b",
        name: "Qwen3 0.6B",
        role: "chat",
        vision: false,
        hf_repo: "unsloth/Qwen3-0.6B-GGUF",
        hf_file: "Qwen3-0.6B-Q4_K_M.gguf",
        hf_mmproj: "",
        ollama: "qwen3:0.6b",
        size_mb: 400,
        ram_mb: 900,
        langs: "ar en de +100",
        note: "fast, simple commands",
    },
    Entry {
        id: "qwen3-1.7b",
        name: "Qwen3 1.7B",
        role: "chat",
        vision: false,
        hf_repo: "unsloth/Qwen3-1.7B-GGUF",
        hf_file: "Qwen3-1.7B-Q4_K_M.gguf",
        hf_mmproj: "",
        ollama: "qwen3:1.7b",
        size_mb: 1100,
        ram_mb: 1800,
        langs: "ar en de +100",
        note: "good tool use on phones",
    },
    Entry {
        id: "qwen3-4b",
        name: "Qwen3 4B",
        role: "chat",
        vision: false,
        hf_repo: "unsloth/Qwen3-4B-GGUF",
        hf_file: "Qwen3-4B-Q4_K_M.gguf",
        hf_mmproj: "",
        ollama: "qwen3:4b",
        size_mb: 2500,
        ram_mb: 3600,
        langs: "ar en de +100",
        note: "smart, phones with 8 GB+",
    },
    Entry {
        id: "qwen3-8b",
        name: "Qwen3 8B",
        role: "chat",
        vision: false,
        hf_repo: "unsloth/Qwen3-8B-GGUF",
        hf_file: "Qwen3-8B-Q4_K_M.gguf",
        hf_mmproj: "",
        ollama: "qwen3:8b",
        size_mb: 5000,
        ram_mb: 6500,
        langs: "ar en de +100",
        note: "laptop/PC, 16 GB phones",
    },
    Entry {
        id: "qwen3-30b-a3b",
        name: "Qwen3 30B-A3B (MoE)",
        role: "chat",
        vision: false,
        hf_repo: "unsloth/Qwen3-30B-A3B-GGUF",
        hf_file: "Qwen3-30B-A3B-Q4_K_M.gguf",
        hf_mmproj: "",
        ollama: "qwen3:30b",
        size_mb: 18600,
        ram_mb: 22000,
        langs: "ar en de +100",
        note: "PC/server 32 GB+, fast for its size (3B active)",
    },
    Entry {
        id: "smolvlm2-500m",
        name: "SmolVLM2 500M (vision)",
        role: "vision",
        vision: true,
        hf_repo: "ggml-org/SmolVLM2-500M-Video-Instruct-GGUF",
        hf_file: "SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
        hf_mmproj: "mmproj-SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
        ollama: "",
        size_mb: 540,
        ram_mb: 1100,
        langs: "en",
        note: "sees screen/camera on small phones (English answers)",
    },
    Entry {
        id: "qwen2.5-vl-3b",
        name: "Qwen2.5-VL 3B (vision)",
        role: "vision",
        vision: true,
        hf_repo: "ggml-org/Qwen2.5-VL-3B-Instruct-GGUF",
        hf_file: "Qwen2.5-VL-3B-Instruct-Q4_K_M.gguf",
        hf_mmproj: "mmproj-Qwen2.5-VL-3B-Instruct-Q8_0.gguf",
        ollama: "",
        size_mb: 2700,
        ram_mb: 4200,
        langs: "ar en de +",
        note: "reads screens, documents, Arabic text in images",
    },
    Entry {
        id: "gemma3-4b",
        name: "Gemma 3 4B (vision)",
        role: "vision",
        vision: true,
        hf_repo: "ggml-org/gemma-3-4b-it-GGUF",
        hf_file: "gemma-3-4b-it-Q4_K_M.gguf",
        hf_mmproj: "mmproj-model-f16.gguf",
        ollama: "",
        size_mb: 3300,
        ram_mb: 5000,
        langs: "ar en de +140",
        note: "strong multilingual vision",
    },
    Entry {
        id: "qwen2.5-vl-7b",
        name: "Qwen2.5-VL 7B (vision)",
        role: "vision",
        vision: true,
        hf_repo: "ggml-org/Qwen2.5-VL-7B-Instruct-GGUF",
        hf_file: "Qwen2.5-VL-7B-Instruct-Q4_K_M.gguf",
        hf_mmproj: "mmproj-Qwen2.5-VL-7B-Instruct-Q8_0.gguf",
        ollama: "",
        size_mb: 5500,
        ram_mb: 8000,
        langs: "ar en de +",
        note: "PC: best screen/document understanding",
    },
];

pub fn find(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.id.eq_ignore_ascii_case(id))
}

/// Grafik für Modelle: NVIDIA (CUDA) / Vulkan-fähige Karte vorhanden?
pub fn gpu() -> Option<String> {
    if Path::new("/dev/nvidia0").exists() {
        return Some("nvidia".into());
    }
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("card") && !n.contains('-') {
                let vendor =
                    std::fs::read_to_string(e.path().join("device/vendor")).unwrap_or_default();
                match vendor.trim() {
                    "0x1002" => return Some("amd".into()),
                    "0x8086" => return Some("intel".into()),
                    "0x10de" => return Some("nvidia".into()),
                    _ => {}
                }
            }
        }
    }
    None
}

#[derive(Clone, Debug, Serialize)]
pub struct Recommendation {
    /// Modell für Gespräch und Werkzeuge
    pub chat: &'static str,
    /// Modell für Bilder (Bildschirm, Kamera, mitgebrachte Fotos); None = Gerät zu schwach
    pub vision: Option<&'static str>,
    /// Begründung (englisch, die Oberfläche übersetzt Schlüsselwörter)
    pub why: String,
    pub mem_gb: f64,
    pub cpu: String,
    pub gpu: Option<String>,
}

/// Empfehlung nach Gerät. Grundsatz: Modell + Kontext + Oberfläche müssen gleichzeitig in den Speicher passen
/// (Modell höchstens ~45 % des Arbeitsspeichers), auf Handys ohne dotprod (z. B. A30, ARMv8.0) eine Stufe kleiner.
pub fn recommend(p: &Profile) -> Recommendation {
    let gb = p.mem_total as f64 / 1e9;
    let slow_arm = p.arch == "aarch64" && !p.simd.contains("dotprod");
    let g = gpu();
    let budget_mb =
        (p.mem_total / 1_000_000) as f64 * if p.kind == Kind::Mobile { 0.45 } else { 0.55 };
    let mut chat = "qwen3-0.6b";
    for id in ["qwen3-1.7b", "qwen3-4b", "qwen3-8b", "qwen3-30b-a3b"] {
        let e = find(id).unwrap();
        if (e.ram_mb as f64) <= budget_mb {
            chat = id;
        }
    }
    // 30B-A3B nur auf Rechnern mit viel Speicher (MoE: schnell, aber groß); Handys bleiben bis 8B
    if p.kind == Kind::Mobile && chat == "qwen3-30b-a3b" {
        chat = "qwen3-8b";
    }
    let mut why = format!("{gb:.1} GB RAM");
    if slow_arm {
        // ohne dotprod rechnen Q4-Modelle deutlich langsamer → eine Stufe kleiner für flüssige Antworten
        chat = match chat {
            "qwen3-4b" | "qwen3-8b" => "qwen3-1.7b",
            c => c,
        };
        why.push_str(", ARMv8.0 without dotprod (slower) → smaller model for fluent answers");
    }
    // Bild-Modell: läuft nur bei Bedarf (wird geladen, wenn ein Bild kommt)
    let vision = if budget_mb >= 7800.0 {
        Some("qwen2.5-vl-7b")
    } else if budget_mb >= 4100.0 && !slow_arm {
        Some("qwen2.5-vl-3b")
    } else if budget_mb >= 1000.0 {
        Some("smolvlm2-500m")
    } else {
        None
    };
    if let Some(gp) = &g {
        why.push_str(&format!(", GPU {gp}"));
    }
    why.push_str(&format!(", {} cores", p.cpus));
    Recommendation {
        chat,
        vision,
        why,
        mem_gb: (gb * 10.0).round() / 10.0,
        cpu: format!("{} {}", p.arch, p.simd),
        gpu: g,
    }
}

// ------------------------------------------------------------------ Herunterladen

/// Was geladen werden soll
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Hugging Face: Repo, Datei, optional Projektor
    Hf {
        repo: String,
        file: String,
        mmproj: Option<String>,
    },
    /// Ollama-Bibliothek: Name, Variante (z. B. "qwen3", "1.7b")
    Ollama { name: String, tag: String },
}

/// Angabe auflösen: Katalog-ID ("qwen3-1.7b", mit Quelle "…@ollama"), "hf:repo/datei.gguf[+mmproj.gguf]",
/// "ollama:name:tag", oder eine volle Hugging-Face-Adresse (…/resolve/main/…).
pub fn parse_spec(spec: &str) -> Result<(String, Source), String> {
    let s = spec.trim();
    if let Some(r) = s.strip_prefix("ollama:") {
        let (n, t) = r.split_once(':').unwrap_or((r, "latest"));
        let id = format!("{}-{}", n.replace('/', "-"), t);
        return Ok((
            id,
            Source::Ollama {
                name: n.into(),
                tag: t.into(),
            },
        ));
    }
    if let Some(r) = s.strip_prefix("hf:") {
        let (main, mm) = match r.split_once('+') {
            Some((a, b)) => (a, Some(b.to_string())),
            None => (r, None),
        };
        let parts: Vec<&str> = main.splitn(3, '/').collect();
        if parts.len() != 3 || !parts[2].ends_with(".gguf") {
            return Err("hf:<user>/<repo>/<file>.gguf[+<mmproj>.gguf]".into());
        }
        let id = parts[2].trim_end_matches(".gguf").to_string();
        return Ok((
            id,
            Source::Hf {
                repo: format!("{}/{}", parts[0], parts[1]),
                file: parts[2].into(),
                mmproj: mm,
            },
        ));
    }
    if s.starts_with("https://huggingface.co/") && s.contains("/resolve/") {
        let rest = s.trim_start_matches("https://huggingface.co/");
        let (repo, file) = rest.split_once("/resolve/").ok_or("bad URL")?;
        let file = file
            .split_once('/')
            .map(|(_, f)| f)
            .unwrap_or(file)
            .split('?')
            .next()
            .unwrap_or("");
        let id = file
            .rsplit('/')
            .next()
            .unwrap_or(file)
            .trim_end_matches(".gguf")
            .to_string();
        return Ok((
            id,
            Source::Hf {
                repo: repo.into(),
                file: file.into(),
                mmproj: None,
            },
        ));
    }
    let (id, via_ollama) = match s.split_once('@') {
        Some((a, "ollama")) => (a, true),
        Some((a, _)) => (a, false),
        None => (s, false),
    };
    let e = find(id).ok_or_else(|| format!("unknown model {id} – see `ultar-llm catalog`"))?;
    if via_ollama {
        if e.ollama.is_empty() {
            return Err(format!(
                "{} is not in the Ollama library – use Hugging Face",
                e.name
            ));
        }
        let (n, t) = e.ollama.split_once(':').unwrap_or((e.ollama, "latest"));
        return Ok((
            e.id.into(),
            Source::Ollama {
                name: n.into(),
                tag: t.into(),
            },
        ));
    }
    Ok((
        e.id.into(),
        Source::Hf {
            repo: e.hf_repo.into(),
            file: e.hf_file.into(),
            mmproj: (!e.hf_mmproj.is_empty()).then(|| e.hf_mmproj.to_string()),
        },
    ))
}

fn hf_base() -> String {
    std::env::var("ULTAR_HF_BASE").unwrap_or_else(|_| "https://huggingface.co".into())
}
fn ollama_base() -> String {
    std::env::var("ULTAR_OLLAMA_BASE").unwrap_or_else(|_| "https://registry.ollama.ai".into())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .redirects(8)
        .user_agent("ultar-llm (Ultar OS, KM Code Craft)")
        .build()
}

/// Fortschritt: (Datei, geladen, gesamt)
pub type Progress<'a> = &'a mut dyn FnMut(&str, u64, u64);

/// Eine Datei laden (fortsetzbar über `<ziel>.part`), optional SHA-256 prüfen.
fn fetch(
    url: &str,
    dest: &Path,
    sha256: Option<&str>,
    label: &str,
    prog: Progress,
) -> Result<u64, String> {
    let part = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let a = agent();
    let mut req = a.get(url);
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(416, _)) if have > 0 => {
            // schon vollständig
            std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
            return Ok(have);
        }
        Err(ureq::Error::Status(c, r)) => {
            return Err(format!("{label}: HTTP {c} {}", r.status_text()))
        }
        Err(e) => return Err(format!("{label}: {e}")),
    };
    if resp.status() == 200 && have > 0 {
        // Server kann nicht fortsetzen → neu beginnen
        have = 0;
    }
    let len = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let total = if len > 0 { have + len } else { 0 };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(have > 0)
        .write(true)
        .truncate(have == 0)
        .open(&part)
        .map_err(|e| format!("{}: {e}", part.display()))?;
    let mut r = resp.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = have;
    let mut last = Instant::now();
    prog(label, done, total);
    loop {
        let n = r
            .read(&mut buf)
            .map_err(|e| format!("{label}: {e} (download can be resumed)"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])
            .map_err(|e| format!("{}: {e}", part.display()))?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(400) {
            prog(label, done, total);
            last = Instant::now();
        }
    }
    f.sync_all().ok();
    drop(f);
    if total > 0 && done < total {
        return Err(format!(
            "{label}: incomplete ({done}/{total}), run again to resume"
        ));
    }
    if let Some(want) = sha256 {
        prog(&format!("{label} (SHA-256)"), done, total);
        let mut h = Sha256::new();
        let mut rf = std::fs::File::open(&part).map_err(|e| e.to_string())?;
        loop {
            let n = rf.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(want.trim_start_matches("sha256:")) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{label}: checksum mismatch – file deleted"));
        }
    }
    // GGUF-Kennung prüfen
    let mut magic = [0u8; 4];
    if let Ok(mut rf) = std::fs::File::open(&part) {
        let _ = rf.read_exact(&mut magic);
    }
    if &magic != b"GGUF" {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{label}: not a GGUF model file"));
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
    prog(label, done, done.max(total));
    Ok(done)
}

/// SHA-256 einer Hugging-Face-LFS-Datei (Kopfzeile X-Linked-Etag bzw. ETag), falls verfügbar
fn hf_sha(url: &str) -> Option<String> {
    let r = agent().head(url).call().ok()?;
    for k in ["x-linked-etag", "etag"] {
        if let Some(v) = r.header(k) {
            let v = v
                .trim_matches('"')
                .trim_start_matches("W/")
                .trim_matches('"')
                .to_string();
            if v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(v);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct Pulled {
    pub id: String,
    pub model: String,
    pub mmproj: Option<String>,
    pub bytes: u64,
}

/// Modell laden nach `dir`. Name im Ordner: `<id>.gguf` und `mmproj-<id>.gguf` (so findet der Dienst beide).
pub fn pull(spec: &str, dir: &str, prog: Progress) -> Result<Pulled, String> {
    let (id, src) = parse_spec(spec)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    let model = PathBuf::from(dir).join(format!("{id}.gguf"));
    let mm_path = PathBuf::from(dir).join(format!("mmproj-{id}.gguf"));
    // genug Platz?
    let free = free_space(dir);
    match src {
        Source::Hf { repo, file, mmproj } => {
            let url = format!("{}/{repo}/resolve/main/{file}", hf_base());
            if let Some(e) = find(&id) {
                if free > 0 && free < e.size_mb * 1_100_000 {
                    return Err(format!(
                        "not enough storage: {} MB free, {} MB needed",
                        free >> 20,
                        e.size_mb
                    ));
                }
            }
            let sha = hf_sha(&url);
            let mut bytes = 0;
            if !model.exists() {
                bytes += fetch(&url, &model, sha.as_deref(), &file, prog)?;
            }
            let mut mm = None;
            if let Some(m) = mmproj {
                let murl = format!("{}/{repo}/resolve/main/{m}", hf_base());
                if !mm_path.exists() {
                    let msha = hf_sha(&murl);
                    bytes += fetch(&murl, &mm_path, msha.as_deref(), &m, prog)?;
                }
                mm = Some(mm_path.to_string_lossy().into_owned());
            }
            Ok(Pulled {
                id,
                model: model.to_string_lossy().into_owned(),
                mmproj: mm,
                bytes,
            })
        }
        Source::Ollama { name, tag } => {
            let lib = if name.contains('/') {
                name.clone()
            } else {
                format!("library/{name}")
            };
            let murl = format!("{}/v2/{lib}/manifests/{tag}", ollama_base());
            let man: serde_json::Value = agent()
                .get(&murl)
                .set(
                    "Accept",
                    "application/vnd.docker.distribution.manifest.v2+json",
                )
                .call()
                .map_err(|e| format!("Ollama manifest {name}:{tag}: {e}"))?
                .into_json()
                .map_err(|e| format!("Ollama manifest: {e}"))?;
            let layers = man["layers"].as_array().cloned().unwrap_or_default();
            let pick = |t: &str| {
                layers
                    .iter()
                    .find(|l| l["mediaType"].as_str() == Some(t))
                    .and_then(|l| {
                        Some((
                            l["digest"].as_str()?.to_string(),
                            l["size"].as_u64().unwrap_or(0),
                        ))
                    })
            };
            let (dg, size) =
                pick("application/vnd.ollama.image.model").ok_or("Ollama: no model layer")?;
            if free > 0 && free < size + size / 10 {
                return Err(format!(
                    "not enough storage: {} MB free, {} MB needed",
                    free >> 20,
                    size >> 20
                ));
            }
            let mut bytes = 0;
            if !model.exists() {
                bytes += fetch(
                    &format!("{}/v2/{lib}/blobs/{dg}", ollama_base()),
                    &model,
                    Some(&dg),
                    &format!("{name}:{tag}"),
                    prog,
                )?;
            }
            let mut mm = None;
            if let Some((pd, _)) = pick("application/vnd.ollama.image.projector") {
                if !mm_path.exists() {
                    bytes += fetch(
                        &format!("{}/v2/{lib}/blobs/{pd}", ollama_base()),
                        &mm_path,
                        Some(&pd),
                        &format!("{name}:{tag} projector"),
                        prog,
                    )?;
                }
                mm = Some(mm_path.to_string_lossy().into_owned());
            }
            Ok(Pulled {
                id,
                model: model.to_string_lossy().into_owned(),
                mmproj: mm,
                bytes,
            })
        }
    }
}

/// Freier Platz im Dateisystem (Bytes), 0 = unbekannt
pub fn free_space(dir: &str) -> u64 {
    let Ok(c) = std::ffi::CString::new(dir) else {
        return 0;
    };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return 0;
    }
    s.f_bavail as u64 * s.f_frsize as u64
}

/// Projektor (mmproj) zu einer Modelldatei finden: `mmproj-<id>.gguf`, `<id>.mmproj.gguf`, sonst ein
/// `mmproj*.gguf` im selben Ordner, dessen Name den Modellnamen (ohne Quantisierung) enthält.
pub fn mmproj_for(model_path: &str) -> Option<String> {
    let p = Path::new(model_path);
    let dir = p.parent()?;
    let stem = p.file_stem()?.to_string_lossy().into_owned();
    for c in [format!("mmproj-{stem}.gguf"), format!("{stem}.mmproj.gguf")] {
        let q = dir.join(&c);
        if q.exists() {
            return Some(q.to_string_lossy().into_owned());
        }
    }
    let base = stem
        .split(['-', '.'])
        .take(2)
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_ascii_lowercase();
        if n.starts_with("mmproj") && n.ends_with(".gguf") && n.contains(&base) {
            return Some(e.path().to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Kind;

    fn prof(gb: f64, kind: Kind, arch: &'static str, simd: &str) -> Profile {
        Profile {
            kind,
            arch,
            cpus: 8,
            cores: 8,
            big_cpus: vec![],
            simd: simd.into(),
            mem_total: (gb * 1e9) as u64,
        }
    }

    #[test]
    fn empfehlung() {
        // Galaxy A30 (3–4 GB, ARMv8.0 ohne dotprod)
        let r = recommend(&prof(3.7, Kind::Mobile, "aarch64", "neon"));
        assert_eq!(r.chat, "qwen3-0.6b");
        assert_eq!(r.vision, Some("smolvlm2-500m"));
        assert_eq!(
            recommend(&prof(1.5, Kind::Mobile, "aarch64", "neon")).vision,
            None
        );
        let r = recommend(&prof(4.0, Kind::Mobile, "aarch64", "neon"));
        assert_eq!(r.chat, "qwen3-1.7b");
        // OnePlus 15 (16 GB, mit dotprod/i8mm)
        let r = recommend(&prof(16.0, Kind::Mobile, "aarch64", "neon dotprod i8mm"));
        assert_eq!(r.chat, "qwen3-8b");
        assert_eq!(r.vision, Some("qwen2.5-vl-3b"));
        // PC mit 32 GB
        let r = recommend(&prof(32.0, Kind::Desktop, "x86_64", "avx2 fma"));
        assert_eq!(r.chat, "qwen3-8b");
        assert_eq!(r.vision, Some("qwen2.5-vl-7b"));
        let r = recommend(&prof(64.0, Kind::Desktop, "x86_64", "avx2 fma"));
        assert_eq!(r.chat, "qwen3-30b-a3b");
    }

    #[test]
    fn angaben() {
        assert_eq!(
            parse_spec("qwen3-1.7b").unwrap().1,
            Source::Hf {
                repo: "unsloth/Qwen3-1.7B-GGUF".into(),
                file: "Qwen3-1.7B-Q4_K_M.gguf".into(),
                mmproj: None
            }
        );
        assert_eq!(
            parse_spec("qwen3-1.7b@ollama").unwrap().1,
            Source::Ollama {
                name: "qwen3".into(),
                tag: "1.7b".into()
            }
        );
        assert!(parse_spec("gemma3-4b@ollama").is_err());
        let (id, s) = parse_spec(
            "hf:ggml-org/gemma-3-4b-it-GGUF/gemma-3-4b-it-Q4_K_M.gguf+mmproj-model-f16.gguf",
        )
        .unwrap();
        assert_eq!(id, "gemma-3-4b-it-Q4_K_M");
        assert!(matches!(
            s,
            Source::Hf {
                mmproj: Some(_),
                ..
            }
        ));
        let (id, _) = parse_spec("https://huggingface.co/unsloth/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf?download=true").unwrap();
        assert_eq!(id, "Qwen3-4B-Q4_K_M");
        assert_eq!(parse_spec("ollama:llama3.2:3b").unwrap().0, "llama3.2-3b");
    }

    /// Hugging Face und Ollama gegen einen lokalen Nachbau: Laden, Fortsetzen, Prüfsumme, Projektor
    #[test]
    fn herunterladen() {
        use std::io::BufRead;
        use std::net::TcpListener;
        let model: Vec<u8> = [b"GGUF".as_slice(), &vec![7u8; 300_000]].concat();
        let proj: Vec<u8> = [b"GGUF".as_slice(), &vec![9u8; 1000]].concat();
        let dg = |d: &[u8]| -> String {
            Sha256::digest(d)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        let (md, pd) = (dg(&model), dg(&proj));
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (m2, p2, md2, pd2) = (model.clone(), proj.clone(), md.clone(), pd.clone());
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let head = line.starts_with("HEAD");
                let mut range = 0usize;
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                    }
                }
                let (body, extra): (Vec<u8>, String) = if path.ends_with("/manifests/1.7b") {
                    (format!(r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{md2}","size":{}}},{{"mediaType":"application/vnd.ollama.image.projector","digest":"sha256:{pd2}","size":{}}}]}}"#, m2.len(), p2.len()).into_bytes(), String::new())
                } else if path.contains(&md2) || path.ends_with("Qwen3-1.7B-Q4_K_M.gguf") {
                    (m2.clone(), format!("X-Linked-Etag: \"{md2}\"\r\n"))
                } else if path.contains(&pd2) {
                    (p2.clone(), String::new())
                } else {
                    let mut w = s;
                    let _ = w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let mut w = s;
                let part = &body[range.min(body.len())..];
                let status = if range > 0 {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let _ = write!(
                    w,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    part.len()
                );
                if !head {
                    let _ = w.write_all(part);
                }
            }
        });
        std::env::set_var("ULTAR_HF_BASE", format!("http://127.0.0.1:{port}"));
        std::env::set_var("ULTAR_OLLAMA_BASE", format!("http://127.0.0.1:{port}"));
        let dir = std::env::temp_dir().join(format!("ultar-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = dir.to_string_lossy().into_owned();
        // Hugging Face, mit angefangenem Teil (fortsetzen)
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("qwen3-1.7b.gguf.part"), &model[..1000]).unwrap();
        let mut seen = 0u64;
        let r = pull("qwen3-1.7b", &d, &mut |_, done, _| seen = seen.max(done)).unwrap();
        assert_eq!(std::fs::read(&r.model).unwrap(), model);
        assert_eq!(r.bytes, model.len() as u64);
        // Ollama mit Projektor (Prüfsumme = Blob-Name)
        let r = pull("ollama:qwen3:1.7b", &d, &mut |_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(&r.model).unwrap(), model);
        assert_eq!(std::fs::read(r.mmproj.unwrap()).unwrap(), proj);
        assert_eq!(
            mmproj_for(&r.model).as_deref(),
            Some(dir.join("mmproj-qwen3-1.7b.gguf").to_str().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
