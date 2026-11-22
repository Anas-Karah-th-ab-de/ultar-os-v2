//! ultraos-server — Ultar-Cloud für ultar.code-craft.tech
//!
//! Dienste: Konten/Login, Geräteregistrierung + eindeutige Rufnummer,
//! Anruf-/Video-Signalisierung (WebRTC über WebSocket), App-Store, OTA-Updates,
//! WireGuard-Peer-Provisionierung, Ultar-Verbund (Geräteliste + E2E-Relais). TLS/ACME liegt davor (Caddy). Siehe docs/DEPLOY.md.

mod auth;
mod db;
mod signal;
mod verbund;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use db::Db;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub struct AppState {
    pub db: Db,
    pub secret: [u8; 32],
    pub hub: signal::Hub,
    /// Ultar-Verbund: Relais zwischen Geräten desselben Kontos
    pub verbund: verbund::Hub,
    /// Zähler für interne WireGuard-IPs (10.80.x.y)
    pub wg_next: std::sync::atomic::AtomicU32,
    pub wg_server_pubkey: String,
    pub wg_endpoint: String,
    /// coturn use-auth-secret (eigenes Geheimnis, nicht der Token-Schlüssel)
    pub turn_secret: String,
    /// öffentlicher TURN/STUN-Host (nicht über Cloudflare-Proxy), z. B. 194.62.29.64
    pub turn_host: String,
}

type Shared = Arc<AppState>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let db_path = std::env::var("ULTRAOS_DB").unwrap_or_else(|_| "ultraos.sqlite".into());
    let db = Db::open(&db_path).expect("DB öffnen");

    // Serverschlüssel für Token: aus Env (hex, 32 Byte) oder zufällig (Neustart = alle Token weg).
    let secret = std::env::var("ULTRAOS_SECRET")
        .ok()
        .and_then(|h| hex::decode(h).ok())
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .unwrap_or_else(|| {
            let mut s = [0u8; 32];
            rand::thread_rng().fill(&mut s);
            tracing::warn!("ULTRAOS_SECRET nicht gesetzt — Zufallsschlüssel (Token überleben keinen Neustart)");
            s
        });

    let wg_start = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT address FROM wg_peers").unwrap();
        let max = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|x| x.ok())
            .filter_map(|a| {
                let ip = a.split('/').next()?.to_string();
                let o: Vec<u32> = ip.split('.').filter_map(|x| x.parse().ok()).collect();
                (o.len() == 4).then(|| (o[2] << 8) | o[3])
            })
            .max()
            .unwrap_or(1);
        max.max(1) + 1
    };

    let state = Arc::new(AppState {
        db,
        secret,
        hub: signal::Hub::default(),
        verbund: verbund::Hub::default(),
        wg_next: std::sync::atomic::AtomicU32::new(wg_start),
        wg_server_pubkey: std::env::var("ULTRAOS_WG_PUBKEY").unwrap_or_else(|_| "<wg-server-pubkey>".into()),
        wg_endpoint: std::env::var("ULTRAOS_WG_ENDPOINT").unwrap_or_else(|_| "ultar.code-craft.tech:51820".into()),
        turn_secret: std::env::var("ULTRAOS_TURN_SECRET").unwrap_or_default(),
        turn_host: std::env::var("ULTRAOS_TURN_HOST").unwrap_or_else(|_| "ultar.code-craft.tech".into()),
    });

    let app = Router::new()
        .route("/v1/health", get(|| async { "ok" }))
        .route("/v1/account/register", post(account_register))
        .route("/v1/account/login", post(account_login))
        .route("/v1/device/register", post(device_register))
        .route("/v1/signal", get(signal_ws))
        .route("/v1/turn-cred", get(turn_cred))
        .route("/v1/store/apps", get(store_apps))
        .route("/v1/update/latest", get(update_latest))
        .route("/v1/vpn/peer", post(vpn_peer))
        .route("/v1/verbund/geraet", post(verbund::geraet))
        .route("/v1/verbund/geraet/entfernen", post(verbund::entfernen))
        .route("/v1/verbund/geraete", get(verbund::geraete))
        .route("/v1/verbund/relais", get(verbund::relais_ws))
        .route("/v1/verbund/statistik", get(verbund::statistik))
        .with_state(state);

    let addr = std::env::var("ULTRAOS_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    tracing::info!("ultraos-server lauscht auf {addr}");
    let l = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(l, app).await.expect("serve");
}

