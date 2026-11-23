//! Echter Signalisierungs-Client (tokio-tungstenite) für Anrufe über den Cloud-Server.
//!
//! Dieser Pfad macht den serverseitig gekoppelten Teil vollständig: WebSocket zu /v1/signal,
//! Anmeldung mit Konto-Token + eigener Nummer, Austausch von Offer/Answer/ICE/Bye, Klingeln,
//! Annehmen, Auflegen und Erreichbarkeit. Die Medien-Engine (webrtc, DTLS-SRTP, Opus/VP8 mit
//! Gerätemikrofon/-kamera/-lautsprecher) wird hier eingehängt: die SDP/ICE, die unten als Platzhalter
//! erzeugt bzw. verarbeitet werden, stammen dann aus der RTCPeerConnection. Siehe docs/A30-ANRUFE.md.

use crate::{CallBackend, CallEvent, CallState};
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

enum Cmd {
    Register {
        server: String,
        auth: Box<dyn crate::SignalAuth>,
    },
    Place {
        peer: String,
        video: bool,
    },
    Answer,
    Hangup,
}

pub struct WebrtcCall {
    tx: Option<mpsc::UnboundedSender<Cmd>>,
    evt: Arc<Mutex<Vec<CallEvent>>>,
    state: Arc<Mutex<CallState>>,
    ready: Arc<Mutex<bool>>,
}

impl Default for WebrtcCall {
    fn default() -> Self {
        Self {
            tx: None,
            evt: Arc::new(Mutex::new(Vec::new())),
            state: Arc::new(Mutex::new(CallState::Idle)),
            ready: Arc::new(Mutex::new(false)),
        }
    }
}

impl WebrtcCall {
    fn ensure_worker(&mut self) -> &mpsc::UnboundedSender<Cmd> {
        if self.tx.is_none() {
            let (tx, rx) = mpsc::unbounded_channel::<Cmd>();
            let evt = self.evt.clone();
            let state = self.state.clone();
            let ready = self.ready.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(_) => return,
                };
                rt.block_on(worker(rx, evt, state, ready));
            });
            self.tx = Some(tx);
        }
        self.tx.as_ref().unwrap()
    }
}

impl CallBackend for WebrtcCall {
    fn register(&mut self, server_base: &str, auth: Box<dyn crate::SignalAuth>) {
        let _ = self.ensure_worker().send(Cmd::Register {
            server: server_base.to_string(),
            auth,
        });
    }
    fn place(&mut self, peer: &str, video: bool) {
        *self.state.lock().unwrap() = CallState::Dialing(peer.to_string());
        let _ = self.ensure_worker().send(Cmd::Place {
            peer: peer.to_string(),
            video,
        });
    }
    fn answer(&mut self) {
        let _ = self.ensure_worker().send(Cmd::Answer);
    }
    fn hangup(&mut self) {
        *self.state.lock().unwrap() = CallState::Ended;
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Hangup);
        }
    }
    fn poll(&mut self) -> Vec<CallEvent> {
        std::mem::take(&mut *self.evt.lock().unwrap())
    }
    fn state(&self) -> CallState {
        self.state.lock().unwrap().clone()
    }
    fn ready(&self) -> bool {
        *self.ready.lock().unwrap()
    }
}

fn push_event(evt: &Arc<Mutex<Vec<CallEvent>>>, event: CallEvent) {
    if let Ok(mut events) = evt.lock() {
        events.push(event);
    }
}

fn text_field(v: &serde_json::Value, name: &str) -> String {
    v.get(name)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .chars()
        .take(64)
        .collect()
}

/// A `notice` frame of the server as an event for the user interface.
fn notice_event(v: &serde_json::Value) -> CallEvent {
    CallEvent::Notice {
        event: text_field(v, "event"),
        device: text_field(v, "device"),
    }
}

/// The pending recovery a `ready` frame reports, as the notice the device may have missed.
fn pending_recovery(ready: &serde_json::Value) -> Option<CallEvent> {
    let recovery = ready.get("recovery").filter(|r| r.is_object())?;
    Some(CallEvent::Notice {
        event: "recovery_started".to_string(),
        device: text_field(recovery, "old_device").chars().take(8).collect(),
    })
}

/// True when a frame that ends or answers a call comes from the party of the current call
/// (`unreachable` is the server's own frame about the number that was dialled).
fn from_current_peer(
    ty: &str,
    v: &serde_json::Value,
    from: &str,
    current_peer: &Option<String>,
) -> bool {
    let named = if ty == "unreachable" {
        text_field(v, "to")
    } else {
        from.to_string()
    };
    current_peer.as_deref() == Some(named.as_str())
}

fn ws_url(server_base: &str) -> String {
    let b = server_base
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    format!("{}/v1/signal", b.trim_end_matches('/'))
}

