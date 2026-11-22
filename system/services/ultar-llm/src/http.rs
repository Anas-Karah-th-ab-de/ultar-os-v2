//! Schlanker HTTP/1.1-Server (ohne Laufzeit-Bibliothek) mit OpenAI-kompatiblen Endpunkten.
//! Gleiches Protokoll über TCP (127.0.0.1) und Unix-Socket.
//!
//!   GET  /health                 Zustand
//!   GET  /v1/models              verfügbare Modelle
//!   POST /v1/chat/completions    Chat (auch "stream": true als Server-Sent Events)
//!   POST /v1/completions         reiner Text
//!   GET  /ultar/status           Gerät, Einstellungen, Leistung der letzten Anfrage
//!   POST /ultar/load  {"model":…} Modell vorab laden      POST /ultar/unload  Speicher freigeben

use crate::engine::{GenParams, Msg, JSON_GBNF};
use crate::platform;
use crate::service::{gate, Event, Handle, Input, Job};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BODY: usize = 8 << 20;
const MAX_CONN: usize = 64;
static CONNS: AtomicUsize = AtomicUsize::new(0);

pub trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(h, _)| h.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

fn read_request(r: &mut BufReader<&mut dyn Conn>) -> Result<Option<Request>, String> {
    let mut line = String::new();
    if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
        return Ok(None);
    }
    let mut it = line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if headers.len() > 100 {
            return Err("zu viele Kopfzeilen".into());
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut req = Request { method, path, headers, body: Vec::new() };
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("Anfrage zu groß".into());
    }
    if len > 0 {
        req.body.resize(len, 0);
        r.read_exact(&mut req.body).map_err(|e| e.to_string())?;
    }
    Ok(Some(req))
}

