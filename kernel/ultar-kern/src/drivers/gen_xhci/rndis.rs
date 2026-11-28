//! USB-Netzwerk über RNDIS (Remote NDIS): der Weg für Handy-USB-Tethering (Android) und für QEMUs
//! `usb-net`. Die Steuerung läuft als „eingekapselte" Befehle über Endpunkt 0 (SEND/GET ENCAPSULATED),
//! die Ethernet-Rahmen über zwei Bulk-Endpunkte, jeder mit einem 44-Byte-RNDIS-Kopf.
//!
//! So kann man ein Ultar-Gerät künftig per USB-C ans Netz bringen. CDC-ECM/NCM (viele USB-C-zu-
//! Ethernet-Dongles) ist der nächste Weg und kommt in ein eigenes Geschwister-Modul – dieselbe
//! Bindung an [`NetDev`], nur ohne RNDIS-Kopf und mit Umschalten auf die Daten-Alternative.
//!
//! Der Datenpfad pollt (wie die übrigen Netzkarten): `recv` stößt eine kurze Bulk-IN-Übertragung an,
//! `send` eine Bulk-OUT-Übertragung. Beide sperren kurz den USB-Controller (`HC`) und geben ihn frei.

use super::{config_ep, Desc, Hc};
use crate::driver::{NetDev, Result, NET};
use crate::mem::alloc_pages;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

// RNDIS-Nachrichtentypen
const INIT_MSG: u32 = 0x0000_0002;
const INIT_CMPLT: u32 = 0x8000_0002;
const QUERY_MSG: u32 = 0x0000_0004;
const QUERY_CMPLT: u32 = 0x8000_0004;
const SET_MSG: u32 = 0x0000_0005;
const SET_CMPLT: u32 = 0x8000_0005;
const PACKET_MSG: u32 = 0x0000_0001;

// OIDs
const OID_PERMANENT_ADDR: u32 = 0x0101_0101; // 802.3 feste MAC
const OID_PACKET_FILTER: u32 = 0x0001_010E; // welche Rahmen durchgelassen werden

/// Steuerbefehl einkapseln (SEND_ENCAPSULATED_COMMAND, Klassen-Anfrage auf EP0).
fn send_cmd(hc: &mut Hc, slot: u32, iface: u16, buf: u64, msg: &[u8]) -> bool {
    // SICHERHEIT: `buf` ist eine zugeteilte, identisch abgebildete Seite ≥ msg.len().
    unsafe { core::ptr::copy_nonoverlapping(msg.as_ptr(), buf as *mut u8, msg.len()) };
    hc.control(slot, 0x21, 0x00, 0, iface, msg.len() as u16, buf).is_some()
}

/// Antwort abholen (GET_ENCAPSULATED_RESPONSE). Gibt die Länge zurück.
fn get_resp(hc: &mut Hc, slot: u32, iface: u16, buf: u64, len: u16) -> Option<usize> {
    hc.control(slot, 0xA1, 0x01, 0, iface, len, buf)
}

fn rd32(p: u64, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned((p + off as u64) as *const u32) }
}
fn wr32(p: u64, off: usize, v: u32) {
    unsafe { core::ptr::write_unaligned((p + off as u64) as *mut u32, v) }
}

/// Eine RNDIS-USB-Netzkarte. Hält nur Kennungen und DMA-Puffer; der Controller wird je Zugriff aus
/// `HC` geholt.
struct RndisNic {
    name: String,
    hc_index: usize,
    slot: u32,
    in_dci: u32,
    out_dci: u32,
    mac: [u8; 6],
    tx: u64,
    rx: u64,
}

const DMA: usize = 8192; // genug für 1514-Byte-Rahmen + RNDIS-Kopf