async fn worker(
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    evt: Arc<Mutex<Vec<CallEvent>>>,
    state: Arc<Mutex<CallState>>,
    ready: Arc<Mutex<bool>>,
) {
    use tokio_tungstenite::tungstenite::Message;

    // auf Register warten
    let (server, auth) = loop {
        match rx.recv().await {
            Some(Cmd::Register { server, auth }) => break (server, auth),
            Some(_) => continue,
            None => return,
        }
    };

    let url = ws_url(&server);
    let (ws, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(x) => x,
        Err(e) => {
            evt.lock()
                .unwrap()
                .push(CallEvent::Error(format!("Signalserver: {e}")));
            return;
        }
    };
    let (mut sink, mut stream) = ws.split();

    // Sign-in with the device key: hello -> challenge -> auth -> ready. Errors are reported as codes.
    let fail = |code: &str| {
        if let Ok(mut events) = evt.lock() {
            events.push(CallEvent::Error(code.to_string()));
        }
    };
    let wait = std::time::Duration::from_secs(15);
    let hello = match auth.hello() {
        Ok(h) => h,
        Err(code) => return fail(&code),
    };
    if sink.send(Message::Text(hello)).await.is_err() {
        return fail("signal_unreachable");
    }
    let challenge = match tokio::time::timeout(wait, stream.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        _ => return fail("signal_no_answer"),
    };
    let answer = match auth.answer(&challenge) {
        Ok(a) => a,
        Err(code) => return fail(&code),
    };
    if sink.send(Message::Text(answer)).await.is_err() {
        return fail("signal_unreachable");
    }
    match tokio::time::timeout(wait, stream.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            if v.get("type").and_then(|x| x.as_str()) != Some("ready") {
                return fail(
                    v.get("error")
                        .and_then(|x| x.as_str())
                        .unwrap_or("signal_refused"),
                );
            }
            // A recovery that is pending while the device signs in is a state, not a message it
            // could have missed: report it like the notice.
            if let Some(notice) = pending_recovery(&v) {
                push_event(&evt, notice);
            }
        }
        _ => return fail("signal_no_answer"),
    }
    *ready.lock().unwrap() = true;

    let mut current_peer: Option<String> = None;

    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                            let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                            let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            match ty {
                                "ready" => {}
                                "notice" => push_event(&evt, notice_event(&v)),
                                "error" => {
                                    let code = v.get("error").and_then(|x| x.as_str()).unwrap_or("signal_error");
                                    fail(code);
                                    break;
                                }
                                "offer" => {
                                    // eingehender Anruf
                                    current_peer = Some(from.clone());
                                    *state.lock().unwrap() = CallState::Incoming(from.clone());
                                    evt.lock().unwrap().push(CallEvent::Incoming(from));
                                    // Medien-Engine: hier RTCPeerConnection.set_remote(offer) + Answer vorbereiten
                                }
                                // Answer, hang-up and "unreachable" count only from the party of
                                // the current call; anybody may send frames to this number.
                                "answer" | "bye" | "unreachable"
                                    if !from_current_peer(ty, &v, &from, &current_peer) => {}
                                "answer" => {
                                    if let CallState::Dialing(p) = state.lock().unwrap().clone() {
                                        *state.lock().unwrap() = CallState::Active(p);
                                    }
                                    evt.lock().unwrap().push(CallEvent::Answered);
                                    // Medien-Engine: set_remote(answer)
                                }
                                "ice" => { /* Medien-Engine: add_ice_candidate(v["data"]) */ }
                                "bye" | "unreachable" => {
                                    *state.lock().unwrap() = CallState::Ended;
                                    evt.lock().unwrap().push(CallEvent::Ended);
                                    current_peer = None;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(Cmd::Place { peer, video: _ }) => {
                        current_peer = Some(peer.clone());
                        // Medien-Engine: Offer erzeugen; hier Platzhalter-SDP
                        let offer = serde_json::json!({"to": peer, "type":"offer", "data": {"sdp":"<offer>"}}).to_string();
                        let _ = sink.send(Message::Text(offer)).await;
                        evt.lock().unwrap().push(CallEvent::Ringing);
                    }
                    Some(Cmd::Answer) => {
                        if let Some(peer) = current_peer.clone() {
                            if let CallState::Incoming(n) = state.lock().unwrap().clone() {
                                *state.lock().unwrap() = CallState::Active(n);
                            }
                            // Medien-Engine: Answer erzeugen; hier Platzhalter
                            let ans = serde_json::json!({"to": peer, "type":"answer", "data":{"sdp":"<answer>"}}).to_string();
                            let _ = sink.send(Message::Text(ans)).await;
                            evt.lock().unwrap().push(CallEvent::Answered);
                        }
                    }
                    Some(Cmd::Hangup) => {
                        if let Some(peer) = current_peer.take() {
                            let bye = serde_json::json!({"to": peer, "type":"bye"}).to_string();
                            let _ = sink.send(Message::Text(bye)).await;
                        }
                        *state.lock().unwrap() = CallState::Ended;
                        evt.lock().unwrap().push(CallEvent::Ended);
                    }
                    Some(Cmd::Register { .. }) => {}
                    None => break,
                }
            }
        }
    }
    *ready.lock().unwrap() = false;
}