fn respond(w: &mut dyn Conn, code: u16, body: &Value) {
    let b = body.to_string();
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let _ = write!(w, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
    let _ = w.flush();
}

fn err(w: &mut dyn Conn, code: u16, msg: &str) {
    let ty = match code {
        400 => "invalid_request_error",
        401 => "authentication_error",
        404 => "not_found_error",
        503 => "unavailable_error",
        _ => "server_error",
    };
    respond(w, code, &json!({"error": {"message": msg, "type": ty, "code": code}}));
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn rid() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("ultar-{:x}", n ^ (std::process::id() as u128) << 64)
}

/// Eine Verbindung bedienen. `tcp` = über das Netz (Schlüssel prüfen, falls gesetzt).
pub fn serve_conn(h: &Handle, mut conn: Box<dyn Conn>, tcp: bool) {
    if CONNS.fetch_add(1, Ordering::SeqCst) >= MAX_CONN {
        CONNS.fetch_sub(1, Ordering::SeqCst);
        err(&mut *conn, 503, "zu viele Verbindungen");
        return;
    }
    let req = {
        let mut r = BufReader::new(&mut *conn as &mut dyn Conn);
        read_request(&mut r)
    };
    match req {
        Ok(Some(req)) => {
            if tcp && !h.cfg.api_key.is_empty() {
                let ok = req.header("authorization").map(|v| v.strip_prefix("Bearer ").unwrap_or(v) == h.cfg.api_key).unwrap_or(false);
                if !ok {
                    err(&mut *conn, 401, "Schlüssel fehlt oder falsch");
                    CONNS.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
            }
            route(h, &mut *conn, req)
        }
        Ok(None) => {}
        Err(e) => err(&mut *conn, if e.contains("groß") { 413 } else { 400 }, &e),
    }
    CONNS.fetch_sub(1, Ordering::SeqCst);
}

fn route(h: &Handle, w: &mut dyn Conn, req: Request) {
    let body: Value = if req.body.is_empty() { json!({}) } else { serde_json::from_slice(&req.body).unwrap_or(Value::Null) };
    match (req.method.as_str(), req.path.trim_end_matches('/')) {
        ("OPTIONS", _) => {
            let _ = write!(w, "HTTP/1.1 204 No Content\r\nAllow: GET, POST, OPTIONS\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
        ("GET", "/health") | ("GET", "/v1/health") => {
            let s = h.state.lock().unwrap().clone();
            respond(w, 200, &json!({"status": "ok", "engine": s.state, "model": s.model.as_ref().map(|m| m.id.clone())}));
        }
        ("GET", "/v1/models") | ("GET", "/models") => {
            let data: Vec<Value> = h
                .models()
                .iter()
                .map(|m| json!({"id": m.id, "object": "model", "created": 0, "owned_by": "ultar", "meta": {"path": m.path, "bytes": m.bytes, "vision": crate::models::mmproj_for(&m.path).is_some()}}))
                .collect();
            respond(w, 200, &json!({"object": "list", "data": data}));
        }
        ("GET", "/ultar/status") => {
            let s = h.state.lock().unwrap().clone();
            respond(
                w,
                200,
                &json!({
                    "service": s,
                    "profile": &*h.profile,
                    "config": {"ctx": h.cfg.ctx, "threads": h.cfg.threads, "threads_batch": h.cfg.threads_batch, "batch": h.cfg.batch,
                               "kv_q8": h.cfg.kv_q8, "gpu_layers": h.cfg.gpu_layers, "idle_unload_secs": h.cfg.idle_unload_secs},
                    "device": {"mem_available_mb": platform::mem_available() >> 20, "battery": platform::battery(),
                               "cpu_temp_c": platform::cpu_temp(), "mem_pressure": platform::mem_pressure(), "blocked": gate(&h.cfg)},
                    "version": env!("CARGO_PKG_VERSION"),
                }),
            );
        }
        ("GET", "/ultar/catalog") => {
            let installed: Vec<String> = h.models().iter().map(|m| m.id.clone()).collect();
            let cat: Vec<Value> = crate::models::CATALOG.iter().map(|e| {
                let mut v = serde_json::to_value(e).unwrap_or_default();
                v["installed"] = json!(installed.iter().any(|i| i.eq_ignore_ascii_case(e.id)));
                v
            }).collect();
            respond(w, 200, &json!({"recommend": crate::models::recommend(&h.profile), "catalog": cat, "installed": installed,
                                     "dir": h.cfg.model_dirs.first().cloned().unwrap_or_default(), "free_bytes": crate::models::free_space(h.cfg.model_dirs.first().map(|s| s.as_str()).unwrap_or("/"))}));
        }
        ("POST", "/ultar/unload") => {
            h.unload();
            respond(w, 200, &json!({"ok": true}));
        }
        ("POST", "/ultar/load") => match h.load(body.get("model").and_then(|m| m.as_str()).map(String::from)) {
            Ok(info) => respond(w, 200, &json!({"ok": true, "model": info})),
            Err(e) => err(w, 503, &e),
        },
        ("POST", "/v1/chat/completions") | ("POST", "/chat/completions") => completion(h, w, &body, true),
        ("POST", "/v1/completions") | ("POST", "/completions") => completion(h, w, &body, false),
        (_, "/v1/chat/completions" | "/v1/completions" | "/ultar/load" | "/ultar/unload") => err(w, 405, "Methode nicht erlaubt"),
        _ => err(w, 404, "unbekannter Pfad"),
    }
}

fn content_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        // OpenAI-Teile: [{"type":"text","text":"…"}]
        Value::Array(a) => a.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

/// Bilder einer Nachricht: OpenAI `{"type":"image_url","image_url":{"url":…}}` mit `data:`-Adresse (Base64) oder
/// `file:///pfad` bzw. absolutem Pfad (nur Bilddateien), Anthropic-Art `{"type":"image","source":{"data":…}}`.
/// Base64-Bilder landen in einer privaten Datei unter /run/ultar/llm-img (bzw. Temp-Ordner) und werden nach der Antwort gelöscht.
fn cleanup(f: &[String]) {
    for x in f {
        let _ = std::fs::remove_file(x);
    }
}

/// Löscht temporäre Bilddateien, wenn die Anfrage fertig ist
struct TmpFiles(Vec<String>);
impl Drop for TmpFiles {
    fn drop(&mut self) {
        cleanup(&self.0);
    }
}

pub fn content_images(v: &Value, tmp: &mut Vec<String>) -> Result<Vec<String>, String> {
    let Value::Array(a) = v else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for p in a {
        let url = p.get("image_url").and_then(|u| u.get("url").or(Some(u))).and_then(|u| u.as_str()).map(String::from).or_else(|| {
            let s = p.get("source")?;
            Some(format!("data:{};base64,{}", s.get("media_type")?.as_str()?, s.get("data")?.as_str()?))
        });
        let Some(url) = url else { continue };
        if let Some(rest) = url.strip_prefix("data:") {
            let (meta, data) = rest.split_once(',').ok_or("image: bad data URL")?;
            if !meta.ends_with(";base64") {
                return Err("image: only base64 data URLs".into());
            }
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| format!("image: {e}"))?;
            if bytes.len() > 40 << 20 {
                return Err("image too large (max 40 MB)".into());
            }
            let ext = if meta.contains("png") { "png" } else if meta.contains("webp") { "webp" } else { "jpg" };
            let dir = if std::path::Path::new("/run/ultar").is_dir() { "/run/ultar/llm-img".to_string() } else { std::env::temp_dir().join("ultar-llm-img").to_string_lossy().into_owned() };
            let _ = std::fs::create_dir_all(&dir);
            let f = format!("{dir}/{}-{}.{ext}", std::process::id(), rid());
            {
                use std::os::unix::fs::OpenOptionsExt;
                let mut fh = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&f).map_err(|e| e.to_string())?;
                std::io::Write::write_all(&mut fh, &bytes).map_err(|e| e.to_string())?;
            }
            tmp.push(f.clone());
            out.push(f);
        } else {
            let path = url.strip_prefix("file://").unwrap_or(&url).to_string();
            let low = path.to_ascii_lowercase();
            let is_img = [".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif"].iter().any(|e| low.ends_with(e));
            if !path.starts_with('/') || !is_img || path.contains("/../") || !std::path::Path::new(&path).is_file() {
                return Err(format!("image: not a readable image file: {path}"));
            }
            out.push(path);
        }
    }
    Ok(out)
}

pub fn params_from(h: &Handle, b: &Value) -> GenParams {
    let mut p = GenParams { max_tokens: h.cfg.default_max_tokens, ..Default::default() };
    if let Some(n) = b.get("max_tokens").or_else(|| b.get("max_completion_tokens")).or_else(|| b.get("n_predict")).and_then(|v| v.as_i64()) {
        if n > 0 {
            p.max_tokens = n.min(1 << 20) as u32;
        }
    }
    if let Some(t) = b.get("temperature").and_then(|v| v.as_f64()) {
        p.temperature = t as f32;
    }
    if let Some(t) = b.get("top_p").and_then(|v| v.as_f64()) {
        p.top_p = t as f32;
    }
    if let Some(t) = b.get("top_k").and_then(|v| v.as_i64()) {
        p.top_k = t as i32;
    }
    if let Some(t) = b.get("min_p").and_then(|v| v.as_f64()) {
        p.min_p = t as f32;
    }
    if let Some(t) = b.get("repeat_penalty").and_then(|v| v.as_f64()) {
        p.repeat_penalty = t as f32;
    }
    if let Some(s) = b.get("seed").and_then(|v| v.as_u64()) {
        p.seed = s as u32;
    }
    p.stop = match b.get("stop") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).take(8).collect(),
        _ => vec![],
    };
    let rf = b.get("response_format").and_then(|r| r.get("type")).and_then(|t| t.as_str()).unwrap_or("");
    if let Some(g) = b.get("grammar").and_then(|g| g.as_str()).filter(|g| !g.trim().is_empty()) {
        p.grammar = Some(g.to_string());
    } else if rf == "json_object" || rf == "json_schema" {
        p.grammar = Some(JSON_GBNF.to_string());
    }
    p
}