impl NetDev for RndisNic {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn driver(&self) -> &'static str {
        "usb-rndis"
    }
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    fn link(&self) -> Option<bool> {
        Some(true) // RNDIS meldet den Verbindungsstatus über den Interrupt-Endpunkt; hier als an angenommen
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let n = frame.len().min(DMA - 44);
        // RNDIS_PACKET_MSG-Kopf (44 Byte): DataOffset zählt ab Byte 8 → 36
        wr32(self.tx, 0, PACKET_MSG);
        wr32(self.tx, 4, (44 + n) as u32);
        wr32(self.tx, 8, 36);
        wr32(self.tx, 12, n as u32);
        for off in (16..44).step_by(4) {
            wr32(self.tx, off, 0);
        }
        // SICHERHEIT: tx ist eine ≥ DMA Byte große, identisch abgebildete Seite.
        unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), (self.tx + 44) as *mut u8, n) };
        let mut hcs = super::HC.lock();
        let hc = hcs.iter_mut().find(|h| h.index == self.hc_index).ok_or("USB-Controller weg")?;
        hc.transfer(self.slot, self.out_dci, self.tx, 44 + n, 100).ok_or("USB-Senden fehlgeschlagen")?;
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut hcs = super::HC.lock();
        let hc = hcs.iter_mut().find(|h| h.index == self.hc_index)?;
        // kurze Bulk-IN-Übertragung; ohne Rahmen kommt nichts (Gerät sendet NAK → Zeit läuft ab)
        let got = hc.transfer(self.slot, self.in_dci, self.rx, DMA, 2)?;
        drop(hcs);
        if got < 44 {
            return None;
        }
        if rd32(self.rx, 0) != PACKET_MSG {
            return None;
        }
        let data_off = rd32(self.rx, 8) as usize + 8;
        let data_len = rd32(self.rx, 12) as usize;
        if data_off + data_len > got {
            return None;
        }
        let n = data_len.min(buf.len());
        // SICHERHEIT: Quelle liegt im rx-Puffer innerhalb der geprüften Grenzen `got`.
        unsafe { core::ptr::copy_nonoverlapping((self.rx + data_off as u64) as *const u8, buf.as_mut_ptr(), n) };
        Some(n)
    }
}

