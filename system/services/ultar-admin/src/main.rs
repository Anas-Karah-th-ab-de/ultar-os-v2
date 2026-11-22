//! ultar-admin – Web-Verwaltung von Ultar OS (Server-Ausgabe immer an, Desktop auf Wunsch).
//!
//!   ultar-admin serve [--listen 0.0.0.0:9443]   HTTPS-Verwaltung starten
//!   ultar-admin passwd                          neues Kennwort setzen (liest von stdin)
//!   ultar-admin fingerprint                     SHA-256 des Zertifikats anzeigen
//!
//! Sicherheit: nur HTTPS (selbst erzeugtes Zertifikat, Fingerabdruck auf der Konsole), Kennwort als
//! Argon2id-Hash, Sitzungs-Cookie HttpOnly+Secure+SameSite=Strict, jede Änderung braucht zusätzlich den
//! Kopf `X-Ultar: 1` (kein Cross-Site-Formular möglich), Sperre nach 5 Fehlversuchen, strenge CSP.

use axum::{
    body::Body,
    extract::{ConnectInfo, Path as AxPath, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);
const COOKIE: &str = "ultar_admin";

fn data_dir() -> PathBuf {
    std::env::var("ULTAR_ADMIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| "/data/ultar/admin".into())
}

#[derive(Default)]
struct Guard {
    fails: u32,
    until: Option<Instant>,
}

struct AppState {
    sessions: Mutex<HashMap<String, Instant>>,
    guards: Mutex<HashMap<IpAddr, Guard>>,
    busy: tokio::sync::Mutex<()>,
}

type St = Arc<AppState>;

// ------------------------------------------------------------ Kennwort

#[derive(Serialize, Deserialize, Default)]
struct AdminConf {
    hash: String,
}

fn conf_path() -> PathBuf {
    data_dir().join("admin.toml")
}

fn hash_pw(pw: &str) -> String {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .expect("argon2")
        .to_string()
}

fn verify_pw(pw: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let Ok(s) = std::fs::read_to_string(conf_path()) else {
        return false;
    };
    let Ok(c) = toml::from_str::<AdminConf>(&s) else {
        return false;
    };
    let Ok(h) = PasswordHash::new(&c.hash) else {
        return false;
    };
    argon2::Argon2::default()
        .verify_password(pw.as_bytes(), &h)
        .is_ok()
}

fn write_private(p: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    std::fs::rename(tmp, p)
}

fn set_password(pw: &str) -> std::io::Result<()> {
    write_private(
        &conf_path(),
        toml::to_string(&AdminConf { hash: hash_pw(pw) })
            .unwrap()
            .as_bytes(),
    )?;
    let _ = std::fs::remove_file(data_dir().join("initial-password"));
    Ok(())
}

fn random_token(n: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// Erster Start: zufälliges Anfangskennwort (nur auf der Konsole und in einer root-Datei sichtbar)
fn ensure_password() {
    if conf_path().exists() {
        return;
    }
    use rand::Rng;
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut r = rand::rngs::OsRng;
    let pw: String = (0..14)
        .map(|_| A[r.gen_range(0..A.len())] as char)
        .collect();
    let _ = write_private(
        &conf_path(),
        toml::to_string(&AdminConf { hash: hash_pw(&pw) })
            .unwrap()
            .as_bytes(),
    );
    let _ = write_private(
        &data_dir().join("initial-password"),
        format!("{pw}\n").as_bytes(),
    );
    println!(
        "ultar-admin: Anfangskennwort: {pw}  (nach der ersten Anmeldung ändern; auch in {})",
        data_dir().join("initial-password").display()
    );
}

// ------------------------------------------------------------ Zertifikat

fn ensure_cert() -> (PathBuf, PathBuf) {
    let (c, k) = (data_dir().join("cert.pem"), data_dir().join("key.pem"));
    if c.exists() && k.exists() {
        return (c, k);
    }
    let host = nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "ultar".into());
    let mut names = vec![host.clone(), format!("{host}.local"), "localhost".into()];
    names.extend(local_ips().into_iter().map(|(_, ip)| ip));
    names.push("127.0.0.1".into());
    let ck = rcgen::generate_simple_self_signed(names).expect("Zertifikat erzeugen");
    write_private(&k, ck.key_pair.serialize_pem().as_bytes()).expect("Schlüssel schreiben");
    write_private(&c, ck.cert.pem().as_bytes()).expect("Zertifikat schreiben");
    (c, k)
}

fn fingerprint(cert: &Path) -> String {
    let pem = std::fs::read_to_string(cert).unwrap_or_default();
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = b64_decode(&b64);
    use sha2::Digest;
    let h = sha2::Sha256::digest(&der);
    h.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter_map(val) {
        acc = (acc << 6) | c;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

// ------------------------------------------------------------ Systemzustand

fn local_ips() -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Ok(addrs) = nix::ifaddrs::getifaddrs() {
        for a in addrs {
            if a.interface_name == "lo" {
                continue;
            }
            if let Some(ad) = a.address {
                if let Some(s4) = ad.as_sockaddr_in() {
                    v.push((a.interface_name.clone(), s4.ip().to_string()));
                } else if let Some(s6) = ad.as_sockaddr_in6() {
                    let ip = s6.ip();
                    if !ip.is_loopback() && (ip.segments()[0] & 0xffc0) != 0xfe80 {
                        v.push((a.interface_name.clone(), ip.to_string()));
                    }
                }
            }
        }
    }
    v
}

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn cpu_sample() -> (u64, u64) {
    let s = read("/proc/stat");
    let v: Vec<u64> = s
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    (v.iter().sum(), idle)
}

fn meminfo() -> (u64, u64) {
    let s = read("/proc/meminfo");
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    (get("MemTotal:"), get("MemAvailable:"))
}

fn disk(path: &str) -> Option<Value> {
    let st = nix::sys::statvfs::statvfs(path).ok()?;
    let total = st.blocks() as u64 * st.fragment_size() as u64;
    if total == 0 {
        return None;
    }
    let free = st.blocks_available() as u64 * st.fragment_size() as u64;
    Some(json!({"path": path, "total": total, "free": free}))
}

async fn api_status() -> Json<Value> {
    let a = cpu_sample();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let b = cpu_sample();
    let dt = b.0.saturating_sub(a.0).max(1);
    let cpu = 100.0 * (1.0 - b.1.saturating_sub(a.1) as f64 / dt as f64);
    let (mt, ma) = meminfo();
    let up: f64 = read("/proc/uptime")
        .split_whitespace()
        .next()
        .and_then(|x| x.parse().ok())
        .unwrap_or(0.0);
    let load = read("/proc/loadavg")
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    let kernel = read("/proc/sys/kernel/osrelease").trim().to_string();
    let host = nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    let cpus = read("/proc/cpuinfo")
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    let model = read("/proc/cpuinfo")
        .lines()
        .find(|l| l.starts_with("model name") || l.starts_with("Hardware"))
        .and_then(|l| l.split(':').nth(1))
        .unwrap_or("")
        .trim()
        .to_string();
    let disks: Vec<Value> = ["/", "/data", "/compat"]
        .iter()
        .filter_map(|p| disk(p))
        .collect();
    let mut bat = Value::Null;
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            if read(&p.join("type").to_string_lossy()).trim() == "Battery" {
                bat = json!({"capacity": read(&p.join("capacity").to_string_lossy()).trim(), "status": read(&p.join("status").to_string_lossy()).trim()});
            }
        }
    }
    let ips: Vec<Value> = local_ips()
        .into_iter()
        .map(|(i, a)| json!({"if": i, "ip": a}))
        .collect();
    Json(json!({
        "host": host, "edition": ultar_hal_system::current_edition().as_str(), "version": env!("CARGO_PKG_VERSION"),
        "kernel": kernel, "uptime": up, "load": load, "cpu": (cpu * 10.0).round() / 10.0, "cpus": cpus, "cpu_model": model,
        "mem_total": mt, "mem_avail": ma, "disks": disks, "net": ips, "battery": bat,
    }))
}

// ------------------------------------------------------------ Dienste (über ultar-init)

fn ctl(cmd: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let sock = std::env::var("ULTAR_CTL_SOCK").unwrap_or_else(|_| "/run/ultar/ctl.sock".into());
    let mut s = std::os::unix::net::UnixStream::connect(&sock)
        .map_err(|e| format!("ultar-init nicht erreichbar: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    s.write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !n.starts_with('.')
}

async fn api_services() -> Response {
    match tokio::task::spawn_blocking(|| ctl("status")).await.unwrap() {
        Ok(t) => {
            let v: Vec<Value> = t.lines().skip(1).filter_map(|l| {
                let c: Vec<&str> = l.split_whitespace().collect();
                (c.len() >= 5).then(|| json!({"name": c[0], "state": c[1], "pid": c[2], "restarts": c[3], "up": c[4]}))
            }).collect();
            Json(json!(v)).into_response()
        }
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))).into_response(),
    }
}

