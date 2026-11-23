//! Call/video signalling: a pure relay for WebRTC (SDP + ICE).
//! The server never sees media — they run end to end over DTLS-SRTP.
//!
//! A socket is opened with the **device key**, not with an account token: the device names its
//! key id, the server answers with a fresh challenge, the device signs the transcript
//! `ultar-device-signal-v1` (see `docs/DEVICE-IDENTITY.md` §5.4). The socket then serves exactly
//! the number of that key. There is one socket per number: a new sign-in of the device closes its
//! previous socket, and removing or replacing the device closes the socket at once. A socket that
//! is not the hub's current one for its number relays nothing.

use crate::devices::{self, DeviceState};
use crate::identity::{self, Purpose, Transcript, CHALLENGE_BYTES, LOG_PREFIX_LEN};
use crate::AppState;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

/// Largest accepted signalling message in bytes.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Messages queued per socket; a full queue reports the peer as unreachable.
const QUEUE: usize = 256;
/// Time a new socket has for each step of its authentication.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10);
/// Time a closed socket gets to deliver its last messages.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

struct Peer {
    sender: mpsc::Sender<Message>,
    account_id: i64,
    /// True when the device may manage the account's devices (it then gets security notices).
    manager: bool,
    stop: Arc<Notify>,
}

/// Number -> the one socket of the device that holds this number.
#[derive(Default, Clone)]
pub struct Hub(Arc<Mutex<HashMap<String, Peer>>>);

impl Hub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Peer>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// True when a socket is connected for `number`.
    pub fn is_online(&self, number: &str) -> bool {
        self.lock().contains_key(number)
    }

    /// Makes `peer` the socket of `number`. A socket that held the number before is closed: there
    /// is exactly one socket per number, so removing or replacing a device closes all it has.
    fn enter(&self, number: &str, peer: Peer) {
        if let Some(previous) = self.lock().insert(number.to_string(), peer) {
            let frame = CloseFrame {
                code: axum::extract::ws::close_code::POLICY,
                reason: "signed_in_elsewhere".into(),
            };
            let _ = previous.sender.try_send(Message::Close(Some(frame)));
            previous.stop.notify_one();
        }
    }

    /// True while `sender` is the socket the hub holds for `number`.
    fn is_current(&self, number: &str, sender: &mpsc::Sender<Message>) -> bool {
        self.lock()
            .get(number)
            .is_some_and(|peer| peer.sender.same_channel(sender))
    }

    /// Closes the socket of `number`, if any (its device was removed or replaced).
    pub fn kick(&self, number: &str) {
        if let Some(peer) = self.lock().remove(number) {
            let note = r#"{"type":"error","error":"device_removed"}"#;
            let _ = peer.sender.try_send(Message::Text(note.into()));
            peer.stop.notify_one();
        }
    }

    /// Pushes a security notice to every connected managing device of an account.
    pub fn notify_account(&self, account_id: i64, event: &str, device_prefix: &str) {
        let note =
            serde_json::json!({"type":"notice","event":event,"device":device_prefix}).to_string();
        let hub = self.lock();
        for peer in hub
            .values()
            .filter(|p| p.account_id == account_id && p.manager)
        {
            let _ = peer.sender.try_send(Message::Text(note.clone()));
        }
    }
}

/// An authenticated socket that has taken its place in the hub.
struct Authed {
    number: String,
    out_tx: mpsc::Sender<Message>,
    out_rx: mpsc::Receiver<Message>,
    stop: Arc<Notify>,
}

type Sink = SplitSink<WebSocket, Message>;
type Stream = SplitStream<WebSocket>;

async fn next_json(rx: &mut Stream) -> Option<serde_json::Value> {
    match tokio::time::timeout(LOGIN_TIMEOUT, rx.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text).ok(),
        _ => None,
    }
}

/// The active device with this key id: its row, account and public key.
fn active_device(st: &AppState, device_id: &str) -> Option<(i64, i64, [u8; 32])> {
    if !identity::is_device_id(device_id) {
        return None;
    }
    let device = devices::by_id(&st.db.lock(), device_id).ok()??;
    let key = device.public_key?;
    (device.state == DeviceState::Active).then_some((device.row, device.account_id, key))
}

/// Runs the handshake `hello` → `challenge` → `auth`. The error is the code sent to the client:
/// `device_key_required` for the first-release frame, `auth` for everything else (no detail).
async fn authenticate(
    st: &AppState,
    tx: &mut Sink,
    rx: &mut Stream,
) -> Result<Authed, &'static str> {
    let hello = next_json(rx).await.ok_or("auth")?;
    if hello.get("token").is_some() {
        return Err("device_key_required");
    }
    let device_id = hello
        .get("device_id")
        .and_then(|v| v.as_str())
        .filter(|_| hello.get("type").and_then(|t| t.as_str()) == Some("hello"))
        .ok_or("auth")?;
    let (row, account_id, key) = active_device(st, device_id).ok_or("auth")?;
    let mut challenge = [0u8; CHALLENGE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut challenge);
    let server = st.devices.config.server_name.as_str();
    let frame = serde_json::json!({
        "type": "challenge",
        "challenge": identity::encode(&challenge),
        "server": server,
        "account_id": account_id,
    });
    tx.send(Message::Text(frame.to_string()))
        .await
        .map_err(|_| "auth")?;
    let answer = next_json(rx).await.ok_or("auth")?;
    let signature = answer
        .get("signature")
        .and_then(|v| v.as_str())
        .filter(|_| answer.get("type").and_then(|t| t.as_str()) == Some("auth"))
        .and_then(|s| identity::parse_signature(s).ok())
        .ok_or("auth")?;
    let transcript = Transcript {
        purpose: Purpose::Signal,
        server,
        account_id,
        challenge: &challenge,
        subject_key: &key,
        extra: b"",
    };
    transcript.verify(&signature).map_err(|_| "auth")?;
    confirm(st, device_id, row, account_id)
}

