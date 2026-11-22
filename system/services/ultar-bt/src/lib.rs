//! ultar-bt – eigener Bluetooth-Host für Geräte ohne Bluetooth im Kernel.
//!
//! Auf dem Samsung A30 gibt es kein BlueZ im Kernel (CONFIG_SCSC_BT_BLUEZ aus): der Chip (SCSC/Maxwell)
//! erscheint nur als Zeichengerät `/dev/scsc_h4_0`, über das rohe HCI-Pakete im H4-Format laufen
//! (1 Byte Pakettyp + HCI-Paket). Das Öffnen startet den Bluetooth-Dienst im Chip (Firmware aus /vendor).
//! Dieser Host spricht HCI direkt: Einschalten, Adresse/Version lesen, Suche (klassisch + Low Energy).
//! Koppeln und Profile (Audio, Eingabegeräte) bauen darauf auf und fehlen noch.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

pub const H4_DEV: &str = "/dev/scsc_h4_0";
const H4_CMD: u8 = 0x01;
const H4_ACL: u8 = 0x02;
const H4_SCO: u8 = 0x03;
const H4_EVT: u8 = 0x04;

// Opcodes = (OGF << 10) | OCF
pub const OP_INQUIRY: u16 = 0x0401;
pub const OP_INQUIRY_CANCEL: u16 = 0x0402;
pub const OP_SET_EVENT_MASK: u16 = 0x0C01;
pub const OP_RESET: u16 = 0x0C03;
pub const OP_WRITE_LOCAL_NAME: u16 = 0x0C13;
pub const OP_WRITE_SCAN_ENABLE: u16 = 0x0C1A;
pub const OP_WRITE_INQUIRY_MODE: u16 = 0x0C45;
pub const OP_READ_LOCAL_VERSION: u16 = 0x1001;
pub const OP_READ_BD_ADDR: u16 = 0x1009;
pub const OP_LE_SET_EVENT_MASK: u16 = 0x2001;
pub const OP_LE_SET_SCAN_PARAMS: u16 = 0x200B;
pub const OP_LE_SET_SCAN_ENABLE: u16 = 0x200C;

/// Ein vollständiges H4-Paket.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    Event { code: u8, data: Vec<u8> },
    Acl(Vec<u8>),
    Sco(Vec<u8>),
}

/// Setzt den Bytestrom des Geräts zu Paketen zusammen (ein read() kann Teile oder mehrere Pakete liefern).
#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn next(&mut self) -> Option<Packet> {
        loop {
            let b = &self.buf;
            let (hdr, len) = match *b.first()? {
                H4_EVT => (3, *b.get(2)? as usize),
                H4_ACL => (5, u16::from_le_bytes([*b.get(3)?, *b.get(4)?]) as usize),
                H4_SCO => (4, *b.get(3)? as usize),
                _ => {
                    // unbekannter Typ: ein Byte verwerfen und neu ausrichten
                    self.buf.remove(0);
                    continue;
                }
            };
            if b.len() < hdr + len {
                return None;
            }
            let pkt: Vec<u8> = self.buf.drain(..hdr + len).collect();
            return Some(match pkt[0] {
                H4_EVT => Packet::Event { code: pkt[1], data: pkt[3..].to_vec() },
                H4_ACL => Packet::Acl(pkt[1..].to_vec()),
                _ => Packet::Sco(pkt[1..].to_vec()),
            });
        }
    }
}

/// Gefundenes Gerät.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub addr: [u8; 6],
    pub le: bool,
    pub rssi: Option<i8>,
    pub name: Option<String>,
    /// Geräteklasse (nur klassisch), 24 Bit
    pub class: u32,
}

pub fn addr_str(a: &[u8; 6]) -> String {
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", a[5], a[4], a[3], a[2], a[1], a[0])
}

/// Name aus EIR-/Werbedaten (Längen-Typ-Wert-Folgen; Typ 0x09 = voller, 0x08 = gekürzter Name).
pub fn name_from_ad(ad: &[u8]) -> Option<String> {
    let (mut i, mut short) = (0usize, None);
    while i < ad.len() {
        let l = ad[i] as usize;
        if l == 0 || i + 1 + l > ad.len() {
            break;
        }
        let (ty, val) = (ad[i + 1], &ad[i + 2..i + 1 + l]);
        if ty == 0x09 {
            return Some(String::from_utf8_lossy(val).trim_end_matches('\0').to_string());
        }
        if ty == 0x08 {
            short = Some(String::from_utf8_lossy(val).trim_end_matches('\0').to_string());
        }
        i += 1 + l;
    }
    short
}

