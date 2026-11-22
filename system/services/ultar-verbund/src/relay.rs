//! Relais-Client: Verbindung über ultar.code-craft.tech, wenn Geräte nicht im selben Netz sind.
//! Der Server leitet nur Binärrahmen zwischen Geräten DESSELBEN Kontos weiter und sieht nur Noise-Chiffretext.
//!
//! Rahmen: [typ:1][partner_pk:32][sitzung:8][nutzlast]  typ 1=öffnen 2=daten 3=schließen 4=unerreichbar
//! Erste Textnachricht: {"token","pk","name","ausgabe","rolle","kerne","frei"}; Server: {"typ":"bereit"} und
//! {"typ":"geraete","liste":[{pk,name,ausgabe,kerne,frei}]} bei Änderungen.

use crate::entdeckung::{Gesehen, Tabelle};
use crate::kanal::Transport;
use rand::Rng;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

pub const OEFFNEN: u8 = 1;
pub const DATEN: u8 = 2;
pub const SCHLIESSEN: u8 = 3;
pub const UNERREICHBAR: u8 = 4;

type Schluessel = (Vec<u8>, [u8; 8]);

pub struct Relais {
    aus: Mutex<Option<Sender<Vec<u8>>>>,
    sitzungen: Mutex<HashMap<Schluessel, Sender<Option<Vec<u8>>>>>,
    pub verbunden: std::sync::atomic::AtomicBool,
    /// false = getrennt halten (Verbund aus)
    pub an: std::sync::atomic::AtomicBool,
    pub geraete: Tabelle,
}

pub struct RelaisTransport {
    r: Arc<Relais>,
    pk: Vec<u8>,
    s: [u8; 8],
    rx: Receiver<Option<Vec<u8>>>,
    limit: Option<Duration>,
}

fn rahmen(typ: u8, pk: &[u8], s: &[u8; 8], nutz: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(41 + nutz.len());
    b.push(typ);
    b.extend_from_slice(pk);
    b.extend_from_slice(s);
    b.extend_from_slice(nutz);
    b
}

impl Transport for RelaisTransport {
    fn senden(&mut self, r: &[u8]) -> std::io::Result<()> {
        self.r.senden(rahmen(DATEN, &self.pk, &self.s, r))
    }
    fn empfangen(&mut self) -> std::io::Result<Vec<u8>> {
        let m = match self.limit {
            Some(d) => self.rx.recv_timeout(d).map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "relais: zeit"))?,
            None => self.rx.recv().map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "relais: weg"))?,
        };
        m.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::ConnectionReset, "relais: geschlossen"))
    }
    fn zeitlimit(&mut self, d: Option<Duration>) {
        self.limit = d;
    }
    fn beschreibung(&self) -> String {
        "relais".into()
    }
}

impl Drop for RelaisTransport {
    fn drop(&mut self) {
        self.r.sitzungen.lock().unwrap().remove(&(self.pk.clone(), self.s));
        let _ = self.r.senden(rahmen(SCHLIESSEN, &self.pk, &self.s, &[]));
    }
}

pub struct Anmeldung {
    pub url: String,
    pub token: String,
    pub pk: String,
    pub name: String,
    pub ausgabe: String,
    /// "dienst" nimmt Verbindungen an, "client" nur ausgehend
    pub rolle: String,
    pub kerne: usize,
}