async fn api_service_action(AxPath((name, action)): AxPath<(String, String)>) -> Response {
    if !valid_name(&name) || !["start", "stop", "restart"].contains(&action.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "ungültig"}))).into_response();
    }
    match tokio::task::spawn_blocking(move || ctl(&format!("{action} {name}")))
        .await
        .unwrap()
    {
        Ok(t) => Json(json!({"ok": true, "msg": t.trim()})).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))).into_response(),
    }
}

async fn api_logs(AxPath(name): AxPath<String>) -> Response {
    if !valid_name(&name) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let t = read(&format!("/run/ultar/log/{name}.log"));
    let lines: Vec<&str> = t.lines().collect();
    let tail = lines[lines.len().saturating_sub(300)..].join("\n");
    Json(json!({"name": name, "log": tail})).into_response()
}

async fn api_power(AxPath(action): AxPath<String>) -> Response {
    if action != "reboot" && action != "poweroff" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match tokio::task::spawn_blocking(move || ctl(&action))
        .await
        .unwrap()
    {
        Ok(t) => Json(json!({"ok": true, "msg": t.trim()})).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))).into_response(),
    }
}

// ------------------------------------------------------------ Apps

async fn api_apps() -> Json<Value> {
    let p = ultar_apps::Paths::system();
    let v: Vec<Value> = ultar_apps::list(&p).into_iter().map(|e| json!({"id": e.id, "name": e.name, "format": e.format.map(|f| f.label()).unwrap_or(""), "status": e.status, "note": e.note})).collect();
    let pol = ultar_apps::Policy::load(&p);
    Json(json!({"apps": v, "allow_foreign": pol.allow_foreign}))
}