/// Suchergebnisse aus einem Ereignis lesen (klassische Suche mit/ohne RSSI/EIR, LE-Werbung).
pub fn parse_found(code: u8, d: &[u8]) -> Vec<Found> {
    let mut out = Vec::new();
    let addr = |s: &[u8]| -> [u8; 6] { [s[0], s[1], s[2], s[3], s[4], s[5]] };
    match code {
        // Inquiry Result: n × (addr 6, psrm 1, res 2, class 3, clock 2) = 14
        0x02 if !d.is_empty() => {
            for c in d[1..].chunks_exact(14).take(d[0] as usize) {
                out.push(Found { addr: addr(c), le: false, rssi: None, name: None, class: u32::from_le_bytes([c[9], c[10], c[11], 0]) });
            }
        }
        // Inquiry Result with RSSI: n × (addr 6, psrm 1, res 1, class 3, clock 2, rssi 1) = 14
        0x22 if !d.is_empty() => {
            for c in d[1..].chunks_exact(14).take(d[0] as usize) {
                out.push(Found { addr: addr(c), le: false, rssi: Some(c[13] as i8), name: None, class: u32::from_le_bytes([c[8], c[9], c[10], 0]) });
            }
        }
        // Extended Inquiry Result: 1 × (addr 6, psrm 1, res 1, class 3, clock 2, rssi 1, eir 240)
        0x2F if d.len() >= 15 => {
            let c = &d[1..];
            out.push(Found { addr: addr(c), le: false, rssi: Some(c[13] as i8), name: name_from_ad(&c[14..]), class: u32::from_le_bytes([c[8], c[9], c[10], 0]) });
        }
        // LE Meta, Unterereignis 0x02 Advertising Report: n × (typ 1, addrtyp 1, addr 6, len 1, daten, rssi 1)
        0x3E if d.len() >= 2 && d[0] == 0x02 => {
            let mut i = 2;
            for _ in 0..d[1] {
                if i + 9 > d.len() {
                    break;
                }
                let l = d[i + 8] as usize;
                if i + 9 + l + 1 > d.len() {
                    break;
                }
                out.push(Found { addr: addr(&d[i + 2..]), le: true, rssi: Some(d[i + 9 + l] as i8), name: name_from_ad(&d[i + 9..i + 9 + l]), class: 0 });
                i += 9 + l + 1;
            }
        }
        _ => {}
    }
    out
}

/// Ergebnisse zusammenführen (gleiche Adresse: Name und bester Empfang bleiben erhalten).
pub fn merge(list: &mut Vec<Found>, f: Found) {
    if let Some(e) = list.iter_mut().find(|e| e.addr == f.addr) {
        if f.name.is_some() {
            e.name = f.name;
        }
        if f.rssi > e.rssi {
            e.rssi = f.rssi;
        }
        if f.class != 0 {
            e.class = f.class;
        }
    } else {
        list.push(f);
    }
}

