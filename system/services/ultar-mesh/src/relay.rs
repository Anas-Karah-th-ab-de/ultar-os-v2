//! Devices that are not in the same network reach each other through the relay of the
//! Ultar account server. The relay forwards frames between devices of the same account
//! and sees ciphertext only: the Noise channel runs end to end inside it, and a device
//! met through the relay is paired with a code like any other – the server is not trusted
//! to say who belongs to the person.
//!
//! Frame (binary): `[type:1][key:32][session:8][payload]`, type 1 = open, 2 = data,
//! 3 = close, 4 = unreachable. The sender writes the target into `key`; the relay replaces
//! it with the sender. The login is one JSON text; its field names are those of the server.

use crate::secure::{Rx, Tx};
use rand::Rng;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const OPEN: u8 = 1;
const DATA: u8 = 2;
const CLOSE: u8 = 3;
const UNREACHABLE: u8 = 4;
const HEAD: usize = 41;
const POLL: Duration = Duration::from_millis(50);
const MAX_WAIT: u64 = 30;

/// How this device signs in at the relay.
#[derive(Clone, Debug)]
pub struct Login {
    /// Address of the relay (`wss://…`).
    pub url: String,
    /// The account's device token.
    pub token: String,
    /// This device's name (data).
    pub name: String,
    /// Its kind as a word.
    pub kind: String,
}

type Session = (Vec<u8>, [u8; 8]);
type Inbox = mpsc::Sender<Option<Vec<u8>>>;
/// A connection through the relay: its two halves.
pub type Halves = (Box<dyn Tx>, Box<dyn Rx>);

/// The connection to the relay.
pub struct Relay {
    out: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    sessions: Mutex<HashMap<Session, Inbox>>,
    devices: Mutex<Vec<Vec<u8>>>,
    stop: AtomicBool,
}

fn frame(kind: u8, key: &[u8], session: &[u8; 8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEAD + payload.len());
    bytes.push(kind);
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(session);
    bytes.extend_from_slice(payload);
    bytes
}

struct Out {
    relay: Arc<Relay>,
    session: Session,
}

impl Tx for Out {
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let (key, session) = &self.session;
        self.relay.write(frame(DATA, key, session, bytes))
    }

    fn close(&mut self) {
        let (key, session) = &self.session;
        let _ = self.relay.write(frame(CLOSE, key, session, &[]));
        if let Ok(mut sessions) = self.relay.sessions.lock() {
            if let Some(inbox) = sessions.remove(&self.session) {
                let _ = inbox.send(None);
            }
        }
    }
}

struct In {
    inbox: mpsc::Receiver<Option<Vec<u8>>>,
    limit: Option<Duration>,
}