// ---------- Hilfen ----------

fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")
}

fn require_account(st: &AppState, h: &HeaderMap) -> Result<i64, StatusCode> {
    bearer(h)
        .and_then(|t| auth::verify_token(&st.secret, t))
        .ok_or(StatusCode::UNAUTHORIZED)
}

fn gen_number() -> String {
    let mut r = rand::thread_rng();
    format!("UOS-{:04X}-{:04X}", r.gen::<u16>(), r.gen::<u16>())
}

// ---------- Konten ----------

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

async fn account_register(State(st): State<Shared>, Json(c): Json<Credentials>) -> impl IntoResponse {
    if c.username.len() < 3 || c.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"username>=3, password>=8"}))).into_response();
    }
    let ph = match auth::hash_password(&c.password) {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":e}))).into_response(),
    };
    let conn = st.db.0.lock().unwrap();
    let r = conn.execute(
        "INSERT INTO accounts(username, pw_hash, created) VALUES(?1,?2,?3)",
        rusqlite::params![c.username, ph, db::now()],
    );
    match r {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            let token = auth::issue_token(&st.secret, id, 30 * 86400);
            (StatusCode::OK, Json(json!({"account_id":id,"token":token}))).into_response()
        }
        Err(_) => (StatusCode::CONFLICT, Json(json!({"error":"username belegt"}))).into_response(),
    }
}

async fn account_login(State(st): State<Shared>, Json(c): Json<Credentials>) -> impl IntoResponse {
    let conn = st.db.0.lock().unwrap();
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, pw_hash FROM accounts WHERE username=?1",
            rusqlite::params![c.username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    match row {
        Some((id, ph)) if auth::verify_password(&c.password, &ph) => {
            let token = auth::issue_token(&st.secret, id, 30 * 86400);
            (StatusCode::OK, Json(json!({"account_id":id,"token":token}))).into_response()
        }
        _ => (StatusCode::UNAUTHORIZED, Json(json!({"error":"ungültige Zugangsdaten"}))).into_response(),
    }
}

// ---------- Gerät + eindeutige Nummer ----------

#[derive(Deserialize)]
struct DeviceReg {
    device_id: String,
    pubkey: String,
}

async fn device_register(State(st): State<Shared>, h: HeaderMap, Json(d): Json<DeviceReg>) -> impl IntoResponse {
    let acc = match require_account(&st, &h) {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let conn = st.db.0.lock().unwrap();
    // Gerät anlegen (idempotent über device_id).
    let _ = conn.execute(
        "INSERT OR IGNORE INTO devices(account_id, device_id, pubkey, created) VALUES(?1,?2,?3,?4)",
        rusqlite::params![acc, d.device_id, d.pubkey, db::now()],
    );
    // Vorhandene Nummer?
    let existing: Option<String> = conn
        .query_row(
            "SELECT number FROM numbers WHERE device_id=?1",
            rusqlite::params![d.device_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(n) = existing {
        return (StatusCode::OK, Json(json!({"number":n}))).into_response();
    }
    // Neue, kollisionsfreie Nummer vergeben.
    for _ in 0..20 {
        let n = gen_number();
        let ok = conn.execute(
            "INSERT INTO numbers(number, account_id, device_id, created) VALUES(?1,?2,?3,?4)",
            rusqlite::params![n, acc, d.device_id, db::now()],
        );
        if ok.is_ok() {
            return (StatusCode::OK, Json(json!({"number":n}))).into_response();
        }
    }
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"Nummernvergabe fehlgeschlagen"}))).into_response()
}

// ---------- Anruf-Signalisierung ----------

async fn signal_ws(State(st): State<Shared>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| signal::handle(socket, st))
}

