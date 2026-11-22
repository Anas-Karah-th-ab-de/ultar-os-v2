//! Ultar-Verbund on the server: device list per account and a relay for devices that are not in
//! the same network.
//!
//! The server is a pure relay: connections between devices are Noise_XX encrypted (end to end),
//! only ciphertext frames arrive here. Frames are forwarded exclusively between devices of the
//! SAME account. Buffers are bounded; if a device is overloaded or gone the sender gets
//! "unreachable".
//!
//! Frame (binary): `[type:1][pk:32][session:8][payload]`, type 1=open 2=data 3=close
//! 4=unreachable. The sender writes the TARGET into pk; the server replaces it with the SENDER
//! when forwarding.
//!
//! The identifiers of this module are German because devices in the field speak this protocol
//! (JSON field names, routes); they are kept unchanged.

use crate::{auth, db, AppState};
use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Frame type: open a session.
pub const OEFFNEN: u8 = 1;
/// Frame type: session data.
#[allow(dead_code)]
pub const DATEN: u8 = 2;
/// Frame type: close a session.
pub const SCHLIESSEN: u8 = 3;
/// Frame type: target unreachable (sent by the server).
pub const UNERREICHBAR: u8 = 4;
/// Puffer je Verbindung (Rahmen); voll → Absender bekommt „unerreichbar“
const PUFFER: usize = 512;
const MAX_RAHMEN: usize = 70 * 1024;
const MAX_JE_KONTO: usize = 64;

/// Table of the devices that belong to an account's Verbund.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS verbund_geraete (
    pk         TEXT PRIMARY KEY,       -- Noise-X25519-Schlüssel (hex) des Geräts
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    ausgabe    TEXT NOT NULL,          -- handy | laptop | server | platine
    zuletzt    INTEGER NOT NULL
);
"#;

struct Verb {
    id: u64,
    pk: String,
    rolle: String,
    name: String,
    ausgabe: String,
    kerne: u64,
    frei: u64,
    tx: mpsc::Sender<Message>,
}

#[derive(Default)]
struct Innen {
    /// Konto → Verbindungen
    konten: HashMap<i64, Vec<Verb>>,
    /// (öffnender pk, ziel pk, sitzung) → Verbindungs-id des Öffnenden (damit Antworten zum richtigen Prozess gehen)
    sitzungen: HashMap<(String, String, [u8; 8]), u64>,
}

/// Live relay connections of all accounts plus the connection id counter.
#[derive(Default, Clone)]
pub struct Hub(Arc<Mutex<Innen>>, Arc<AtomicU64>);

/// Relay counters since process start.
pub struct Zaehler {
    /// Frames received.
    pub rahmen: AtomicU64,
    /// Frames dropped (target missing or overloaded).
    pub verworfen: AtomicU64,
}

/// The global relay counters.
pub static ZAEHLER: Zaehler = Zaehler {
    rahmen: AtomicU64::new(0),
    verworfen: AtomicU64::new(0),
};

fn gueltiger_pk(pk: &str) -> bool {
    pk.len() == 64
        && pk
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Gerät dem Konto zuordnen. Gehört der Schlüssel schon einem anderen Konto → Fehler.
fn eintragen(
    st: &AppState,
    acc: i64,
    pk: &str,
    name: &str,
    ausgabe: &str,
) -> Result<(), StatusCode> {
    if !gueltiger_pk(pk) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let conn = st.db.lock();
    let besitzer: Option<i64> = conn
        .query_row(
            "SELECT account_id FROM verbund_geraete WHERE pk=?1",
            rusqlite::params![pk],
            |r| r.get(0),
        )
        .ok();
    if let Some(b) = besitzer {
        if b != acc {
            return Err(StatusCode::CONFLICT);
        }
    }
    let name: String = name.chars().take(64).collect();
    let ausgabe: String = ausgabe.chars().take(16).collect();
    conn.execute(
        "INSERT INTO verbund_geraete(pk,account_id,name,ausgabe,zuletzt) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(pk) DO UPDATE SET name=excluded.name, ausgabe=excluded.ausgabe, zuletzt=excluded.zuletzt",
        rusqlite::params![pk, acc, name, ausgabe, db::now()],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

/// Body of `POST /v1/verbund/geraet`.
#[derive(Deserialize)]
pub struct GeraetReg {
    pk: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    ausgabe: String,
}

/// `POST /v1/verbund/geraet` — assigns a device key to the account.
pub async fn geraet(
    State(st): State<Arc<AppState>>,
    h: HeaderMap,
    Json(g): Json<GeraetReg>,
) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match eintragen(&st, acc, &g.pk, &g.name, &g.ausgabe) {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(e) => (e, Json(json!({"error":"schlüssel ungültig oder fremd"}))).into_response(),
    }
}

/// Body of `POST /v1/verbund/geraet/entfernen`.
#[derive(Deserialize)]
pub struct Entfernen {
    pk: String,
}

/// `POST /v1/verbund/geraet/entfernen` — removes a device from the account.
pub async fn entfernen(
    State(st): State<Arc<AppState>>,
    h: HeaderMap,
    Json(g): Json<Entfernen>,
) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let n = st
        .db
        .lock()
        .execute(
            "DELETE FROM verbund_geraete WHERE pk=?1 AND account_id=?2",
            rusqlite::params![g.pk, acc],
        )
        .unwrap_or(0);
    Json(json!({"entfernt":n})).into_response()
}

/// `GET /v1/verbund/geraete` — devices of the account with their online state.
pub async fn geraete(State(st): State<Arc<AppState>>, h: HeaderMap) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let online: Vec<String> = st
        .verbund
        .lock_inner()
        .konten
        .get(&acc)
        .map(|v| v.iter().map(|c| c.pk.clone()).collect())
        .unwrap_or_default();
    let conn = st.db.lock();
    let liste: rusqlite::Result<Vec<serde_json::Value>> = conn
        .prepare("SELECT pk,name,ausgabe,zuletzt FROM verbund_geraete WHERE account_id=?1 ORDER BY name")
        .and_then(|mut s| {
            let rows = s.query_map(rusqlite::params![acc], |r| {
                let pk: String = r.get(0)?;
                Ok(json!({"pk":pk,"name":r.get::<_,String>(1)?,"ausgabe":r.get::<_,String>(2)?,"zuletzt":r.get::<_,i64>(3)?,"online":online.contains(&pk)}))
            })?;
            rows.collect()
        });
    let Ok(liste) = liste else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    Json(json!({"geraete":liste})).into_response()
}

