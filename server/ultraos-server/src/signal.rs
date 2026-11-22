//! Anruf-/Video-Signalisierung: reiner Vermittler für WebRTC (SDP + ICE).
//! Der Server sieht die Medien nie — die laufen Ende-zu-Ende per DTLS-SRTP.
use crate::{auth, AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Nummer -> Sender zum jeweiligen Client-Socket.
#[derive(Default, Clone)]
pub struct Hub(pub Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>>);

pub async fn handle(socket: WebSocket, st: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    // erste Nachricht = Anmeldung: {"token":"..","number":"UOS-..-.."}
    let mut my_number: Option<String> = None;
    if let Some(Ok(Message::Text(first))) = rx.next().await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&first) {
            let token = v.get("token").and_then(|x| x.as_str()).unwrap_or("");
            let number = v.get("number").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if let Some(acc) = auth::verify_token(&st.secret, token) {
                // prüfen, dass die Nummer wirklich dem Konto gehört
                let owns = {
                    let conn = st.db.0.lock().unwrap();
                    conn.query_row(
                        "SELECT 1 FROM numbers WHERE number=?1 AND account_id=?2",
                        rusqlite::params![number, acc],
                        |_| Ok(()),
                    )
                    .is_ok()
                };
                if owns {
                    st.hub.0.lock().unwrap().insert(number.clone(), out_tx.clone());
                    my_number = Some(number);
                    let _ = out_tx.send(Message::Text(r#"{"type":"ready"}"#.into()));
                }
            }
        }
    }
    let my_number = match my_number {
        Some(n) => n,
        None => {
            let _ = tx.send(Message::Text(r#"{"type":"error","error":"auth"}"#.into())).await;
            return;
        }
    };

    // Ausgehende Nachrichten an diesen Socket schreiben
    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if tx.send(m).await.is_err() {
                break;
            }
        }
    });

    // Eingehende Nachrichten weiterleiten: {"to":"UOS-..","type":"offer|answer|ice|bye","data":..}
    while let Some(Ok(msg)) = rx.next().await {
        if let Message::Text(t) = msg {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&t) {
                if let Some(to) = v.get("to").and_then(|x| x.as_str()).map(|s| s.to_string()) {
                    // Absender authentisch eintragen (kein Spoofing)
                    v["from"] = serde_json::Value::String(my_number.clone());
                    let target = st.hub.0.lock().unwrap().get(&to).cloned();
                    if let Some(peer) = target {
                        let _ = peer.send(Message::Text(v.to_string()));
                    } else {
                        let _ = out_tx.send(Message::Text(
                            serde_json::json!({"type":"unreachable","to":to}).to_string(),
                        ));
                    }
                }
            }
        } else if let Message::Close(_) = msg {
            break;
        }
    }

    // Abmelden
    st.hub.0.lock().unwrap().remove(&my_number);
    writer.abort();
}