/// Last step of the handshake, all of it under the database lock: the device may have been
/// removed while it was signing, so its state is read again; its number is looked up; the socket
/// takes its place in the hub. A removal either happened before (the sign-in fails) or comes
/// after (it finds the socket in the hub and closes it) – never in between.
fn confirm(
    st: &AppState,
    device_id: &str,
    row: i64,
    account_id: i64,
) -> Result<Authed, &'static str> {
    let conn = st.db.lock();
    let now = st.devices.now_secs();
    let still = devices::by_id(&conn, device_id)
        .ok()
        .flatten()
        .filter(|d| d.row == row && d.state == DeviceState::Active)
        .ok_or("auth")?;
    let number = devices::number_of(&conn, still.row)
        .ok()
        .flatten()
        .ok_or("auth")?;
    let _ = devices::touch(&conn, still.row, now);
    let manage_in = still.manage_in(now, st.devices.config.recovery_cooldown_secs);
    let recovery = crate::device_moves::recovery_info(&conn, account_id, now)
        .ok()
        .flatten()
        .map(|r| serde_json::json!({"old_device": r.old_device, "eligible_in": r.eligible_in}));
    let (out_tx, out_rx) = mpsc::channel::<Message>(QUEUE);
    let stop = Arc::new(Notify::new());
    let ready = serde_json::json!({
        "type": "ready",
        "number": number,
        "manage": manage_in == Some(0),
        "recovery": recovery,
    });
    let _ = out_tx.try_send(Message::Text(ready.to_string()));
    let peer = Peer {
        sender: out_tx.clone(),
        account_id,
        manager: manage_in == Some(0),
        stop: stop.clone(),
    };
    st.hub.enter(&number, peer);
    tracing::info!(
        event = "signal_connected",
        device = %identity::id_prefix(device_id, LOG_PREFIX_LEN)
    );
    Ok(Authed {
        number,
        out_tx,
        out_rx,
        stop,
    })
}

/// Forwards `{"to":"UOS-..","type":"offer|answer|ice|bye","data":..}` to its target. The sender
/// number is written by the server (no spoofing). Only the socket the hub holds for the number
/// may speak under it; `false` tells the caller that this socket is no longer that one.
fn forward(st: &AppState, from: &str, own: &mpsc::Sender<Message>, text: &str) -> bool {
    if !st.hub.is_current(from, own) {
        return false;
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) else {
        return true;
    };
    let Some(to) = v.get("to").and_then(|x| x.as_str()).map(str::to_string) else {
        return true;
    };
    v["from"] = serde_json::Value::String(from.to_string());
    let target = st.hub.lock().get(&to).map(|p| p.sender.clone());
    let delivered = target.is_some_and(|peer| peer.try_send(Message::Text(v.to_string())).is_ok());
    if !delivered {
        let note = serde_json::json!({"type":"unreachable","to":to}).to_string();
        let _ = own.try_send(Message::Text(note));
    }
    true
}

/// Relays the socket's messages until it closes or loses its place in the hub (its device was
/// removed or replaced, or the device signed in again).
async fn relay(
    st: &AppState,
    number: &str,
    out_tx: &mpsc::Sender<Message>,
    stop: &Notify,
    rx: &mut Stream,
) {
    loop {
        tokio::select! {
            () = stop.notified() => return,
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if !forward(st, number, out_tx, &t) {
                        return;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            }
        }
    }
}

/// Runs one signalling socket until it closes.
pub async fn handle(socket: WebSocket, st: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();
    let me = match authenticate(&st, &mut tx, &mut rx).await {
        Ok(me) => me,
        Err(code) => {
            let note = serde_json::json!({"type":"error","error":code}).to_string();
            let _ = tx.send(Message::Text(note)).await;
            return;
        }
    };
    let Authed {
        number,
        out_tx,
        mut out_rx,
        stop,
    } = me;
    let mut writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            let last = matches!(m, Message::Close(_));
            if tx.send(m).await.is_err() || last {
                break;
            }
        }
    });

    relay(&st, &number, &out_tx, &stop, &mut rx).await;

    // Sign off, unless a newer socket of the same number has taken the entry.
    {
        let mut hub = st.hub.lock();
        if hub
            .get(&number)
            .is_some_and(|current| current.sender.same_channel(&out_tx))
        {
            hub.remove(&number);
        }
    }
    drop(out_tx);
    // Let what is queued (a last notice, the close frame) reach the device, then hang up.
    if tokio::time::timeout(FLUSH_TIMEOUT, &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}
