//! Lokaler Agent: steuert das Gerät über Werkzeuge, angetrieben von einem lokalen Sprachmodell.
//!
//! Das Modell läuft getrennt im Dienst `ultar-llm` (Rust-Kern, llama.cpp statisch eingebunden; früher
//! llama-server im Ubuntu-Bereich) und wird über die
//! OpenAI-kompatible Schnittstelle /v1/chat/completions auf dem Gerät selbst angesprochen –
//! keine Cloud. Das Modell antwortet mit JSON: entweder ein Werkzeugaufruf oder eine Antwort.
//! Eingriffe mit Risiko (Befehle, Pakete, Dienste, Ausschalten) brauchen die Zustimmung des Nutzers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct AgentCfg {
    pub enabled: bool,
    /// Adresse des Modell-Servers als IP:Port
    pub endpoint: String,
    /// Vor riskanten Aktionen nachfragen
    pub confirm: bool,
    /// "local" = Sprachmodell auf dem Gerät, "claude" = Claude über die Anthropic-API (braucht Internet)
    pub backend: String,
    pub api_key: String,
    pub claude_model: String,
}

impl Default for AgentCfg {
    fn default() -> Self {
        Self { enabled: false, endpoint: "127.0.0.1:8080".into(), confirm: true, backend: "local".into(), api_key: String::new(), claude_model: "claude-haiku-5-5".into() }
    }
}

pub const MODEL_DIR: &str = "/data/ultar/agent";

/// Ordner, in denen der Dienst ultar-llm Modelle sucht (wie dessen Vorgabe `model_dirs`).
pub const MODEL_DIRS: [&str; 2] = ["/data/ultar/models", MODEL_DIR];

fn any_gguf() -> bool {
    MODEL_DIRS.iter().any(|d| std::fs::read_dir(d).map(|r| r.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".gguf") && !e.file_name().to_string_lossy().starts_with("mmproj"))).unwrap_or(false))
}

/// Sprachmodell nutzbar: nativer Dienst ultar-llm + irgendein .gguf, oder der alte Weg (llama-server im Ubuntu-Bereich).
pub fn model_installed() -> bool {
    let native = std::path::Path::new("/usr/bin/ultar-llm").exists() && any_gguf();
    native || std::path::Path::new(&format!("{MODEL_DIR}/llama-server")).exists() && std::path::Path::new(&format!("{MODEL_DIR}/model.gguf")).exists()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub tool: String,
    pub args: Value,
}

impl Call {
    pub fn arg(&self, k: &str) -> String {
        match self.args.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }

    /// Aktionen, die Daten oder den Betrieb verändern können.
    pub fn risky(&self) -> bool {
        self.risk().is_some()
    }

    /// Art des Eingriffs (für die Rückfrage), None = ungefährlich (nur lesen/anzeigen).
    /// Root-Befehle, Sicherheit, Kamera, Löschen/Schreiben außerhalb der Apps fragen IMMER nach – auch wenn die
    /// Rückfrage für einfache Aktionen ausgeschaltet ist (siehe `always_confirm`).
    pub fn risk(&self) -> Option<&'static str> {
        Some(match self.tool.as_str() {
            "run_command" | "kill_process" => "Root command",
            "install_package" | "remove_package" => "Package change",
            "power" => "Power",
            "call" => "Phone call",
            "set_privacy" | "set_security" => "Security change",
            "write_file" | "delete_path" => "Changes files",
            "look_at_user" => "Camera",
            "model_download" => "Download",
            "service" if self.arg("action") != "start" => "Service change",
            // vorhandene Datei überschreiben = Daten können verloren gehen
            "doc_create" if std::path::Path::new(&self.arg("path")).exists() => "Changes files",
            "read_any" | "list_dir" if secret_path(&self.arg("path")) => "Private data",
            _ => return None,
        })
    }

    /// Diese Eingriffe brauchen immer die Zustimmung des Nutzers (Root, Sicherheit, Kamera, Dateien, Geheimnisse).
    pub fn always_confirm(&self) -> bool {
        matches!(self.risk(), Some("Root command" | "Security change" | "Camera" | "Changes files" | "Private data" | "Package change" | "Power" | "Download"))
    }

    pub fn describe(&self) -> String {
        let a: Vec<String> = self.args.as_object().map(|o| o.iter().map(|(k, v)| format!("{k}={}", v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))).collect()).unwrap_or_default();
        format!("{}({})", self.tool, a.join(", "))
    }
}