pub struct Hci {
    dev: File,
    fr: Framer,
    /// Ereignisse, die beim Warten auf eine Befehlsantwort ankamen
    pub pending: Vec<Packet>,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Timeout(u16),
    Status(u16, u8),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Timeout(op) => write!(f, "keine Antwort auf Befehl 0x{op:04X}"),
            Error::Status(op, s) => write!(f, "Befehl 0x{op:04X} abgelehnt (Status 0x{s:02X})"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl Hci {
    /// Öffnet das H4-Gerät – das startet den Bluetooth-Dienst im Chip.
    pub fn open(path: &str) -> std::io::Result<Hci> {
        let dev = File::options().read(true).write(true).open(path)?;
        Ok(Hci { dev, fr: Framer::default(), pending: Vec::new() })
    }

    /// Nächstes Paket, höchstens `timeout` warten.
    pub fn recv(&mut self, timeout: Duration) -> std::io::Result<Option<Packet>> {
        use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
        let end = Instant::now() + timeout;
        loop {
            if let Some(p) = self.fr.next() {
                return Ok(Some(p));
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let mut fds = [PollFd::new(self.dev.as_fd(), PollFlags::POLLIN)];
            let ms = left.as_millis().min(60_000) as u16;
            if poll(&mut fds, PollTimeout::from(ms)).unwrap_or(0) <= 0 {
                continue;
            }
            let mut buf = [0u8; 2048];
            let n = self.dev.read(&mut buf)?;
            if n == 0 {
                return Ok(None);
            }
            self.fr.push(&buf[..n]);
        }
    }

    /// Befehl senden und auf Command Complete (Rückgabedaten ohne Status) bzw. Command Status warten.
    pub fn cmd(&mut self, op: u16, params: &[u8]) -> Result<Vec<u8>, Error> {
        let mut p = vec![H4_CMD, op as u8, (op >> 8) as u8, params.len() as u8];
        p.extend_from_slice(params);
        self.dev.write_all(&p)?;
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end {
            match self.recv(end.saturating_duration_since(Instant::now()))? {
                Some(Packet::Event { code: 0x0E, data }) if data.len() >= 4 && u16::from_le_bytes([data[1], data[2]]) == op => {
                    return if data[3] == 0 { Ok(data[4..].to_vec()) } else { Err(Error::Status(op, data[3])) };
                }
                Some(Packet::Event { code: 0x0F, data }) if data.len() >= 4 && u16::from_le_bytes([data[2], data[3]]) == op => {
                    return if data[0] == 0 { Ok(Vec::new()) } else { Err(Error::Status(op, data[0])) };
                }
                Some(other) => self.pending.push(other),
                None => break,
            }
        }
        Err(Error::Timeout(op))
    }

    /// Grundeinrichtung: zurücksetzen, Ereignisse freischalten, Suchergebnisse mit Empfangsstärke und Namen.
    pub fn init(&mut self, name: &str) -> Result<(), Error> {
        self.cmd(OP_RESET, &[])?;
        self.cmd(OP_SET_EVENT_MASK, &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F])?;
        let _ = self.cmd(OP_LE_SET_EVENT_MASK, &[0x1F, 0, 0, 0, 0, 0, 0, 0]);
        let _ = self.cmd(OP_WRITE_INQUIRY_MODE, &[2]);
        let mut n = [0u8; 248];
        let b = name.as_bytes();
        n[..b.len().min(247)].copy_from_slice(&b[..b.len().min(247)]);
        let _ = self.cmd(OP_WRITE_LOCAL_NAME, &n);
        Ok(())
    }

    pub fn bd_addr(&mut self) -> Result<[u8; 6], Error> {
        let r = self.cmd(OP_READ_BD_ADDR, &[])?;
        let mut a = [0u8; 6];
        if r.len() >= 6 {
            a.copy_from_slice(&r[..6]);
        }
        Ok(a)
    }

    /// (HCI-Version, Hersteller, LMP-Unterversion)
    pub fn version(&mut self) -> Result<(u8, u16, u16), Error> {
        let r = self.cmd(OP_READ_LOCAL_VERSION, &[])?;
        if r.len() < 8 {
            return Ok((0, 0, 0));
        }
        Ok((r[0], u16::from_le_bytes([r[4], r[5]]), u16::from_le_bytes([r[6], r[7]])))
    }

    /// Sichtbar/verbindbar schalten (0 = aus, 2 = verbindbar, 3 = sichtbar + verbindbar).
    pub fn scan_enable(&mut self, mode: u8) -> Result<(), Error> {
        self.cmd(OP_WRITE_SCAN_ENABLE, &[mode]).map(|_| ())
    }

    /// Geräte suchen: klassisch (Inquiry) und Low Energy gleichzeitig, `secs` Sekunden.
    pub fn scan(&mut self, secs: u64, mut on_found: impl FnMut(&Found)) -> Result<Vec<Found>, Error> {
        let mut list = Vec::new();
        // LE: aktiv suchen, 10 ms Fenster alle 100 ms (lässt dem klassischen Funk Zeit), Duplikate filtern
        let le = self.cmd(OP_LE_SET_SCAN_PARAMS, &[1, 0xA0, 0x00, 0x10, 0x00, 0, 0]).is_ok() && self.cmd(OP_LE_SET_SCAN_ENABLE, &[1, 1]).is_ok();
        // klassisch: allgemeine Suche (LAP 9E8B33), Dauer in 1,28-s-Schritten (max. 0x30)
        let units = ((secs as f32 / 1.28).ceil() as u8).clamp(1, 0x30);
        let classic = self.cmd(OP_INQUIRY, &[0x33, 0x8B, 0x9E, units, 0]).is_ok();
        if !le && !classic {
            return Err(Error::Timeout(OP_INQUIRY));
        }
        let end = Instant::now() + Duration::from_secs(secs);
        let mut queued: Vec<Packet> = std::mem::take(&mut self.pending);
        loop {
            let pkt = if let Some(p) = queued.pop() { Some(p) } else { self.recv(end.saturating_duration_since(Instant::now()))? };
            let Some(pkt) = pkt else { break };
            if let Packet::Event { code, data } = pkt {
                for f in parse_found(code, &data) {
                    on_found(&f);
                    merge(&mut list, f);
                }
            }
            if Instant::now() >= end {
                break;
            }
        }
        if le {
            let _ = self.cmd(OP_LE_SET_SCAN_ENABLE, &[0, 0]);
        }
        if classic {
            let _ = self.cmd(OP_INQUIRY_CANCEL, &[]);
        }
        self.pending.clear();
        list.sort_by_key(|f| std::cmp::Reverse(f.rssi));
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framer_splits_and_joins() {
        let mut f = Framer::default();
        // Command Complete für Reset, in zwei Teilen, direkt gefolgt von einem zweiten Ereignis
        f.push(&[0x04, 0x0E, 0x04, 0x01]);
        assert!(f.next().is_none());
        f.push(&[0x03, 0x0C, 0x00, 0x04, 0x01, 0x01, 0x00]);
        assert_eq!(f.next(), Some(Packet::Event { code: 0x0E, data: vec![1, 3, 0x0C, 0] }));
        assert_eq!(f.next(), Some(Packet::Event { code: 0x01, data: vec![0] }));
        assert!(f.next().is_none());
    }

    #[test]
    fn framer_acl_and_resync() {
        let mut f = Framer::default();
        f.push(&[0xEE, 0x02, 0x01, 0x20, 0x02, 0x00, 0xAA, 0xBB]);
        assert_eq!(f.next(), Some(Packet::Acl(vec![0x01, 0x20, 0x02, 0x00, 0xAA, 0xBB])));
    }

    #[test]
    fn ad_names() {
        assert_eq!(name_from_ad(&[2, 1, 6, 6, 9, b'U', b'l', b't', b'a', b'r']), Some("Ultar".into()));
        assert_eq!(name_from_ad(&[3, 8, b'U', b'l']), Some("Ul".into()));
        assert_eq!(name_from_ad(&[2, 1, 6]), None);
        assert_eq!(name_from_ad(&[9, 9, b'x']), None); // Länge läuft über das Ende
    }

    #[test]
    fn le_report() {
        // 1 Bericht: ADV_IND, öffentliche Adresse 11:22:33:44:55:66, 7 Byte Daten (Name "Maus"), RSSI -60
        let mut d = vec![0x02, 1, 0, 0, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 6, 5, 9, b'M', b'a', b'u', b's'];
        d.push((-60i8) as u8);
        let f = parse_found(0x3E, &d);
        assert_eq!(f.len(), 1);
        assert_eq!(addr_str(&f[0].addr), "11:22:33:44:55:66");
        assert_eq!((f[0].le, f[0].rssi, f[0].name.as_deref()), (true, Some(-60), Some("Maus")));
    }

    #[test]
    fn classic_rssi_and_merge() {
        let mut d = vec![1, 1, 2, 3, 4, 5, 6, 1, 0, 0x0C, 0x02, 0x5A, 0, 0];
        d.push((-45i8) as u8);
        let f = parse_found(0x22, &d);
        assert_eq!((f[0].rssi, f[0].class), (Some(-45), 0x5A020C));
        let mut list = Vec::new();
        merge(&mut list, f[0].clone());
        merge(&mut list, Found { name: Some("Box".into()), rssi: Some(-70), ..f[0].clone() });
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].rssi, list[0].name.as_deref()), (Some(-45), Some("Box")));
    }
}