fn completion(h: &Handle, w: &mut dyn Conn, b: &Value, chat: bool) {
    if b.is_null() || !b.is_object() {
        return err(w, 400, "ungültiges JSON");
    }
    let params = params_from(h, b);
    let json_only = params.grammar.as_deref() == Some(JSON_GBNF);
    // Base64-Bilder als Dateien – nach der Antwort wieder löschen
    let mut tmp_imgs: Vec<String> = Vec::new();
    let input = if chat {
        let Some(arr) = b.get("messages").and_then(|m| m.as_array()) else { return err(w, 400, "messages fehlt") };
        let mut msgs: Vec<Msg> = Vec::new();
        for m in arr {
            let c = m.get("content").unwrap_or(&Value::Null);
            let images = match content_images(c, &mut tmp_imgs) {
                Ok(i) => i,
                Err(e) => {
                    cleanup(&tmp_imgs);
                    return err(w, 400, &e);
                }
            };
            msgs.push(Msg { role: m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string(), content: content_text(c), images });
        }
        if msgs.is_empty() {
            return err(w, 400, "messages ist leer");
        }
        Input::Chat { msgs, json_only }
    } else {
        let prompt = match b.get("prompt") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(a)) => a.first().and_then(|x| x.as_str()).unwrap_or("").to_string(),
            _ => return err(w, 400, "prompt fehlt"),
        };
        Input::Text(prompt)
    };
    let _tmp_guard = TmpFiles(std::mem::take(&mut tmp_imgs));
    let stream = b.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let mut want = b.get("model").and_then(|m| m.as_str()).map(String::from);
    // Bilder dabei (oder Modell "vision"): ein installiertes Modell mit Projektor nehmen
    let has_images = matches!(&input, Input::Chat { msgs, .. } if msgs.iter().any(|m| !m.images.is_empty()));
    if has_images || want.as_deref() == Some("vision") {
        let models = h.models();
        let sees = |m: &crate::config::ModelFile| m.path.starts_with("mock:") || crate::models::mmproj_for(&m.path).is_some();
        let can = |id: &str| models.iter().any(|m| m.id == id && sees(m));
        if !want.as_deref().is_some_and(can) {
            let rec = crate::models::recommend(&h.profile).vision;
            let pick = rec.filter(|r| can(r)).map(String::from).or_else(|| models.iter().find(|m| sees(m)).map(|m| m.id.clone()));
            match pick {
                Some(p) => want = Some(p),
                None => return err(w, 400, "no vision model installed – install one (ultar-llm recommend / pull)"),
            }
        }
    }
    let (tx, rx) = mpsc::channel();
    h.submit(Job { model: want, input, params, events: tx });
    let id = rid();
    let created = now();
    let obj = if chat { "chat.completion" } else { "text_completion" };

    if !stream {
        let mut text = String::new();
        let mut model = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(1800)) {
                Ok(Event::Started { model: m }) => model = m,
                Ok(Event::Text(t)) => text.push_str(&t),
                Ok(Event::Done(st)) => {
                    let choice = if chat {
                        json!({"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": st.finish})
                    } else {
                        json!({"index": 0, "text": text, "finish_reason": st.finish})
                    };
                    return respond(
                        w,
                        200,
                        &json!({"id": id, "object": obj, "created": created, "model": model, "choices": [choice],
                            "usage": {"prompt_tokens": st.prompt_tokens, "completion_tokens": st.completion_tokens, "total_tokens": st.prompt_tokens + st.completion_tokens},
                            "timings": {"prompt_ms": st.prompt_ms, "predicted_ms": st.gen_ms, "cached_tokens": st.cached_tokens,
                                        "prompt_per_second": st.prompt_tps(), "predicted_per_second": st.gen_tps()}}),
                    );
                }
                Ok(Event::Error { code, msg }) => return err(w, code, &msg),
                Err(_) => return err(w, 500, "keine Antwort vom Rechenkern"),
            }
        }
    }

    // Server-Sent Events (OpenAI-Format). Kopf erst senden, wenn die Arbeit beginnt – Fehler davor als JSON.
    let mut started = false;
    let mut model = String::new();
    let chunk = |model: &str, delta: Value, finish: Value| -> String {
        let c = if chat { json!({"index": 0, "delta": delta, "finish_reason": finish}) } else { json!({"index": 0, "text": delta.get("content").cloned().unwrap_or(json!("")), "finish_reason": finish}) };
        format!("data: {}\n\n", json!({"id": id, "object": if chat {"chat.completion.chunk"} else {"text_completion"}, "created": created, "model": model, "choices": [c]}))
    };
    loop {
        let ev = match rx.recv_timeout(Duration::from_secs(1800)) {
            Ok(e) => e,
            Err(_) => return,
        };
        match ev {
            Event::Started { model: m } => {
                model = m;
                started = true;
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
                if w.write_all(head.as_bytes()).is_err() {
                    return;
                }
                if chat {
                    let _ = w.write_all(chunk(&model, json!({"role": "assistant", "content": ""}), Value::Null).as_bytes());
                }
                let _ = w.flush();
            }
            Event::Text(t) => {
                // Schreibfehler = Nutzer hat abgebrochen → rx fallen lassen, Arbeits-Thread bricht ab
                if w.write_all(chunk(&model, json!({"content": t}), Value::Null).as_bytes()).is_err() || w.flush().is_err() {
                    return;
                }
            }
            Event::Done(st) => {
                let mut last = chunk(&model, json!({}), json!(st.finish));
                last.push_str(&format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"id": id, "object": "chat.completion.chunk", "created": created, "model": model, "choices": [],
                        "usage": {"prompt_tokens": st.prompt_tokens, "completion_tokens": st.completion_tokens, "total_tokens": st.prompt_tokens + st.completion_tokens}})
                ));
                let _ = w.write_all(last.as_bytes());
                let _ = w.flush();
                return;
            }
            Event::Error { code, msg } => {
                if !started {
                    return err(w, code, &msg);
                }
                let _ = w.write_all(format!("data: {}\n\ndata: [DONE]\n\n", json!({"error": {"message": msg, "code": code}})).as_bytes());
                return;
            }
        }
    }
}