/// Geheime Orte: Schlüssel, Kennwörter, Konten, WLAN-Zugänge, Kennwortdateien des Systems
pub fn secret_path(p: &str) -> bool {
    let p = p.trim();
    let low = p.to_ascii_lowercase();
    p.contains("..")
        || p == "/data/ultar"
        || p.starts_with("/data/ultar/") && !p.starts_with("/data/ultar/help")
        || low.starts_with("/etc/shadow")
        || low.starts_with("/etc/gshadow")
        || low.starts_with("/etc/ultar/keys")
        || low.starts_with("/etc/ultar/trusted-keys") && low.ends_with(".key")
        || low.contains("/.ssh/")
        || low.contains("wpa_supplicant")
        || low.ends_with(".key")
        || low.ends_with(".pem")
        || low.contains("/proc/") && (low.ends_with("/environ") || low.ends_with("/mem"))
}

pub enum Reply {
    Call(Call),
    Answer(String),
}

pub const SYSTEM: &str = r#"You are KM, the system assistant of Ultar OS (KM Code Craft, developed by Mohamad Anas Kara Mohamad) – like Jarvis: you run on the device itself, see the whole system and operate it with tools for the user.
RULES: Act ONLY on what the user asks in this conversation – never on your own initiative, never more than asked. Answer in the user's language (Arabic, English or German), short and friendly.
Root commands, security changes, camera, deleting/writing files, packages and power always show the user a confirmation first – just call the tool; never try to work around a denial. Never reveal passwords, keys or PINs.
Reply with ONE JSON object only, no other text.
To use a tool: {"tool":"NAME","args":{...}}
To answer the user: {"answer":"TEXT"}
SEE:
- see_screen {} : text of what the user is looking at (app, buttons, values)
- look_screen {"question":"..."} : you get the screenshot as an image (needs a vision model)
- look_at_user {} : take a front-camera photo of the user and look at it (camera, asks first)
- Things the user brings you (photos, screenshots, files) arrive with their message.
SYSTEM (root):
- system_info {} : CPU, memory, temperature, battery, storage, network, top processes
- processes {"sort":"mem|cpu"} / kill_process {"pid":123}
- network_connections {} : which programs talk to which servers (security check)
- security_status {} : privacy switches, VPN, screen lock, listening ports
- set_privacy {"group":"all|network|camera|microphone|location|sensors|bluetooth","on":true|false}
- logs {"source":"kernel|services","lines":40}
- list_dir {"path":"/any/path"} / read_any {"path":"/any/file"} / write_file {"path","content","append":false} / delete_path {"path"}
- run_command {"command":"...","where":"system|ubuntu"} : shell command as root (system = Ultar core, ubuntu = Ubuntu area)
- model_status {} / model_download {"id":"qwen3-1.7b","source":"huggingface|ollama"} : local AI models
DEVICE:
- get_status {} : time, network, memory, storage, battery
- set_setting {"name":"brightness|dark|language|rotation|scale|timeout|timezone|volume","value":"..."} : brightness 10-100, dark true/false, language ar/en/de, rotation 0/90/180/270, scale 85/100/115/130, timeout seconds (0=never), timezone minutes from UTC, volume 0-100
- toggle {"name":"wifi|bluetooth|mobile_data|airplane|flashlight|vpn","on":true|false}
- open_app {"name":"home|camera|gallery|phone|messages|contacts|notes|clock|calculator|files|browser|store|terminal|monitor|privacy|settings|network|display|language|time|storage|services|system|hardware|ubuntu_apps|pdf|documents|sheets|code_editor"}
- open_url {"url":"https://..."} : open a web page in the browser
- take_photo {} : opens the camera and takes a photo after 3 seconds
- set_alarm {"hour":7,"minute":30} / start_timer {"minutes":5}
- add_note {"text":"..."} / list_notes {}
- add_contact {"name":"...","number":"..."} / find_contact {"name":"..."} / call {"number":"..."}
- list_files {"path":"/data/..."} / read_file {"path":"/data/..."}
- notify {"title":"...","text":"..."}
- list_services {} / service {"action":"start|stop|restart","name":"..."}
- search_packages {"query":"..."} / install_package {"name":"..."} / remove_package {"name":"..."} : Ubuntu packages (apt)
- power {"action":"reboot|poweroff"}
- help {"topic":"..."} : built-in help for a part of the system (topic ids below). Prefer it when the user asks how something works.
After a tool you receive TOOL_RESULT (sometimes with an image). Use as few steps as possible, then answer briefly."#;