/// Die Daten-Schnittstelle (CDC-Daten, Klasse 0x0A) einer RNDIS-Funktion anbinden: Bulk-Endpunkte
/// einrichten, RNDIS starten (INIT → Paketfilter setzen → MAC abfragen), Netzkarte anmelden.
/// `data` = die 0x0A-Schnittstelle, `ifs` = alle Schnittstellen (für die Steuer-Schnittstelle),
/// `buf` = eine freie DMA-Seite aus `attach`. Rückgabe: der Gerätename, oder `Ok(None)` wenn es
/// keine RNDIS-Funktion ist.
pub(super) fn attach(
    hc: &mut Hc,
    slot: u32,
    ctx: (u64, u64, u32),
    data: &Desc,
    ifs: &[Desc],
    buf: u64,
) -> core::result::Result<Option<String>, String> {
    // Steuer-Schnittstelle: Kommunikation (0x02), Teilklasse ACM (0x02), Protokoll herstellerspezifisch
    // (0xFF = RNDIS). Ohne sie ist die 0x0A-Schnittstelle kein RNDIS (sondern evtl. CDC-ECM, anderes Modul).
    let Some(ctrl) = ifs.iter().find(|d| d.class == 0x02 && d.sub == 0x02 && d.proto == 0xFF) else {
        return Ok(None);
    };
    let ctrl_iface = ctrl.iface as u16;
    // Bulk IN und OUT der Daten-Schnittstelle
    let Some(&(ina, _, inmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 2) else {
        return Ok(None);
    };
    let Some(&(outa, _, outmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 == 0 && e.1 & 3 == 2) else {
        return Ok(None);
    };
    let in_dci = (ina as u32 & 0xf) * 2 + 1;
    let out_dci = (outa as u32 & 0xf) * 2;
    config_ep(hc, slot, ctx.0, ctx.1, ctx.2, &[(in_dci, 6, inmp as u32, 0), (out_dci, 2, outmp as u32, 0)])
        .map_err(|_| "RNDIS: Bulk-Endpunkte nicht eingerichtet")?;

    // RNDIS INITIALIZE
    let mut msg = [0u8; 24];
    wr32(msg.as_mut_ptr() as u64, 0, INIT_MSG);
    wr32(msg.as_mut_ptr() as u64, 4, 24);
    wr32(msg.as_mut_ptr() as u64, 8, 1); // RequestId
    wr32(msg.as_mut_ptr() as u64, 12, 1); // MajorVersion
    wr32(msg.as_mut_ptr() as u64, 16, 0); // MinorVersion
    wr32(msg.as_mut_ptr() as u64, 20, 0x4000); // MaxTransferSize
    if !send_cmd(hc, slot, ctrl_iface, buf, &msg) {
        return Err("RNDIS: INIT nicht angenommen".into());
    }
    let r = get_resp(hc, slot, ctrl_iface, buf, 1025).ok_or("RNDIS: keine INIT-Antwort")?;
    if r < 16 || rd32(buf, 0) != INIT_CMPLT || rd32(buf, 12) != 0 {
        return Err("RNDIS: INIT fehlgeschlagen".into());
    }

    // Paketfilter setzen (DIRECTED|MULTICAST|ALL_MULTICAST|BROADCAST|PROMISCUOUS) → Datenpfad an
    let mut set = [0u8; 28];
    wr32(set.as_mut_ptr() as u64, 0, SET_MSG);
    wr32(set.as_mut_ptr() as u64, 4, 28);
    wr32(set.as_mut_ptr() as u64, 8, 2); // RequestId
    wr32(set.as_mut_ptr() as u64, 12, OID_PACKET_FILTER);
    wr32(set.as_mut_ptr() as u64, 16, 4); // InformationBufferLength
    wr32(set.as_mut_ptr() as u64, 20, 20); // InformationBufferOffset (ab Byte 8)
    wr32(set.as_mut_ptr() as u64, 24, 0x2F); // Filter
    if send_cmd(hc, slot, ctrl_iface, buf, &set) {
        let _ = get_resp(hc, slot, ctrl_iface, buf, 1025);
        if rd32(buf, 0) != SET_CMPLT {
            // nicht tödlich; manche Geräte brauchen den Filter nicht zum Empfang
        }
    }

    // MAC abfragen (OID_802_3_PERMANENT_ADDRESS)
    let mut mac = [0u8; 6];
    let mut q = [0u8; 28];
    wr32(q.as_mut_ptr() as u64, 0, QUERY_MSG);
    wr32(q.as_mut_ptr() as u64, 4, 28);
    wr32(q.as_mut_ptr() as u64, 8, 3); // RequestId
    wr32(q.as_mut_ptr() as u64, 12, OID_PERMANENT_ADDR);
    wr32(q.as_mut_ptr() as u64, 16, 0); // InformationBufferLength
    wr32(q.as_mut_ptr() as u64, 20, 0); // InformationBufferOffset
    if send_cmd(hc, slot, ctrl_iface, buf, &q) {
        if let Some(rl) = get_resp(hc, slot, ctrl_iface, buf, 1025) {
            // QUERY_CMPLT: Status(12), InfoBufLen(16), InfoBufOffset(20, ab Byte 8)
            if rl >= 24 && rd32(buf, 0) == QUERY_CMPLT && rd32(buf, 12) == 0 {
                let off = rd32(buf, 20) as usize + 8;
                if rd32(buf, 16) as usize >= 6 && off + 6 <= rl {
                    // SICHERHEIT: off+6 ≤ rl ≤ gelesene Länge im buf.
                    unsafe { core::ptr::copy_nonoverlapping((buf + off as u64) as *const u8, mac.as_mut_ptr(), 6) };
                }
            }
        }
    }
    if mac == [0; 6] {
        mac = [0x02, 0x55, 0x53, 0x42, 0x00, slot as u8]; // lokal verwaltete Ersatz-MAC
    }

    let tx = alloc_pages(DMA / 4096).ok_or("kein Speicher")?;
    let rx = alloc_pages(DMA / 4096).ok_or("kein Speicher")?;
    let mut nl = NET.lock();
    let name = format!("usb{}", nl.len());
    crate::kprintln!(
        "[usb-net] {name}: RNDIS (slot {slot}) MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    nl.push(Box::new(RndisNic { name: name.clone(), hc_index: hc.index, slot, in_dci, out_dci, mac, tx, rx }));
    Ok(Some(name))
}

/// Ob die Schnittstelle die Daten-Schnittstelle einer RNDIS-Funktion sein könnte (CDC-Daten 0x0A).
pub(super) fn is_data_iface(d: &Desc) -> bool {
    d.class == 0x0A
}