/// `GET /v1/verbund/relais` — WebSocket relay between devices of one account.
pub async fn relais_ws(State(st): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.max_message_size(MAX_RAHMEN)
        .max_frame_size(MAX_RAHMEN)
        .on_upgrade(move |s| relais(s, st))
}

impl Hub {
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Innen> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn liste_senden(&self, i: &Innen, acc: i64) {
        let Some(v) = i.konten.get(&acc) else { return };
        let liste: Vec<serde_json::Value> = v
            .iter()
            .filter(|c| c.rolle == "dienst")
            .map(|c| json!({"pk":c.pk,"name":c.name,"ausgabe":c.ausgabe,"kerne":c.kerne,"frei":c.frei}))
            .collect();
        let m = json!({"typ":"geraete","liste":liste}).to_string();
        for c in v {
            let _ = c.tx.try_send(Message::Text(m.clone()));
        }
    }

    /// Number of live relay connections.
    pub fn anzahl(&self) -> usize {
        self.lock_inner().konten.values().map(|v| v.len()).sum()
    }
}

fn rahmen(typ: u8, pk: &[u8], s: &[u8; 8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(41);
    b.push(typ);
    b.extend_from_slice(pk);
    b.extend_from_slice(s);
    b
}

/// One admitted relay connection.
struct RelaySession {
    id: u64,
    acc: i64,
    pk: String,
    pk_raw: Vec<u8>,
    rolle: String,
    out_tx: mpsc::Sender<Message>,
}

/// Checks the login message, assigns the device to the account and registers the connection.
/// On refusal returns the protocol message to send before closing.
fn admit(
    st: &AppState,
    anm: &serde_json::Value,
) -> Result<(RelaySession, mpsc::Receiver<Message>), String> {
    let acc = auth::verify_token(&st.secret, anm["token"].as_str().unwrap_or(""))
        .ok_or_else(|| r#"{"typ":"fehler","text":"anmeldung"}"#.to_string())?;
    let pk = anm["pk"].as_str().unwrap_or("").to_string();
    let name = anm["name"].as_str().unwrap_or("").to_string();
    let ausgabe = anm["ausgabe"].as_str().unwrap_or("").to_string();
    let rolle = if anm["rolle"] == "dienst" {
        "dienst"
    } else {
        "client"
    }
    .to_string();
    eintragen(st, acc, &pk, &name, &ausgabe)
        .map_err(|e| json!({"typ":"fehler","text":format!("gerät: {e}")}).to_string())?;
    let (out_tx, out_rx) = mpsc::channel::<Message>(PUFFER);
    let id = st.verbund.1.fetch_add(1, Ordering::SeqCst) + 1;
    let mut i = st.verbund.lock_inner();
    let v = i.konten.entry(acc).or_default();
    if v.len() >= MAX_JE_KONTO {
        return Err(r#"{"typ":"fehler","text":"zu viele Verbindungen"}"#.to_string());
    }
    // One service per device: replace an older service connection of the same key.
    if rolle == "dienst" {
        v.retain(|c| !(c.pk == pk && c.rolle == "dienst"));
    }
    v.push(Verb {
        id,
        pk: pk.clone(),
        rolle: rolle.clone(),
        name,
        ausgabe,
        kerne: anm["kerne"].as_u64().unwrap_or(0),
        frei: anm["frei"].as_u64().unwrap_or(0),
        tx: out_tx.clone(),
    });
    let _ = out_tx.try_send(Message::Text(r#"{"typ":"bereit"}"#.into()));
    st.verbund.liste_senden(&i, acc);
    let pk_raw = hex::decode(&pk).unwrap_or_default();
    Ok((
        RelaySession {
            id,
            acc,
            pk,
            pk_raw,
            rolle,
            out_tx,
        },
        out_rx,
    ))
}

/// Picks the connection a frame goes to and keeps the session table up to date.
fn route(
    st: &AppState,
    sess: &RelaySession,
    typ: u8,
    ziel: &str,
    s: [u8; 8],
) -> Option<mpsc::Sender<Message>> {
    let mut i = st.verbund.lock_inner();
    let forward_key = (sess.pk.clone(), ziel.to_string(), s);
    let reverse_key = (ziel.to_string(), sess.pk.clone(), s);
    // Reply to a session the target opened?
    let rueck = i.sitzungen.get(&reverse_key).copied();
    if typ == OEFFNEN {
        i.sitzungen.insert(forward_key.clone(), sess.id);
        if i.sitzungen.len() > 100_000 {
            i.sitzungen.clear();
        }
    }
    if typ == SCHLIESSEN || typ == UNERREICHBAR {
        i.sitzungen.remove(&forward_key);
        i.sitzungen.remove(&reverse_key);
    }
    let v = i.konten.get(&sess.acc)?;
    match rueck {
        Some(rid) => v.iter().find(|c| c.id == rid).map(|c| c.tx.clone()),
        None => v
            .iter()
            .find(|c| c.pk == ziel && c.rolle == "dienst")
            .map(|c| c.tx.clone()),
    }
}

/// Forwards one binary frame within the account; the target key is replaced by the sender key.
fn forward_frame(st: &AppState, sess: &RelaySession, b: Vec<u8>) {
    if b.len() < 41 || sess.pk_raw.len() != 32 {
        return;
    }
    ZAEHLER.rahmen.fetch_add(1, Ordering::Relaxed);
    let typ = b[0];
    let ziel = hex::encode(&b[1..33]);
    let mut s = [0u8; 8];
    s.copy_from_slice(&b[33..41]);
    let mut neu = b.clone();
    neu[1..33].copy_from_slice(&sess.pk_raw);
    let delivered = route(st, sess, typ, &ziel, s)
        .is_some_and(|target| target.try_send(Message::Binary(neu)).is_ok());
    if !delivered && typ != SCHLIESSEN && typ != UNERREICHBAR {
        ZAEHLER.verworfen.fetch_add(1, Ordering::Relaxed);
        let _ = sess
            .out_tx
            .try_send(Message::Binary(rahmen(UNERREICHBAR, &b[1..33], &s)));
    }
}

/// Handles a text message of an admitted connection (`{"typ":"frei","frei":n}`).
fn update_free(st: &AppState, sess: &RelaySession, text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    if v["typ"] != "frei" {
        return;
    }
    let mut i = st.verbund.lock_inner();
    if let Some(c) = i
        .konten
        .get_mut(&sess.acc)
        .and_then(|v| v.iter_mut().find(|c| c.id == sess.id))
    {
        c.frei = v["frei"].as_u64().unwrap_or(0);
    }
    st.verbund.liste_senden(&i, sess.acc);
}

/// Removes the connection and its sessions and tells the remaining devices.
fn leave(st: &AppState, sess: &RelaySession) {
    let mut i = st.verbund.lock_inner();
    if let Some(v) = i.konten.get_mut(&sess.acc) {
        v.retain(|c| c.id != sess.id);
        if v.is_empty() {
            i.konten.remove(&sess.acc);
        }
    }
    i.sitzungen.retain(|_, v| *v != sess.id);
    st.verbund.liste_senden(&i, sess.acc);
}

async fn relais(socket: WebSocket, st: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();
    let anm = match tokio::time::timeout(std::time::Duration::from_secs(10), rx.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => {
            serde_json::from_str::<serde_json::Value>(&t).unwrap_or_default()
        }
        _ => return,
    };
    let (sess, mut out_rx) = match admit(&st, &anm) {
        Ok(admitted) => admitted,
        Err(refusal) => {
            let _ = tx.send(Message::Text(refusal)).await;
            return;
        }
    };
    tracing::info!(event = "verbund_connected", role = %sess.rolle);

    let schreiber = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if tx.send(m).await.is_err() {
                break;
            }
        }
    });

    loop {
        let m = match tokio::time::timeout(std::time::Duration::from_secs(90), rx.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        match m {
            Message::Binary(b) => forward_frame(&st, &sess, b),
            Message::Text(t) => update_free(&st, &sess, &t),
            Message::Close(_) => break,
            _ => {}
        }
    }
    schreiber.abort();
    leave(&st, &sess);
    tracing::info!(event = "verbund_disconnected", role = %sess.rolle);
}

/// `GET /v1/verbund/statistik` — connection and frame counters.
pub async fn statistik(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(
        json!({"verbindungen":st.verbund.anzahl(),"rahmen":ZAEHLER.rahmen.load(Ordering::Relaxed),"verworfen":ZAEHLER.verworfen.load(Ordering::Relaxed)}),
    )
}
