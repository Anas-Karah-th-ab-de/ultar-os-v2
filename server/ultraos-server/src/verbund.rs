//! Ultar-Verbund auf dem Server: Geräteliste je Konto und Relais für Geräte, die nicht im selben Netz sind.
//!
//! Der Server ist ein reiner Vermittler: Verbindungen zwischen Geräten sind Noise_XX-verschlüsselt
//! (Ende-zu-Ende), hier kommen nur Chiffretext-Rahmen an. Weitergeleitet wird ausschließlich zwischen
//! Geräten DESSELBEN Kontos. Puffer sind begrenzt; ist ein Gerät überlastet oder weg, bekommt der
//! Absender „unerreichbar“.
//!
//! Rahmen (binär): [typ:1][pk:32][sitzung:8][nutzlast]  typ 1=öffnen 2=daten 3=schließen 4=unerreichbar
//! Der Absender schreibt in pk das ZIEL, der Server ersetzt es beim Weiterleiten durch den ABSENDER.

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

pub const OEFFNEN: u8 = 1;
#[allow(dead_code)]
pub const DATEN: u8 = 2;
pub const SCHLIESSEN: u8 = 3;
pub const UNERREICHBAR: u8 = 4;
/// Puffer je Verbindung (Rahmen); voll → Absender bekommt „unerreichbar“
const PUFFER: usize = 512;
const MAX_RAHMEN: usize = 70 * 1024;
const MAX_JE_KONTO: usize = 64;

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

#[derive(Default, Clone)]
pub struct Hub(Arc<Mutex<Innen>>, Arc<AtomicU64>);

pub struct Zaehler {
    pub rahmen: AtomicU64,
    pub verworfen: AtomicU64,
}

pub static ZAEHLER: Zaehler = Zaehler { rahmen: AtomicU64::new(0), verworfen: AtomicU64::new(0) };

fn gueltiger_pk(pk: &str) -> bool {
    pk.len() == 64 && pk.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")
}