/// Vollständiger Systemprompt: Grundregeln + Katalog aller Hilfethemen, damit das
/// Modell weiß, was jeder Teil des Systems tut (Basis des lokalen Agenten).
pub fn system_prompt() -> String {
    format!("{SYSTEM}\n\nApp guide (topic id: what it does — use `help` for the full text):\n{}", crate::help::digest())
}

/// Antwort des Modells auswerten; duldet Text um das JSON herum.
pub fn parse(content: &str) -> Reply {
    // Denkphase mancher Modelle (<think>…</think>) gehört nicht zur Antwort
    let content = match (content.find("<think>"), content.find("</think>")) {
        (Some(a), Some(b)) if b > a => &content[b + 8..],
        _ => content,
    };
    let (a, b) = (content.find('{'), content.rfind('}'));
    if let (Some(a), Some(b)) = (a, b) {
        if let Ok(v) = serde_json::from_str::<Value>(&content[a..=b]) {
            if let Some(t) = v.get("tool").and_then(|t| t.as_str()) {
                return Reply::Call(Call { tool: t.to_string(), args: v.get("args").cloned().unwrap_or(json!({})) });
            }
            if let Some(ans) = v.get("answer") {
                return Reply::Answer(ans.as_str().map(String::from).unwrap_or_else(|| ans.to_string()));
            }
        }
    }
    Reply::Answer(content.trim().to_string())
}