fn safe_filename(s: &str) -> String {
    let n: String = s
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._-+ ()".contains(*c))
        .take(120)
        .collect();
    let n = n.trim_start_matches('.').to_string();
    if n.is_empty() {
        "upload.bin".into()
    } else {
        n
    }
}

async fn api_install(State(st): State<St>, headers: HeaderMap, body: Body) -> Response {
    let _busy = st.busy.lock().await; // eine Installation zur Zeit
    let name = safe_filename(
        headers
            .get("x-filename")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("upload.bin"),
    );
    let dir = ultar_apps::Paths::system().incoming();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let path = dir.join(format!("upload-{}-{name}", random_token(4)));
    let mut f = match tokio::fs::File::create(&path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };
    let mut stream = body.into_data_stream();
    let mut size: u64 = 0;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(c) => {
                size += c.len() as u64;
                if size > 8 * 1024 * 1024 * 1024 || f.write_all(&c).await.is_err() {
                    let _ = tokio::fs::remove_file(&path).await;
                    return (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(json!({"error": "Datei zu groß oder Speicher voll"})),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": e.to_string()})),
                )
                    .into_response();
            }
        }
    }
    let _ = f.flush().await;
    drop(f);
    let silent = headers.get("x-silent").is_some();
    let p2 = path.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut lines = Vec::new();
        let o = ultar_apps::Opts {
            silent,
            ..Default::default()
        };
        let r = ultar_apps::install(&ultar_apps::Paths::system(), &p2, &o, &mut |l| {
            lines.push(l.to_string())
        });
        (r, lines)
    })
    .await
    .unwrap();
    let _ = tokio::fs::remove_file(&path).await;
    match res {
        (Ok(e), log) => {
            Json(json!({"ok": true, "id": e.id, "name": e.name, "status": e.status, "log": log}))
                .into_response()
        }
        (Err(err), log) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": err, "log": log})),
        )
            .into_response(),
    }
}

