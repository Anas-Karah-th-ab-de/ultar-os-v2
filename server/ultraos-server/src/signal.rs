//! Call/video signalling: a pure relay for WebRTC (SDP + ICE).
//! The server never sees media — they run end to end over DTLS-SRTP.
use crate::{auth, AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::mpsc;

/// Largest accepted signalling message in bytes.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Messages queued per socket; a full queue reports the peer as unreachable.
const QUEUE: usize = 256;
/// Time a new socket has to authenticate.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Number -> sender to the socket of that number.
#[derive(Default, Clone)]
pub struct Hub(pub Arc<Mutex<HashMap<String, mpsc::Sender<Message>>>>);

impl Hub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, mpsc::Sender<Message>>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Checks the first message `{"token":"..","number":"UOS-..-.."}`: the token must be valid and
/// the number must belong to its account.
fn login(st: &AppState, first: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(first).ok()?;
    let token = v.get("token")?.as_str()?;
    let number = v.get("number")?.as_str()?;
    let acc = auth::verify_token(&st.secret, token)?;
    let owns = st
        .db
        .lock()
        .query_row(
            "SELECT 1 FROM numbers WHERE number=?1 AND account_id=?2",
            rusqlite::params![number, acc],
            |_| Ok(()),
        )
        .optional()
        .ok()?
        .is_some();
    owns.then(|| number.to_string())
}

/// Forwards `{"to":"UOS-..","type":"offer|answer|ice|bye","data":..}` to its target. The sender
/// number is written by the server (no spoofing).
fn forward(st: &AppState, from: &str, own: &mpsc::Sender<Message>, text: &str) {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(to) = v.get("to").and_then(|x| x.as_str()).map(str::to_string) else {
        return;
    };
    v["from"] = serde_json::Value::String(from.to_string());
    let target = st.hub.lock().get(&to).cloned();
    let delivered = target.is_some_and(|peer| peer.try_send(Message::Text(v.to_string())).is_ok());
    if !delivered {
        let note = serde_json::json!({"type":"unreachable","to":to}).to_string();
        let _ = own.try_send(Message::Text(note));
    }
}

/// Runs one signalling socket until it closes.
pub async fn handle(socket: WebSocket, st: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();
    let first = match tokio::time::timeout(LOGIN_TIMEOUT, rx.next()).await {
        Ok(Some(Ok(Message::Text(first)))) => first,
        _ => String::new(),
    };
    let Some(my_number) = login(&st, &first) else {
        let _ = tx
            .send(Message::Text(r#"{"type":"error","error":"auth"}"#.into()))
            .await;
        return;
    };
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(QUEUE);
    st.hub.lock().insert(my_number.clone(), out_tx.clone());
    let _ = out_tx.try_send(Message::Text(r#"{"type":"ready"}"#.into()));

    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if tx.send(m).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = rx.next().await {
        match msg {
            Message::Text(t) => forward(&st, &my_number, &out_tx, &t),
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Sign off, unless a newer socket of the same number has taken the entry.
    {
        let mut hub = st.hub.lock();
        if hub
            .get(&my_number)
            .is_some_and(|current| current.same_channel(&out_tx))
        {
            hub.remove(&my_number);
        }
    }
    writer.abort();
}
