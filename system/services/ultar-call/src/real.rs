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
    Register { number: String, token: String, server: String },
    Place { peer: String, video: bool },
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
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
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
    fn register(&mut self, number: &str, token: &str, server_base: &str) {
        let (n, t, s) = (number.to_string(), token.to_string(), server_base.to_string());
        let _ = self.ensure_worker().send(Cmd::Register { number: n, token: t, server: s });
    }
    fn place(&mut self, peer: &str, video: bool) {
        *self.state.lock().unwrap() = CallState::Dialing(peer.to_string());
        let _ = self.ensure_worker().send(Cmd::Place { peer: peer.to_string(), video });
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

fn ws_url(server_base: &str) -> String {
    let b = server_base.replace("https://", "wss://").replace("http://", "ws://");
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
    let (number, token, server) = loop {
        match rx.recv().await {
            Some(Cmd::Register { number, token, server }) => break (number, token, server),
            Some(_) => continue,
            None => return,
        }
    };

    let url = ws_url(&server);
    let (ws, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(x) => x,
        Err(e) => {
            evt.lock().unwrap().push(CallEvent::Error(format!("Signalserver: {e}")));
            return;
        }
    };
    let (mut sink, mut stream) = ws.split();

    // Anmeldung
    let auth = serde_json::json!({"token": token, "number": number}).to_string();
    if sink.send(Message::Text(auth)).await.is_err() {
        evt.lock().unwrap().push(CallEvent::Error("Anmeldung fehlgeschlagen".into()));
        return;
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
                                "offer" => {
                                    // eingehender Anruf
                                    current_peer = Some(from.clone());
                                    *state.lock().unwrap() = CallState::Incoming(from.clone());
                                    evt.lock().unwrap().push(CallEvent::Incoming(from));
                                    // Medien-Engine: hier RTCPeerConnection.set_remote(offer) + Answer vorbereiten
                                }
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