impl Rx for In {
    fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let got = match self.limit {
            Some(time) => self.inbox.recv_timeout(time).ok(),
            None => self.inbox.recv().ok(),
        };
        got.flatten()
            .ok_or_else(|| std::io::ErrorKind::ConnectionReset.into())
    }

    fn limit(&mut self, time: Option<Duration>) {
        self.limit = time;
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() != 64 {
        return None;
    }
    (0..32)
        .map(|i| u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

fn waiting(error: &tungstenite::Error) -> bool {
    use std::io::ErrorKind::{TimedOut, WouldBlock};
    matches!(error, tungstenite::Error::Io(e) if matches!(e.kind(), WouldBlock | TimedOut))
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn poll(socket: &mut Socket) {
    let time = Some(POLL);
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(time);
            let _ = s.set_nodelay(true);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_read_timeout(time);
            let _ = s.get_mut().set_nodelay(true);
        }
        _ => {}
    }
}

impl Relay {
    fn write(&self, bytes: Vec<u8>) -> std::io::Result<()> {
        let gone = || std::io::Error::from(std::io::ErrorKind::NotConnected);
        let out = self.out.lock().map_err(|_| gone())?;
        out.as_ref()
            .ok_or_else(gone)?
            .send(bytes)
            .map_err(|_| gone())
    }

    fn halves(self: &Arc<Self>, session: Session) -> Option<Halves> {
        let (inbox, rx) = mpsc::channel();
        self.sessions.lock().ok()?.insert(session.clone(), inbox);
        let out = Out {
            relay: self.clone(),
            session,
        };
        let rx = In {
            inbox: rx,
            limit: None,
        };
        Some((Box::new(out), Box::new(rx)))
    }

    /// Opens a connection to the device with the static key `key` through the relay.
    pub fn open(self: &Arc<Self>, key: &[u8]) -> Option<Halves> {
        let mut session = [0u8; 8];
        rand::thread_rng().fill(&mut session);
        let halves = self.halves((key.to_owned(), session))?;
        self.write(frame(OPEN, key, &session, &[])).ok()?;
        Some(halves)
    }

    /// The static keys of the account's other devices that are at the relay now.
    pub fn devices(&self) -> Vec<Vec<u8>> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    /// Leaves the relay for good.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn text(&self, text: &str, own: &[u8]) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        if value["typ"] != "geraete" {
            return;
        }
        let list = value["liste"].as_array().cloned().unwrap_or_default();
        let keys: Vec<Vec<u8>> = list
            .iter()
            .filter_map(|d| unhex(d["pk"].as_str()?))
            .filter(|key| key != own)
            .collect();
        if let Ok(mut devices) = self.devices.lock() {
            *devices = keys;
        }
    }

    fn binary(self: &Arc<Self>, bytes: &[u8], incoming: &mpsc::Sender<Halves>) {
        if bytes.len() < HEAD {
            return;
        }
        let mut session = [0u8; 8];
        session.copy_from_slice(&bytes[33..HEAD]);
        let key = (bytes[1..33].to_vec(), session);
        match bytes[0] {
            OPEN => {
                if let Some(halves) = self.halves(key) {
                    let _ = incoming.send(halves);
                }
            }
            DATA => {
                if let Ok(sessions) = self.sessions.lock() {
                    if let Some(inbox) = sessions.get(&key) {
                        let _ = inbox.send(Some(bytes[HEAD..].to_vec()));
                    }
                }
            }
            CLOSE | UNREACHABLE => {
                if let Ok(mut sessions) = self.sessions.lock() {
                    if let Some(inbox) = sessions.remove(&key) {
                        let _ = inbox.send(None);
                    }
                }
            }
            _ => {}
        }
    }

    fn session(
        self: &Arc<Self>,
        login: &Login,
        own: &[u8],
        incoming: &mpsc::Sender<Halves>,
    ) -> Option<()> {
        let (mut socket, _) = tungstenite::connect(login.url.as_str()).ok()?;
        let hello = serde_json::json!({
            "token": login.token, "pk": hex(own), "name": login.name,
            "ausgabe": login.kind, "rolle": "dienst", "kerne": 0, "frei": 0,
        });
        socket.send(Message::Text(hello.to_string())).ok()?;
        match socket.read().ok()? {
            Message::Text(text) if text.contains("\"bereit\"") => {}
            _ => return None,
        }
        poll(&mut socket);
        let (out, queued) = mpsc::channel::<Vec<u8>>();
        *self.out.lock().ok()? = Some(out);
        while !self.stop.load(Ordering::SeqCst) {
            for bytes in queued.try_iter().take(256) {
                socket.write(Message::Binary(bytes)).ok()?;
            }
            match socket.flush() {
                Err(e) if !waiting(&e) => return None,
                _ => {}
            }
            match socket.read() {
                Ok(Message::Binary(bytes)) => self.binary(&bytes, incoming),
                Ok(Message::Text(text)) => self.text(&text, own),
                Ok(Message::Close(_)) => return None,
                Err(e) if !waiting(&e) => return None,
                _ => {}
            }
        }
        let _ = socket.close(None);
        Some(())
    }

    fn forget(&self) {
        if let Ok(mut out) = self.out.lock() {
            *out = None;
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            for (_, inbox) in sessions.drain() {
                let _ = inbox.send(None);
            }
        }
        if let Ok(mut devices) = self.devices.lock() {
            devices.clear();
        }
    }

    /// Signs in at the relay and stays there (reconnecting with growing pauses).
    /// Connections other devices open arrive on `incoming`.
    pub fn start(login: Login, own: Vec<u8>, incoming: mpsc::Sender<Halves>) -> Arc<Self> {
        let relay = Arc::new(Self {
            out: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            devices: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
        });
        let worker = relay.clone();
        let _ = std::thread::Builder::new().spawn(move || {
            let mut wait = 1;
            while !worker.stop.load(Ordering::SeqCst) {
                let connected = worker.session(&login, &own, &incoming).is_some();
                worker.forget();
                wait = if connected {
                    1
                } else {
                    (wait * 2).min(MAX_WAIT)
                };
                for _ in 0..wait * 10 {
                    if worker.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        relay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_travel_as_lower_case_hex() {
        let key: Vec<u8> = (0..32).collect();
        assert_eq!(unhex(&hex(&key)), Some(key));
        assert_eq!(unhex("zz"), None);
        assert_eq!(unhex(&"g".repeat(64)), None);
        let f = frame(DATA, &[7; 32], &[1; 8], b"xy");
        assert_eq!((f.len(), f[0], f[HEAD]), (HEAD + 2, DATA, b'x'));
    }
}