/// Gerät dem Konto zuordnen. Gehört der Schlüssel schon einem anderen Konto → Fehler.
fn eintragen(st: &AppState, acc: i64, pk: &str, name: &str, ausgabe: &str) -> Result<(), StatusCode> {
    if !gueltiger_pk(pk) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let conn = st.db.0.lock().unwrap();
    let besitzer: Option<i64> = conn.query_row("SELECT account_id FROM verbund_geraete WHERE pk=?1", rusqlite::params![pk], |r| r.get(0)).ok();
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

#[derive(Deserialize)]
pub struct GeraetReg {
    pk: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    ausgabe: String,
}

pub async fn geraet(State(st): State<Arc<AppState>>, h: HeaderMap, Json(g): Json<GeraetReg>) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match eintragen(&st, acc, &g.pk, &g.name, &g.ausgabe) {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(e) => (e, Json(json!({"error":"schlüssel ungültig oder fremd"}))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct Entfernen {
    pk: String,
}

pub async fn entfernen(State(st): State<Arc<AppState>>, h: HeaderMap, Json(g): Json<Entfernen>) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let n = st.db.0.lock().unwrap().execute("DELETE FROM verbund_geraete WHERE pk=?1 AND account_id=?2", rusqlite::params![g.pk, acc]).unwrap_or(0);
    Json(json!({"entfernt":n})).into_response()
}

pub async fn geraete(State(st): State<Arc<AppState>>, h: HeaderMap) -> impl IntoResponse {
    let Some(acc) = bearer(&h).and_then(|t| auth::verify_token(&st.secret, t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let online: Vec<String> = st.verbund.0.lock().unwrap().konten.get(&acc).map(|v| v.iter().map(|c| c.pk.clone()).collect()).unwrap_or_default();
    let conn = st.db.0.lock().unwrap();
    let mut s = conn.prepare("SELECT pk,name,ausgabe,zuletzt FROM verbund_geraete WHERE account_id=?1 ORDER BY name").unwrap();
    let liste: Vec<serde_json::Value> = s
        .query_map(rusqlite::params![acc], |r| {
            let pk: String = r.get(0)?;
            Ok(json!({"pk":pk,"name":r.get::<_,String>(1)?,"ausgabe":r.get::<_,String>(2)?,"zuletzt":r.get::<_,i64>(3)?,"online":online.contains(&pk)}))
        })
        .unwrap()
        .flatten()
        .collect();
    Json(json!({"geraete":liste})).into_response()
}

pub async fn relais_ws(State(st): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.max_message_size(MAX_RAHMEN).max_frame_size(MAX_RAHMEN).on_upgrade(move |s| relais(s, st))
}

impl Hub {
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

    pub fn anzahl(&self) -> usize {
        self.0.lock().unwrap().konten.values().map(|v| v.len()).sum()
    }
}

fn rahmen(typ: u8, pk: &[u8], s: &[u8; 8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(41);
    b.push(typ);
    b.extend_from_slice(pk);
    b.extend_from_slice(s);
    b
}

async fn relais(socket: WebSocket, st: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();
    // Anmeldung
    let anm = match tokio::time::timeout(std::time::Duration::from_secs(10), rx.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<serde_json::Value>(&t).unwrap_or_default(),
        _ => return,
    };
    let acc = match auth::verify_token(&st.secret, anm["token"].as_str().unwrap_or("")) {
        Some(a) => a,
        None => {
            let _ = tx.send(Message::Text(r#"{"typ":"fehler","text":"anmeldung"}"#.into())).await;
            return;
        }
    };
    let pk = anm["pk"].as_str().unwrap_or("").to_string();
    let name = anm["name"].as_str().unwrap_or("").to_string();
    let ausgabe = anm["ausgabe"].as_str().unwrap_or("").to_string();
    let rolle = if anm["rolle"] == "dienst" { "dienst" } else { "client" }.to_string();
    if let Err(e) = eintragen(&st, acc, &pk, &name, &ausgabe) {
        let _ = tx.send(Message::Text(json!({"typ":"fehler","text":format!("gerät: {e}")}).to_string())).await;
        return;
    }
    let pk_roh = hex::decode(&pk).unwrap_or_default();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(PUFFER);
    let id = st.verbund.1.fetch_add(1, Ordering::SeqCst) + 1;
    let zu_viele = {
        let mut i = st.verbund.0.lock().unwrap();
        let v = i.konten.entry(acc).or_default();
        if v.len() >= MAX_JE_KONTO {
            true
        } else {
            // ein Dienst je Gerät: alte Dienst-Verbindung desselben Schlüssels ersetzen
            if rolle == "dienst" {
                v.retain(|c| !(c.pk == pk && c.rolle == "dienst"));
            }
            v.push(Verb { id, pk: pk.clone(), rolle: rolle.clone(), name, ausgabe, kerne: anm["kerne"].as_u64().unwrap_or(0), frei: anm["frei"].as_u64().unwrap_or(0), tx: out_tx.clone() });
            let _ = out_tx.try_send(Message::Text(r#"{"typ":"bereit"}"#.into()));
            st.verbund.liste_senden(&i, acc);
            false
        }
    };
    if zu_viele {
        let _ = tx.send(Message::Text(r#"{"typ":"fehler","text":"zu viele Verbindungen"}"#.into())).await;
        return;
    }
    tracing::info!("verbund: {} ({rolle}) Konto {acc} verbunden", &pk[..8]);

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
            Message::Binary(b) => {
                if b.len() < 41 {
                    continue;
                }
                ZAEHLER.rahmen.fetch_add(1, Ordering::Relaxed);
                let typ = b[0];
                let ziel = hex::encode(&b[1..33]);
                let mut s = [0u8; 8];
                s.copy_from_slice(&b[33..41]);
                let mut neu = b.clone();
                neu[1..33].copy_from_slice(&pk_roh);
                let ziel_tx = {
                    let mut i = st.verbund.0.lock().unwrap();
                    // Antwort auf eine Sitzung, die das Ziel geöffnet hat?
                    let rueck = i.sitzungen.get(&(ziel.clone(), pk.clone(), s)).copied();
                    if typ == OEFFNEN {
                        i.sitzungen.insert((pk.clone(), ziel.clone(), s), id);
                        if i.sitzungen.len() > 100_000 {
                            i.sitzungen.clear();
                        }
                    }
                    if typ == SCHLIESSEN || typ == UNERREICHBAR {
                        i.sitzungen.remove(&(pk.clone(), ziel.clone(), s));
                        i.sitzungen.remove(&(ziel.clone(), pk.clone(), s));
                    }
                    let v = i.konten.get(&acc);
                    v.and_then(|v| match rueck {
                        Some(rid) => v.iter().find(|c| c.id == rid).map(|c| c.tx.clone()),
                        None => v.iter().find(|c| c.pk == ziel && c.rolle == "dienst").map(|c| c.tx.clone()),
                    })
                };
                let ok = match ziel_tx {
                    Some(t) => t.try_send(Message::Binary(neu)).is_ok(),
                    None => false,
                };
                if !ok && typ != SCHLIESSEN && typ != UNERREICHBAR {
                    ZAEHLER.verworfen.fetch_add(1, Ordering::Relaxed);
                    let _ = out_tx.try_send(Message::Binary(rahmen(UNERREICHBAR, &b[1..33], &s)));
                }
            }
            Message::Text(t) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    if v["typ"] == "frei" {
                        let mut i = st.verbund.0.lock().unwrap();
                        if let Some(c) = i.konten.get_mut(&acc).and_then(|v| v.iter_mut().find(|c| c.id == id)) {
                            c.frei = v["frei"].as_u64().unwrap_or(0);
                        }
                        st.verbund.liste_senden(&i, acc);
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    schreiber.abort();
    let mut i = st.verbund.0.lock().unwrap();
    if let Some(v) = i.konten.get_mut(&acc) {
        v.retain(|c| c.id != id);
        if v.is_empty() {
            i.konten.remove(&acc);
        }
    }
    i.sitzungen.retain(|_, v| *v != id);
    st.verbund.liste_senden(&i, acc);
    tracing::info!("verbund: {} ({rolle}) getrennt", &pk[..8]);
}

pub async fn statistik(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({"verbindungen":st.verbund.anzahl(),"rahmen":ZAEHLER.rahmen.load(Ordering::Relaxed),"verworfen":ZAEHLER.verworfen.load(Ordering::Relaxed)}))
}
