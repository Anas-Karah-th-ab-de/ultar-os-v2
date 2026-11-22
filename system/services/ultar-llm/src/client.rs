//! Kleiner Client für andere Ultar-Programme (Oberfläche, Helfer, Desktop): spricht den Dienst über
//! den Unix-Socket ("unix:/run/ultar/llm.sock") oder TCP ("127.0.0.1:8080") an.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

pub const DEFAULT_SOCKET: &str = "/run/ultar/llm.sock";

fn connect(endpoint: &str, timeout: u64) -> Result<Box<dyn crate::http::Conn>, String> {
    if let Some(p) = endpoint.strip_prefix("unix:") {
        let s = std::os::unix::net::UnixStream::connect(p).map_err(|e| format!("{p}: {e}"))?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(timeout)));
        Ok(Box::new(s))
    } else {
        let addr: std::net::SocketAddr = endpoint
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .parse()
            .map_err(|_| format!("ungültige Adresse: {endpoint}"))?;
        let s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))
            .map_err(|e| format!("{addr}: {e}"))?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(timeout)));
        Ok(Box::new(s))
    }
}

fn send(
    c: &mut dyn crate::http::Conn,
    method: &str,
    path: &str,
    body: &str,
    key: &str,
) -> Result<(), String> {
    let auth = if key.is_empty() {
        String::new()
    } else {
        format!("Authorization: Bearer {key}\r\n")
    };
    let req = format!("{method} {path} HTTP/1.1\r\nHost: ultar-llm\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    c.write_all(req.as_bytes()).map_err(|e| e.to_string())
}

/// Anfrage mit JSON-Antwort.
pub fn request(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    timeout: u64,
) -> Result<Value, String> {
    let mut c = connect(endpoint, timeout)?;
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    send(
        &mut *c,
        method,
        path,
        &b,
        &std::env::var("ULTAR_LLM_KEY").unwrap_or_default(),
    )?;
    let mut raw = Vec::new();
    c.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (head, rest) = text
        .split_once("\r\n\r\n")
        .ok_or("ungültige HTTP-Antwort")?;
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let v: Value =
        serde_json::from_str(rest).map_err(|e| format!("ungültiges JSON ({status}): {e}"))?;
    if status != 200 {
        return Err(v["error"]["message"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| format!("HTTP {status}")));
    }
    Ok(v)
}

/// Chat mit gestreamter Antwort: `on_text` bekommt jedes Stück; Rückgabe = ganzer Text + Nutzungsdaten.
pub fn chat_stream(
    endpoint: &str,
    body: &Value,
    on_text: &mut dyn FnMut(&str),
) -> Result<(String, Value), String> {
    let mut b = body.clone();
    b["stream"] = json!(true);
    let mut c = connect(endpoint, 1800)?;
    send(
        &mut *c,
        "POST",
        "/v1/chat/completions",
        &b.to_string(),
        &std::env::var("ULTAR_LLM_KEY").unwrap_or_default(),
    )?;
    let mut r = BufReader::new(c);
    let mut status = String::new();
    r.read_line(&mut status).map_err(|e| e.to_string())?;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
            break;
        }
    }
    if !status.contains(" 200 ") {
        let mut rest = String::new();
        let _ = r.read_to_string(&mut rest);
        let v: Value = serde_json::from_str(&rest).unwrap_or(Value::Null);
        return Err(v["error"]["message"]
            .as_str()
            .map(String::from)
            .unwrap_or(status.trim().to_string()));
    }
    let (mut all, mut usage) = (String::new(), Value::Null);
    for line in r.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Some(d) = line.strip_prefix("data: ") else {
            continue;
        };
        if d == "[DONE]" {
            break;
        }
        let v: Value = serde_json::from_str(d).unwrap_or(Value::Null);
        if let Some(e) = v.get("error") {
            return Err(e["message"].as_str().unwrap_or("Fehler").to_string());
        }
        if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
            all.push_str(t);
            on_text(t);
        }
        if !v["usage"].is_null() {
            usage = v["usage"].clone();
        }
    }
    Ok((all, usage))
}

/// Standard-Endpunkt: Socket, wenn vorhanden, sonst 127.0.0.1:8080.
pub fn default_endpoint() -> String {
    if let Ok(e) = std::env::var("ULTAR_LLM") {
        return e;
    }
    for p in [
        DEFAULT_SOCKET.to_string(),
        std::env::var("XDG_RUNTIME_DIR")
            .map(|d| format!("{d}/ultar-llm.sock"))
            .unwrap_or_default(),
    ] {
        if !p.is_empty() && std::path::Path::new(&p).exists() {
            return format!("unix:{p}");
        }
    }
    "127.0.0.1:8080".into()
}