impl Relais {
    fn senden(&self, b: Vec<u8>) -> std::io::Result<()> {
        match &*self.aus.lock().unwrap() {
            Some(tx) => tx.send(b).map_err(|_| std::io::Error::new(std::io::ErrorKind::NotConnected, "relais getrennt")),
            None => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "relais getrennt")),
        }
    }

    /// startet den Relais-Faden (verbindet sich bei Abbruch neu). `eingang` erhält neue eingehende Sitzungen.
    pub fn starten(a: Anmeldung, eingang: Option<Sender<RelaisTransport>>, frei: Arc<dyn Fn() -> usize + Send + Sync>) -> Arc<Relais> {
        let r = Arc::new(Relais {
            aus: Mutex::new(None),
            sitzungen: Mutex::new(HashMap::new()),
            verbunden: false.into(),
            an: true.into(),
            geraete: Default::default(),
        });
        let r2 = r.clone();
        std::thread::Builder::new()
            .name("relais".into())
            .spawn(move || {
                let mut warte = 1u64;
                loop {
                    while !r2.an.load(std::sync::atomic::Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    match r2.sitzung(&a, &eingang, &frei) {
                        Ok(()) => warte = 1,
                        Err(e) => {
                            if std::env::var("ULTAR_VERBUND_LOG").is_ok() {
                                eprintln!("[verbund] relais: {e}");
                            }
                        }
                    }
                    r2.verbunden.store(false, std::sync::atomic::Ordering::SeqCst);
                    *r2.aus.lock().unwrap() = None;
                    // alle offenen Sitzungen beenden
                    for (_, tx) in r2.sitzungen.lock().unwrap().drain() {
                        let _ = tx.send(None);
                    }
                    r2.geraete.lock().unwrap().clear();
                    std::thread::sleep(Duration::from_secs(warte));
                    warte = (warte * 2).min(30);
                }
            })
            .unwrap();
        r
    }

    fn sitzung(self: &Arc<Self>, a: &Anmeldung, eingang: &Option<Sender<RelaisTransport>>, frei: &Arc<dyn Fn() -> usize + Send + Sync>) -> Result<(), String> {
        let (mut ws, _) = tungstenite::connect(a.url.as_str()).map_err(|e| e.to_string())?;
        let setz = |ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>| {
            let d = Some(Duration::from_millis(50));
            match ws.get_mut() {
                MaybeTlsStream::Plain(s) => {
                    let _ = s.set_read_timeout(d);
                    let _ = s.set_nodelay(true);
                }
                MaybeTlsStream::Rustls(s) => {
                    let _ = s.get_mut().set_read_timeout(d);
                    let _ = s.get_mut().set_nodelay(true);
                }
                _ => {}
            }
        };
        let anm = serde_json::json!({"token":a.token,"pk":a.pk,"name":a.name,"ausgabe":a.ausgabe,"rolle":a.rolle,"kerne":a.kerne,"frei":frei()});
        ws.send(Message::Text(anm.to_string())).map_err(|e| e.to_string())?;
        // auf „bereit“ warten
        let m = ws.read().map_err(|e| e.to_string())?;
        match &m {
            Message::Text(t) if t.contains("\"bereit\"") => {}
            other => return Err(format!("abgelehnt: {other}")),
        }
        setz(&mut ws);
        let (tx, rx) = channel::<Vec<u8>>();
        *self.aus.lock().unwrap() = Some(tx);
        self.verbunden.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut letzter_zustand = std::time::Instant::now();
        loop {
            if !self.an.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = ws.close(None);
                let _ = ws.flush();
                return Err("verbund aus".into());
            }
            // ausgehend
            let mut n = 0;
            while let Ok(b) = rx.try_recv() {
                ws.write(Message::Binary(b)).map_err(|e| e.to_string())?;
                n += 1;
                if n > 256 {
                    break;
                }
            }
            if letzter_zustand.elapsed() > Duration::from_secs(5) {
                letzter_zustand = std::time::Instant::now();
                ws.write(Message::Text(serde_json::json!({"typ":"frei","frei":frei()}).to_string())).map_err(|e| e.to_string())?;
            }
            match ws.flush() {
                Ok(()) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
            // eingehend
            match ws.read() {
                Ok(Message::Binary(b)) => self.eingang(b, eingang),
                Ok(Message::Text(t)) => self.text(&t),
                Ok(Message::Close(_)) => return Err("geschlossen".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn text(&self, t: &str) {
        let v: serde_json::Value = match serde_json::from_str(t) {
            Ok(v) => v,
            Err(_) => return,
        };
        if v["typ"] == "geraete" {
            let mut tab = self.geraete.lock().unwrap();
            tab.clear();
            for g in v["liste"].as_array().cloned().unwrap_or_default() {
                let pk = g["pk"].as_str().unwrap_or("").to_string();
                let Some(roh) = crate::ident::unhex(&pk) else { continue };
                if roh.len() != 32 {
                    continue;
                }
                tab.insert(
                    pk.clone(),
                    Gesehen {
                        kennung: crate::ident::kennung(&roh),
                        pk,
                        name: g["name"].as_str().unwrap_or("").chars().take(64).collect(),
                        ausgabe: g["ausgabe"].as_str().unwrap_or("").chars().take(16).collect(),
                        adresse: "relais".into(),
                        kerne: g["kerne"].as_u64().unwrap_or(1) as usize,
                        frei: g["frei"].as_u64().unwrap_or(0) as usize,
                        zuletzt_ms: crate::jetzt_ms(),
                        vertraut: false,
                        weg: "relais".into(),
                    },
                );
            }
        }
    }

    fn eingang(self: &Arc<Self>, b: Vec<u8>, eingang: &Option<Sender<RelaisTransport>>) {
        if b.len() < 41 {
            return;
        }
        let typ = b[0];
        let pk = b[1..33].to_vec();
        let mut s = [0u8; 8];
        s.copy_from_slice(&b[33..41]);
        let nutz = b[41..].to_vec();
        let k = (pk.clone(), s);
        match typ {
            OEFFNEN => {
                if let Some(e) = eingang {
                    let (tx, rx) = channel();
                    self.sitzungen.lock().unwrap().insert(k, tx);
                    let _ = e.send(RelaisTransport { r: self.clone(), pk, s, rx, limit: None });
                }
            }
            DATEN => {
                if let Some(tx) = self.sitzungen.lock().unwrap().get(&k) {
                    let _ = tx.send(Some(nutz));
                }
            }
            SCHLIESSEN | UNERREICHBAR => {
                if let Some(tx) = self.sitzungen.lock().unwrap().remove(&k) {
                    let _ = tx.send(None);
                }
            }
            _ => {}
        }
    }

    /// neue Sitzung zu einem Gerät öffnen
    pub fn oeffnen(self: &Arc<Self>, partner_pk: &[u8]) -> std::io::Result<RelaisTransport> {
        let mut s = [0u8; 8];
        rand::thread_rng().fill(&mut s);
        let (tx, rx) = channel();
        self.sitzungen.lock().unwrap().insert((partner_pk.to_vec(), s), tx);
        self.senden(rahmen(OEFFNEN, partner_pk, &s, &[]))?;
        Ok(RelaisTransport { r: self.clone(), pk: partner_pk.to_vec(), s, rx, limit: None })
    }

    pub fn warte_verbunden(&self, d: Duration) -> bool {
        let t = std::time::Instant::now();
        while t.elapsed() < d {
            if self.verbunden.load(std::sync::atomic::Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}