async fn api_app_remove(AxPath(id): AxPath<String>) -> Response {
    if !valid_name(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let r = tokio::task::spawn_blocking(move || {
        let mut lines = Vec::new();
        let r = ultar_apps::remove(&ultar_apps::Paths::system(), &id, &mut |l| {
            lines.push(l.to_string())
        });
        (r, lines)
    })
    .await
    .unwrap();
    match r {
        (Ok(()), log) => Json(json!({"ok": true, "log": log})).into_response(),
        (Err(e), log) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": e, "log": log})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct AptReq {
    package: String,
    #[serde(default)]
    remove: bool,
}

/// Ubuntu-Paket aus den Paketquellen (z. B. nginx, postgresql) – nur gültige Paketnamen
async fn api_apt(State(st): State<St>, Json(r): Json<AptReq>) -> Response {
    let ok = !r.package.is_empty()
        && r.package.len() < 100
        && r.package
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c))
        && !r.package.starts_with('-');
    if !ok {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "ungültiger Paketname"})),
        )
            .into_response();
    }
    let _busy = st.busy.lock().await;
    let out = tokio::task::spawn_blocking(move || {
        let compat =
            std::env::var("ULTAR_COMPAT_BIN").unwrap_or_else(|_| "/usr/bin/ultar-compat".into());
        let mut c = std::process::Command::new(compat);
        if r.remove {
            c.args(["apt", "remove", "--autoremove", &r.package]);
        } else {
            let _ = std::process::Command::new(
                std::env::var("ULTAR_COMPAT_BIN")
                    .unwrap_or_else(|_| "/usr/bin/ultar-compat".into()),
            )
            .args(["apt", "update", "-qq"])
            .output();
            c.args(["apt", "install", "--no-install-recommends", &r.package]);
        }
        c.output()
    })
    .await
    .unwrap();
    match out {
        Ok(o) => {
            let log = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let tail: Vec<&str> = log.lines().collect();
            let tail = tail[tail.len().saturating_sub(60)..].join("\n");
            let code = if o.status.success() {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (code, Json(json!({"ok": o.status.success(), "log": tail}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ------------------------------------------------------------ SSH-Schlüssel

fn ssh_keys_path() -> PathBuf {
    std::env::var("ULTAR_SSH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| "/data/ultar/ssh".into())
        .join("authorized_keys")
}

fn valid_key_line(l: &str) -> bool {
    let l = l.trim();
    const P: [&str; 6] = [
        "ssh-ed25519 ",
        "ssh-rsa ",
        "ecdsa-sha2-nistp256 ",
        "ecdsa-sha2-nistp384 ",
        "sk-ssh-ed25519@openssh.com ",
        "sk-ecdsa-sha2-nistp256@openssh.com ",
    ];
    l.len() < 16_384
        && P.iter().any(|p| l.starts_with(p))
        && l.split_whitespace().nth(1).is_some_and(|k| {
            k.len() > 40
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
        })
}

async fn api_ssh_get() -> Json<Value> {
    Json(json!({"keys": std::fs::read_to_string(ssh_keys_path()).unwrap_or_default()}))
}

#[derive(Deserialize)]
struct SshReq {
    keys: String,
}

async fn api_ssh_set(Json(r): Json<SshReq>) -> Response {
    let lines: Vec<&str> = r
        .keys
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if let Some(bad) = lines.iter().find(|l| !valid_key_line(l)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "key", "line": bad.chars().take(40).collect::<String>()})),
        )
            .into_response();
    }
    let p = ssh_keys_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    match write_private(
        &p,
        (lines.join("\n") + if lines.is_empty() { "" } else { "\n" }).as_bytes(),
    ) {
        Ok(()) => Json(json!({"ok": true, "count": lines.len()})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ------------------------------------------------------------ Anmeldung

fn cookie_token(h: &HeaderMap) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .find_map(|c| c.strip_prefix(&format!("{COOKIE}=")).map(String::from))
}

fn logged_in(st: &AppState, h: &HeaderMap) -> bool {
    let Some(t) = cookie_token(h) else {
        return false;
    };
    let mut s = st.sessions.lock().unwrap();
    s.retain(|_, exp| *exp > Instant::now());
    s.contains_key(&t)
}

#[derive(Deserialize)]
struct LoginReq {
    password: String,
}

async fn api_login(
    State(st): State<St>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(r): Json<LoginReq>,
) -> Response {
    let ip = addr.ip();
    {
        let g = st.guards.lock().unwrap();
        if let Some(Guard { until: Some(u), .. }) = g.get(&ip) {
            if *u > Instant::now() {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "locked", "wait": u.duration_since(Instant::now()).as_secs()}))).into_response();
            }
        }
    }
    let pw = r.password;
    let ok = tokio::task::spawn_blocking(move || verify_pw(&pw))
        .await
        .unwrap_or(false);
    let mut g = st.guards.lock().unwrap();
    let e = g.entry(ip).or_default();
    if !ok {
        e.fails += 1;
        if e.fails >= 5 {
            e.until =
                Some(Instant::now() + Duration::from_secs(30 * (1 << (e.fails - 5).min(6)) as u64));
        }
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "wrong"}))).into_response();
    }
    *e = Guard::default();
    drop(g);
    let tok = random_token(32);
    st.sessions
        .lock()
        .unwrap()
        .insert(tok.clone(), Instant::now() + SESSION_TTL);
    let mut resp =
        Json(json!({"ok": true, "initial": data_dir().join("initial-password").exists()}))
            .into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{COOKIE}={tok}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={}",
            SESSION_TTL.as_secs()
        ))
        .unwrap(),
    );
    resp
}

