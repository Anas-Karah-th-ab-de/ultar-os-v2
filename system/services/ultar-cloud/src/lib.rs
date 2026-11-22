//! Ultar-Cloud-Client. HTTPS läuft über das geprüfte statische `curl` (TLS liegt bei Caddy vorne),
//! damit der statische aarch64-Build keine TLS-Bibliothek einbinden muss. JSON über serde_json.

use serde_json::Value;

pub const CONF: &str = "/data/ultar/cloud.conf"; // base_url in Zeile 1
pub const TOKEN_FILE: &str = "/data/ultar/cloud.token";
pub const NUMBER_FILE: &str = "/data/ultar/number";

pub const DEFAULT_BASE: &str = "https://ultar.code-craft.tech";

pub fn base_url() -> String {
    std::fs::read_to_string(CONF)
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

pub fn token() -> Option<String> {
    std::fs::read_to_string(TOKEN_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_token(t: &str) {
    let _ = std::fs::create_dir_all("/data/ultar");
    let _ = std::fs::write(TOKEN_FILE, t);
}

/// HTTP-Aufruf über curl. body=None → GET/kein Body. Gibt den geparsten JSON-Body zurück.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build()
}

/// HTTPS direkt in Rust (rustls) – ohne curl, läuft auch auf dem Ultar-Kern ohne Ubuntu
fn http(method: &str, path: &str, auth: Option<&str>, body: Option<&str>) -> Result<Value, String> {
    let url = format!("{}{}", base_url(), path);
    let mut r = agent()
        .request(method, &url)
        .set("content-type", "application/json");
    if let Some(t) = auth {
        r = r.set("authorization", &format!("Bearer {t}"));
    }
    let res = match body {
        Some(b) => r.send_string(b),
        None => r.call(),
    };
    let resp = match res {
        Ok(x) => x,
        Err(ureq::Error::Status(code, x)) => {
            let t = x.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v["error"].as_str().map(String::from))
                .unwrap_or(t);
            return Err(format!("HTTP {code}: {}", msg.trim()));
        }
        Err(e) => return Err(format!("Verbindung: {e}")),
    };
    let t = resp.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| format!("JSON: {e} / {t}"))
}

/// Datei herunterladen (HTTPS, rustls)
fn laden(url: &str, auth: Option<&str>, ziel: &str) -> Result<(), String> {
    let mut r = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .get(url);
    if let Some(t) = auth {
        r = r.set("authorization", &format!("Bearer {t}"));
    }
    let resp = r.call().map_err(|e| format!("Download: {e}"))?;
    let mut f = std::fs::File::create(ziel).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.into_reader(), &mut f).map_err(|e| format!("Download: {e}"))?;
    Ok(())
}

// ---------- Konto ----------