fn http(endpoint: &str, method: &str, path: &str, body: &str, read_timeout: u64) -> Result<String, String> {
    let addr: SocketAddr = endpoint.trim().parse().map_err(|_| format!("invalid address: {endpoint}"))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(read_timeout)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let req = format!("{method} {path} HTTP/1.1\r\nHost: {endpoint}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").ok_or("invalid HTTP response")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(rest) } else { rest.to_string() };
    if status != 200 {
        return Err(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>()));
    }
    Ok(body)
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((len, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(len.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

pub fn reachable(endpoint: &str) -> bool {
    http(endpoint, "GET", "/v1/models", "", 5).is_ok()
}

/// Eine Runde mit Claude über die Anthropic-API (Messages). Die Verbindung läuft über curl im
/// Ubuntu-Bereich (TLS, DNS); der Schlüssel wird über die Standardeingabe übergeben.
pub fn complete_claude(cfg: &AgentCfg, history: &[Value]) -> Result<String, String> {
    if cfg.api_key.trim().is_empty() {
        return Err("API key missing".into());
    }
    if !crate::apps::curl_ready() {
        return Err("curl is not installed in the Ubuntu area (Packages: curl, ca-certificates)".into());
    }
    // Bilder (file://) als Base64-Blöcke im Format der Anthropic-API
    let msgs: Vec<Value> = history
        .iter()
        .map(|m| {
            let Some(parts) = m["content"].as_array() else { return m.clone() };
            let conv: Vec<Value> = parts
                .iter()
                .map(|p| match p["image_url"]["url"].as_str() {
                    Some(u) => {
                        let path = u.trim_start_matches("file://");
                        match std::fs::read(path) {
                            Ok(b) => {
                                use base64::Engine;
                                let mt = if path.ends_with(".png") { "image/png" } else { "image/jpeg" };
                                json!({"type": "image", "source": {"type": "base64", "media_type": mt, "data": base64::engine::general_purpose::STANDARD.encode(b)}})
                            }
                            Err(_) => json!({"type": "text", "text": "[image missing]"}),
                        }
                    }
                    None => p.clone(),
                })
                .collect();
            json!({"role": m["role"], "content": conv})
        })
        .collect();
    let body = json!({"model": cfg.claude_model, "max_tokens": 1024, "system": system_prompt(), "messages": msgs}).to_string();
    let path = "/tmp/.ultar-agent-req.json";
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
        f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    let conf = format!(
        "url = \"https://api.anthropic.com/v1/messages\"\nheader = \"x-api-key: {}\"\nheader = \"anthropic-version: 2023-06-01\"\nheader = \"content-type: application/json\"\ndata-binary = \"@{path}\"\n",
        cfg.api_key.trim().replace(['"', '\n'], "")
    );
    let out = crate::apps::curl(&conf, &[]);
    let _ = std::fs::remove_file(path);
    let v: Value = serde_json::from_slice(&out?).map_err(|e| format!("invalid response: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(format!("Claude API: {}", e["message"].as_str().unwrap_or("error")));
    }
    v["content"].as_array().and_then(|a| a.iter().find_map(|b| b["text"].as_str())).map(String::from).ok_or_else(|| "Claude API returned no text".to_string())
}

/// Enthält eine Nachricht ein Bild (OpenAI-Teile `image_url`)?
pub fn has_image(m: &Value) -> bool {
    m["content"].as_array().is_some_and(|a| a.iter().any(|p| p.get("image_url").is_some()))
}

/// Nachricht mit Text und Bildern (Dateipfade) im OpenAI-Format
pub fn user_msg(text: &str, images: &[String]) -> Value {
    if images.is_empty() {
        return json!({"role": "user", "content": text});
    }
    let mut parts = vec![json!({"type": "text", "text": text})];
    for i in images {
        parts.push(json!({"type": "image_url", "image_url": {"url": format!("file://{i}")}}));
    }
    json!({"role": "user", "content": parts})
}

/// Nach der Antwort: Bilder aus dem Verlauf durch einen Hinweis ersetzen (nicht bei jeder Runde neu rechnen/senden)
pub fn strip_images(history: &mut [Value]) {
    for m in history.iter_mut() {
        if !has_image(m) {
            continue;
        }
        let parts = m["content"].as_array().cloned().unwrap_or_default();
        let mut text = String::new();
        let mut n = 0;
        for p in parts {
            if let Some(t) = p["text"].as_str() {
                text.push_str(t);
            } else if p.get("image_url").is_some() {
                n += 1;
            }
        }
        m["content"] = json!(format!("{text}\n[{n} image(s) were shown here]"));
    }
}

/// Eine Runde mit dem Modell: liefert den Antworttext.
pub fn complete(endpoint: &str, history: &[Value]) -> Result<String, String> {
    // "/no_think": Qwen3 antwortet ohne Denkphase (schneller); andere Modelle ignorieren den Zusatz
    let mut messages = vec![json!({"role": "system", "content": format!("{}\n/no_think", system_prompt())})];
    messages.extend_from_slice(history);
    // Bild im Verlauf → ein Modell mit Projektor (der Dienst ultar-llm wählt das empfohlene Bild-Modell)
    let model = if history.iter().any(has_image) { "vision" } else { "local" };
    let body = json!({"model": model, "messages": messages, "temperature": 0.2, "max_tokens": 500, "response_format": {"type": "json_object"}});
    let resp = http(endpoint, "POST", "/v1/chat/completions", &body.to_string(), 300)?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("invalid JSON from model server: {e}"))?;
    v["choices"][0]["message"]["content"].as_str().map(String::from).ok_or_else(|| "model server returned no text".to_string())
}