async fn api_logout(State(st): State<St>, h: HeaderMap) -> Response {
    if let Some(t) = cookie_token(&h) {
        st.sessions.lock().unwrap().remove(&t);
    }
    let mut resp = Json(json!({"ok": true})).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "ultar_admin=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0",
        ),
    );
    resp
}

#[derive(Deserialize)]
struct PwReq {
    old: String,
    new: String,
}

async fn api_password(State(st): State<St>, Json(r): Json<PwReq>) -> Response {
    if r.new.chars().count() < 10 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "short"}))).into_response();
    }
    let old = r.old.clone();
    if !tokio::task::spawn_blocking(move || verify_pw(&old))
        .await
        .unwrap_or(false)
    {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "wrong"}))).into_response();
    }
    let new = r.new;
    match tokio::task::spawn_blocking(move || set_password(&new))
        .await
        .unwrap()
    {
        Ok(()) => {
            st.sessions.lock().unwrap().clear(); // alle Sitzungen beenden
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn api_me(State(st): State<St>, h: HeaderMap) -> Json<Value> {
    Json(
        json!({"logged_in": logged_in(&st, &h), "edition": ultar_hal_system::current_edition().as_str()}),
    )
}

/// Alles unter /api außer login/me braucht eine Sitzung; Änderungen zusätzlich den Kopf X-Ultar: 1
async fn guard(State(st): State<St>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let open = path == "/api/login" || path == "/api/me";
    if path.starts_with("/api/") {
        if req.method() != axum::http::Method::GET
            && req.headers().get("x-ultar").and_then(|v| v.to_str().ok()) != Some("1")
        {
            return (StatusCode::FORBIDDEN, Json(json!({"error": "csrf"}))).into_response();
        }
        if !open && !logged_in(&st, req.headers()) {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "login"}))).into_response();
        }
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=31536000"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

// ------------------------------------------------------------ Seiten

const INDEX: &str = include_str!("web/index.html");
const APP_JS: &str = include_str!("web/app.js");
const APP_CSS: &str = include_str!("web/app.css");
const LOGO: &[u8] = include_bytes!("../../../../assets/brand/logo.png");