pub fn register(username: &str, password: &str) -> Result<String, String> {
    let body = serde_json::json!({"username":username,"password":password}).to_string();
    let v = http("POST", "/v1/account/register", None, Some(&body))?;
    let t = v
        .get("token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| err(&v))?
        .to_string();
    save_token(&t);
    Ok(t)
}

pub fn login(username: &str, password: &str) -> Result<String, String> {
    let body = serde_json::json!({"username":username,"password":password}).to_string();
    let v = http("POST", "/v1/account/login", None, Some(&body))?;
    let t = v
        .get("token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| err(&v))?
        .to_string();
    save_token(&t);
    Ok(t)
}

// ---------- Geräte-ID + eindeutige Nummer ----------

/// Stabile, nicht umkehrbare Geräte-ID aus Seriennummer + Gerätepubkey.
pub fn device_id() -> String {
    let serial = std::fs::read_to_string("/proc/cmdline")
        .ok()
        .and_then(|c| {
            c.split_whitespace().find_map(|k| {
                k.strip_prefix("androidboot.serialno=")
                    .map(|s| s.to_string())
            })
        })
        .or_else(|| std::fs::read_to_string("/sys/class/android_usb/android0/iSerial").ok())
        .unwrap_or_else(|| "unknown".into());
    let pk = std::fs::read_to_string("/data/ultar/device.pub").unwrap_or_default();
    // einfache, stabile Kurz-ID (kein Krypto-Anspruch – Eindeutigkeit reicht)
    let mut h: u64 = 1469598103934665603;
    for b in serial.trim().bytes().chain(pk.trim().bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

pub fn register_number() -> Result<String, String> {
    let t = token().ok_or("nicht angemeldet")?;
    let body = serde_json::json!({"device_id":device_id(),"pubkey":std::fs::read_to_string("/data/ultar/device.pub").unwrap_or_default().trim()}).to_string();
    let v = http("POST", "/v1/device/register", Some(&t), Some(&body))?;
    let n = v
        .get("number")
        .and_then(|x| x.as_str())
        .ok_or_else(|| err(&v))?
        .to_string();
    let _ = std::fs::write(NUMBER_FILE, &n);
    Ok(n)
}

pub fn my_number() -> Option<String> {
    std::fs::read_to_string(NUMBER_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------- VPN-Provisionierung ----------

/// Schlüsselpaar erzeugen, Pubkey an den Server, Antwort als /run/ultar/wg-peer.conf ablegen.
pub fn vpn_provision() -> Result<(), String> {
    let t = token().ok_or("nicht angemeldet")?;
    let (priv_k, pub_k) = ultar_vpn::genkeys()?;
    let body = serde_json::json!({"pubkey":pub_k}).to_string();
    let v = http("POST", "/v1/vpn/peer", Some(&t), Some(&body))?;
    let endpoint = v
        .get("endpoint")
        .and_then(|x| x.as_str())
        .ok_or_else(|| err(&v))?;
    let server_pubkey = v
        .get("server_pubkey")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let address = v.get("address").and_then(|x| x.as_str()).unwrap_or("");
    let dns = v.get("dns").and_then(|x| x.as_str()).unwrap_or("10.80.0.1");
    let allowed = v
        .get("allowed_ips")
        .and_then(|x| x.as_str())
        .unwrap_or("0.0.0.0/0, ::/0");
    let _ = std::fs::create_dir_all("/run/ultar");
    let conf = format!(
        "private_key={priv_k}\nserver_pubkey={server_pubkey}\nendpoint={endpoint}\naddress={address}\ndns={dns}\nallowed_ips={allowed}\n"
    );
    std::fs::write("/run/ultar/wg-peer.conf", conf).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- Store ----------

#[derive(serde::Deserialize, Clone, Debug)]
pub struct AppMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub size: i64,
    pub sha256: String,
    pub sig: String,
    pub url: String,
}

pub fn store_list() -> Result<Vec<AppMeta>, String> {
    let t = token().ok_or("nicht angemeldet")?;
    let v = http("GET", "/v1/store/apps", Some(&t), None)?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// App installieren: .uap herunterladen und über ultar-pkg installieren.
/// ultar-pkg prüft die Signatur gegen die vertrauten Schlüssel — unsigniert/ungültig wird abgelehnt.
pub fn store_install(app: &AppMeta) -> Result<String, String> {
    let t = token().ok_or("nicht angemeldet")?;
    let _ = std::fs::create_dir_all("/data/ultar/dl");
    let dst = format!("/data/ultar/dl/{}.uap", app.id);
    // URL kann absolut sein oder relativ zum Server
    let url = if app.url.starts_with("http") {
        app.url.clone()
    } else {
        format!("{}{}", base_url(), app.url)
    };
    laden(&url, Some(&t), &dst)?;
    // harte Signaturprüfung + Installation
    let m = ultar_pkg::install(
        std::path::Path::new(&dst),
        std::path::Path::new(ultar_pkg::TRUSTED_DIR),
    )?;
    let _ = std::fs::remove_file(&dst);
    Ok(format!("{} v{}", m.name, m.version))
}

// ---------- OTA-Update ----------

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Release {
    pub channel: String,
    pub version: String,
    pub url: String,
    pub roothash: String,
    pub sig: String,
}

pub fn update_check(channel: &str) -> Result<Option<Release>, String> {
    let t = token().ok_or("nicht angemeldet")?;
    let url = format!("{}/v1/update/latest", base_url());
    let resp = match agent()
        .get(&url)
        .set("authorization", &format!("Bearer {t}"))
        .set("x-ultar-channel", channel)
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(..)) => return Ok(None),
        Err(e) => return Err(format!("Verbindung: {e}")),
    };
    let text = resp.into_string().map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v.get("version").is_some() {
        Ok(Some(serde_json::from_value(v).map_err(|e| e.to_string())?))
    } else {
        Ok(None)
    }
}

fn err(v: &Value) -> String {
    v.get("error")
        .and_then(|x| x.as_str())
        .unwrap_or("unerwartete Antwort")
        .to_string()
}