/// Lauscht auf TCP und/oder Unix-Socket, je Verbindung ein Thread. Blockiert.
pub fn run(h: Handle) -> Result<(), String> {
    use std::net::TcpListener;
    use std::os::unix::net::UnixListener;
    let mut threads = Vec::new();
    if !h.cfg.socket.is_empty() {
        let path = std::path::Path::new(&h.cfg.socket);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::remove_file(path);
        let l = UnixListener::bind(path).map_err(|e| format!("{}: {e}", h.cfg.socket))?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660));
        }
        eprintln!("ultar-llm: Socket {}", h.cfg.socket);
        let h2 = h.clone();
        threads.push(std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let h3 = h2.clone();
                let _ = c.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = std::thread::Builder::new().stack_size(256 << 10).spawn(move || serve_conn(&h3, Box::new(c), false));
            }
        }));
    }
    if !h.cfg.listen.is_empty() {
        let addr: std::net::SocketAddr = h.cfg.listen.parse().map_err(|_| format!("ungültige Adresse: {}", h.cfg.listen))?;
        if !addr.ip().is_loopback() && !h.cfg.allow_remote {
            return Err(format!("{addr} ist nicht lokal – nur mit allow_remote = true (und api_key) erlaubt"));
        }
        if !addr.ip().is_loopback() && h.cfg.api_key.is_empty() {
            return Err("allow_remote braucht einen api_key".into());
        }
        let l = TcpListener::bind(addr).map_err(|e| format!("{addr}: {e}"))?;
        eprintln!("ultar-llm: http://{addr}/v1");
        let h2 = h.clone();
        threads.push(std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let h3 = h2.clone();
                let _ = c.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = c.set_nodelay(true);
                let _ = std::thread::Builder::new().stack_size(256 << 10).spawn(move || serve_conn(&h3, Box::new(c), true));
            }
        }));
    }
    if threads.is_empty() {
        return Err("weder listen noch socket gesetzt".into());
    }
    for t in threads {
        let _ = t.join();
    }
    Ok(())
}