/// Kurzlebige TURN-Zugangsdaten (coturn „REST API"-Schema: user = exp:accountid, pass = HMAC).
async fn turn_cred(State(st): State<Shared>, h: HeaderMap) -> impl IntoResponse {
    let acc = match require_account(&st, &h) {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let exp = db::now() + 3600;
    let user = format!("{exp}:{acc}");
    // coturn use-auth-secret: password = base64(hmac_sha1(static-auth-secret, user))
    if st.turn_secret.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"turn nicht eingerichtet"}))).into_response();
    }
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    let mut mac = Hmac::<Sha1>::new_from_slice(st.turn_secret.as_bytes()).unwrap();
    mac.update(user.as_bytes());
    let pass = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, mac.finalize().into_bytes());
    let host = &st.turn_host;
    (
        StatusCode::OK,
        Json(json!({
            "urls": [
                format!("stun:{host}:3478"),
                format!("turn:{host}:3478?transport=udp"),
                format!("turn:{host}:3478?transport=tcp")
            ],
            "username": user,
            "credential": pass,
            "ttl": 3600
        })),
    )
        .into_response()
}

// ---------- Store ----------

#[derive(Serialize)]
struct AppMeta {
    id: String,
    name: String,
    version: String,
    size: i64,
    sha256: String,
    sig: String,
    url: String,
}

async fn store_apps(State(st): State<Shared>, h: HeaderMap) -> impl IntoResponse {
    if require_account(&st, &h).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let conn = st.db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id,name,version,size,sha256,sig,url FROM store_apps").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(AppMeta {
                id: r.get(0)?,
                name: r.get(1)?,
                version: r.get(2)?,
                size: r.get(3)?,
                sha256: r.get(4)?,
                sig: r.get(5)?,
                url: r.get(6)?,
            })
        })
        .unwrap();
    let apps: Vec<AppMeta> = rows.filter_map(|x| x.ok()).collect();
    (StatusCode::OK, Json(apps)).into_response()
}

// ---------- OTA-Update ----------

async fn update_latest(State(st): State<Shared>, h: HeaderMap, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if require_account(&st, &h).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let channel = headers
        .get("x-ultar-channel")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("stable")
        .to_string();
    let conn = st.db.0.lock().unwrap();
    let row = conn
        .query_row(
            "SELECT version,url,roothash,sig FROM releases WHERE channel=?1 ORDER BY created DESC LIMIT 1",
            rusqlite::params![channel],
            |r| {
                Ok(json!({
                    "channel": channel,
                    "version": r.get::<_,String>(0)?,
                    "url": r.get::<_,String>(1)?,
                    "roothash": r.get::<_,String>(2)?,
                    "sig": r.get::<_,String>(3)?,
                }))
            },
        )
        .ok();
    match row {
        Some(j) => (StatusCode::OK, Json(j)).into_response(),
        None => (StatusCode::NO_CONTENT, Json(json!({"update": false}))).into_response(),
    }
}

// ---------- WireGuard-Peer ----------

#[derive(Deserialize)]
struct PeerReq {
    pubkey: String,
}

async fn vpn_peer(State(st): State<Shared>, h: HeaderMap, Json(p): Json<PeerReq>) -> impl IntoResponse {
    let acc = match require_account(&st, &h) {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    // nur echte WireGuard-Schlüssel (32 Byte, base64) annehmen
    let gueltig = p.pubkey.len() == 44
        && p.pubkey.ends_with('=')
        && base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &p.pubkey)
            .map(|v| v.len() == 32)
            .unwrap_or(false);
    if !gueltig {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"ungültiger WireGuard-Schlüssel"}))).into_response();
    }
    // interne IP zuteilen (vorhandene wiederverwenden)
    let conn = st.db.0.lock().unwrap();
    let existing: Option<String> = conn
        .query_row(
            "SELECT address FROM wg_peers WHERE account_id=?1 AND pubkey=?2",
            rusqlite::params![acc, p.pubkey],
            |r| r.get(0),
        )
        .ok();
    let address = if let Some(a) = existing {
        a
    } else {
        let n = st.wg_next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let a = format!("10.80.{}.{}/32", (n >> 8) & 0xff, n & 0xff);
        let _ = conn.execute(
            "INSERT OR REPLACE INTO wg_peers(account_id,pubkey,address,created) VALUES(?1,?2,?3,?4)",
            rusqlite::params![acc, p.pubkey, a, db::now()],
        );
        // Der Host-Dienst ultar-wg-sync übernimmt neue Peers aus der DB nach wg0.
        a
    };
    (
        StatusCode::OK,
        Json(json!({
            "server_pubkey": st.wg_server_pubkey,
            "endpoint": st.wg_endpoint,
            "address": address,
            "dns": "10.80.0.1",
            "allowed_ips": "0.0.0.0/0, ::/0"
        })),
    )
        .into_response()
}