fn page(ct: &'static str, body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, ct)], body).into_response()
}

fn router(st: St) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { page("text/html; charset=utf-8", INDEX.as_bytes()) }),
        )
        .route(
            "/app.js",
            get(|| async { page("text/javascript; charset=utf-8", APP_JS.as_bytes()) }),
        )
        .route(
            "/app.css",
            get(|| async { page("text/css; charset=utf-8", APP_CSS.as_bytes()) }),
        )
        .route("/logo.png", get(|| async { page("image/png", LOGO) }))
        .route("/api/me", get(api_me))
        .route("/api/login", post(api_login))
        .route("/api/logout", post(api_logout))
        .route("/api/password", post(api_password))
        .route("/api/status", get(api_status))
        .route("/api/services", get(api_services))
        .route("/api/services/:name/:action", post(api_service_action))
        .route("/api/logs/:name", get(api_logs))
        .route("/api/power/:action", post(api_power))
        .route("/api/apps", get(api_apps))
        .route(
            "/api/apps/install",
            post(api_install).layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .route("/api/apps/:id/remove", post(api_app_remove))
        .route("/api/apt", post(api_apt))
        .route("/api/ssh-keys", get(api_ssh_get).post(api_ssh_set))
        .layer(middleware::from_fn_with_state(st.clone(), guard))
        .with_state(st)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("passwd") => {
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).ok();
            let pw = s.trim_end_matches(['\n', '\r']);
            if pw.chars().count() < 10 {
                eprintln!("Kennwort: mindestens 10 Zeichen");
                std::process::exit(1);
            }
            set_password(pw).expect("speichern");
            println!("Kennwort gesetzt.");
        }
        Some("fingerprint") => {
            let (c, _) = ensure_cert();
            println!("{}", fingerprint(&c));
        }
        Some("serve") | None => {
            let mut listen: SocketAddr = match ultar_hal_system::current_edition() {
                // Desktop: nur vom Gerät selbst; Server: aus dem Netz
                ultar_profile::Edition::Server => "0.0.0.0:9443",
                _ => "127.0.0.1:9443",
            }
            .parse()
            .unwrap();
            if let Some(i) = a.iter().position(|x| x == "--listen") {
                listen = a
                    .get(i + 1)
                    .and_then(|x| x.parse().ok())
                    .unwrap_or_else(|| {
                        eprintln!("--listen ADRESSE:PORT");
                        std::process::exit(2)
                    });
            }
            ensure_password();
            let (cert, key) = ensure_cert();
            println!(
                "ultar-admin: https://{listen}  Zertifikat SHA-256 {}",
                fingerprint(&cert)
            );
            for (i, ip) in local_ips() {
                println!(
                    "ultar-admin: erreichbar über https://{ip}:{} ({i})",
                    listen.port()
                );
            }
            let _ = rustls::crypto::ring::default_provider().install_default();
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                    .await
                    .expect("TLS");
                let st = Arc::new(AppState {
                    sessions: Mutex::new(HashMap::new()),
                    guards: Mutex::new(HashMap::new()),
                    busy: tokio::sync::Mutex::new(()),
                });
                axum_server::bind_rustls(listen, tls)
                    .serve(router(st).into_make_service_with_connect_info::<SocketAddr>())
                    .await
                    .expect("Server");
            });
        }
        _ => {
            eprintln!(
                "Benutzung: ultar-admin serve [--listen ADRESSE:PORT] | passwd | fingerprint"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert!(valid_name("ui"));
        assert!(!valid_name("../etc"));
        assert!(!valid_name("a b"));
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("C:\\x\\Setup 1.0.exe"), "Setup 1.0.exe");
        assert_eq!(b64_decode("SGFsbG8="), b"Hallo");
        assert!(valid_key_line("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxJ0nYb7m3Yb0nq5x7k1Vq1u2b3c4d5e6f7g8h9i0j1 anas@pc"));
        assert!(!valid_key_line("command=\"rm -rf /\" ssh-ed25519 AAAA"));
        assert!(!valid_key_line("ssh-ed25519 kurz"));
    }
}
