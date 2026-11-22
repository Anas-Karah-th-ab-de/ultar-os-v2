//! Verschlüsselter Kanal: Noise_XX über einen Rahmen-Transport (TCP direkt oder Relais).
//! TCP-Rahmen: u16 Länge (big endian) + Noise-Nachricht. Anwendungsnachrichten: u32 Länge + Stücke ≤ 65000.

use crate::chaos;
use crate::ident::{Ident, NOISE};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub trait Transport: Send {
    fn senden(&mut self, rahmen: &[u8]) -> std::io::Result<()>;
    fn empfangen(&mut self) -> std::io::Result<Vec<u8>>;
    fn zeitlimit(&mut self, _d: Option<Duration>) {}
    fn beschreibung(&self) -> String;
}

pub struct TcpTransport(pub TcpStream);

impl Transport for TcpTransport {
    fn senden(&mut self, r: &[u8]) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(r.len() + 2);
        b.extend_from_slice(&(r.len() as u16).to_be_bytes());
        b.extend_from_slice(r);
        self.0.write_all(&b)
    }
    fn empfangen(&mut self) -> std::io::Result<Vec<u8>> {
        let mut l = [0u8; 2];
        self.0.read_exact(&mut l)?;
        let mut b = vec![0u8; u16::from_be_bytes(l) as usize];
        self.0.read_exact(&mut b)?;
        Ok(b)
    }
    fn zeitlimit(&mut self, d: Option<Duration>) {
        let _ = self.0.set_read_timeout(d);
        let _ = self.0.set_write_timeout(d);
    }
    fn beschreibung(&self) -> String {
        self.0.peer_addr().map(|a| format!("direkt {a}")).unwrap_or_else(|_| "direkt".into())
    }
}

fn fehler<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

pub struct Kanal {
    t: Box<dyn Transport>,
    n: snow::TransportState,
    pub partner_pk: Vec<u8>,
    buf: Vec<u8>,
}

const STUECK: usize = 65000;

impl Kanal {
    pub fn verbinden(t: Box<dyn Transport>, ich: &Ident) -> std::io::Result<Kanal> {
        Self::handshake(t, ich, true)
    }
    pub fn annehmen(t: Box<dyn Transport>, ich: &Ident) -> std::io::Result<Kanal> {
        Self::handshake(t, ich, false)
    }

    fn handshake(mut t: Box<dyn Transport>, ich: &Ident, initiator: bool) -> std::io::Result<Kanal> {
        t.zeitlimit(Some(Duration::from_secs(10)));
        let b = snow::Builder::new(NOISE.parse().unwrap()).local_private_key(&ich.privat);
        let mut hs = if initiator { b.build_initiator() } else { b.build_responder() }.map_err(fehler)?;
        let mut buf = vec![0u8; 65535];
        let mut tmp = vec![0u8; 65535];
        // XX: -> e ; <- e, ee, s, es ; -> s, se
        let schritte: [bool; 3] = if initiator { [true, false, true] } else { [false, true, false] };
        for schreiben in schritte {
            if schreiben {
                let n = hs.write_message(&[], &mut buf).map_err(fehler)?;
                t.senden(&buf[..n])?;
            } else {
                let m = t.empfangen()?;
                hs.read_message(&m, &mut tmp).map_err(fehler)?;
            }
        }
        let partner_pk = hs.get_remote_static().ok_or_else(|| fehler("kein Partnerschlüssel"))?.to_vec();
        let n = hs.into_transport_mode().map_err(fehler)?;
        t.zeitlimit(None);
        Ok(Kanal { t, n, partner_pk, buf })
    }

    fn rahmen_senden(&mut self, klar: &[u8]) -> std::io::Result<()> {
        let n = self.n.write_message(klar, &mut self.buf).map_err(fehler)?;
        let mut out = self.buf[..n].to_vec();
        chaos::get().rahmen(&mut out)?;
        self.t.senden(&out)
    }

    fn rahmen_empfangen(&mut self) -> std::io::Result<Vec<u8>> {
        let m = self.t.empfangen()?;
        let mut out = vec![0u8; m.len()];
        let n = self.n.read_message(&m, &mut out).map_err(fehler)?;
        out.truncate(n);
        Ok(out)
    }

    pub fn senden(&mut self, daten: &[u8]) -> std::io::Result<()> {
        let mut erst = (daten.len() as u32).to_be_bytes().to_vec();
        let k = daten.len().min(STUECK - 4);
        erst.extend_from_slice(&daten[..k]);
        self.rahmen_senden(&erst)?;
        let mut p = k;
        while p < daten.len() {
            let e = (p + STUECK).min(daten.len());
            self.rahmen_senden(&daten[p..e].to_vec())?;
            p = e;
        }
        Ok(())
    }

    pub fn empfangen(&mut self) -> std::io::Result<Vec<u8>> {
        let erst = self.rahmen_empfangen()?;
        if erst.len() < 4 {
            return Err(fehler("kurzer Rahmen"));
        }
        let gesamt = u32::from_be_bytes([erst[0], erst[1], erst[2], erst[3]]) as usize;
        if gesamt > 64 << 20 {
            return Err(fehler("Nachricht zu groß"));
        }
        let mut d = erst[4..].to_vec();
        while d.len() < gesamt {
            d.extend(self.rahmen_empfangen()?);
        }
        if d.len() != gesamt {
            return Err(fehler("Länge passt nicht"));
        }
        Ok(d)
    }

    pub fn nachricht_senden(&mut self, m: &crate::proto::Nachricht) -> std::io::Result<()> {
        self.senden(&serde_json::to_vec(m).unwrap())
    }

    pub fn nachricht_empfangen(&mut self) -> std::io::Result<crate::proto::Nachricht> {
        let d = self.empfangen()?;
        serde_json::from_slice(&d).map_err(fehler)
    }

    pub fn zeitlimit(&mut self, d: Option<Duration>) {
        self.t.zeitlimit(d)
    }

    pub fn weg(&self) -> String {
        self.t.beschreibung()
    }
}
