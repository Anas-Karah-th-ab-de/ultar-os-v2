//! USB-Netzwerk über RNDIS (Remote NDIS): der Weg für Handy-USB-Tethering (Android) und für QEMUs
//! `usb-net`. Die Steuerung läuft als „eingekapselte" Befehle über Endpunkt 0 (SEND/GET ENCAPSULATED),
//! die Ethernet-Rahmen über zwei Bulk-Endpunkte, jeder mit einem 44-Byte-RNDIS-Kopf.
//!
//! So kann man ein Ultar-Gerät künftig per USB-C ans Netz bringen. CDC-ECM/NCM (viele USB-C-zu-
//! Ethernet-Dongles) ist der nächste Weg und kommt in ein eigenes Geschwister-Modul – dieselbe
//! Bindung an [`NetDev`], nur ohne RNDIS-Kopf und mit Umschalten auf die Daten-Alternative.
//!
//! Empfang wie bei Bluetooth (`bt`): mehrere Bulk-IN-Übertragungen sind **dauerhaft** armiert; wenn
//! eine fertig wird, ruft die Ereignisschleife `report`, das den Rahmen in eine Warteschlange legt
//! und die Übertragung neu armiert. `recv` pumpt den Controller und nimmt aus der Warteschlange.
//! (Ein erster Entwurf armierte nur kurz in `recv` – dann hatte das Gerät keinen Puffer, um
//! Empfangenes abzulegen: „Slirp: Failed to send packet".) `send` schickt synchron über Bulk-OUT.

use super::{w32, Desc, Hc, TRB_NORMAL};
use crate::driver::{NetDev, Result, NET};
use crate::mem::{alloc_pages, free_page};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

// RNDIS-Nachrichtentypen
const INIT_MSG: u32 = 0x0000_0002;
const INIT_CMPLT: u32 = 0x8000_0002;
const QUERY_MSG: u32 = 0x0000_0004;
const QUERY_CMPLT: u32 = 0x8000_0004;
const SET_MSG: u32 = 0x0000_0005;
const SET_CMPLT: u32 = 0x8000_0005;
const PACKET_MSG: u32 = 0x0000_0001;

// OIDs
const OID_PERMANENT_ADDR: u32 = 0x0101_0101;
const OID_PACKET_FILTER: u32 = 0x0001_010E;

/// Armierte Bulk-IN-Übertragungen in Teilen einer DMA-Fläche (jeder Teil fasst einen RNDIS-Rahmen).
const TDS: usize = 8;
const TD: u32 = 2048; // ≥ 1514-Byte-Rahmen + 44-Byte-RNDIS-Kopf

struct InEp {
    dci: u32,
    buf: u64,
    trb: [u64; TDS],
}
impl InEp {
    fn arm(&mut self, hc: &mut Hc, slot: u32, i: usize) {
        let ring = hc.ring(slot, self.dci);
        self.trb[i] = ring.push(self.buf + i as u64 * TD as u64, TD, (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
    }
    fn arm_all(&mut self, hc: &mut Hc, slot: u32) {
        for i in 0..TDS {
            self.arm(hc, slot, i);
        }
        w32(hc.db + slot as u64 * 4, self.dci);
    }
    fn part(&self, trb: u64) -> usize {
        self.trb.iter().position(|&t| t == trb).unwrap_or(0)
    }
}

/// Zustand einer USB-Netzkarte: der armierte Empfang und die Warteschlange der Rahmen.
struct Nic {
    hc_index: usize,
    slot: u32,
    in_ep: InEp,
    rx: VecDeque<Vec<u8>>,
}
static NICS: Mutex<Vec<Nic>> = Mutex::new(Vec::new());
const RX_MAX: usize = 256;

// Vorübergehende Zähler zur Fehlersuche (über `netz` sichtbar): report-Aufrufe für usb-net, davon
// mit gutem Abschluss, davon als RNDIS-Paket erkannt, davon in die Warteschlange gelegt.
use core::sync::atomic::{AtomicU64, Ordering};
static DBG: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
pub fn dbg() -> [u64; 4] {
    core::array::from_fn(|i| DBG[i].load(Ordering::Relaxed))
}

fn rd32(p: u64, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned((p + off as u64) as *const u32) }
}
fn wr32(p: u64, off: usize, v: u32) {
    unsafe { core::ptr::write_unaligned((p + off as u64) as *mut u32, v) }
}

/// Eine fertige Bulk-IN-Übertragung der USB-Netzkarte: Rahmen aus dem RNDIS-Paket nehmen, in die
/// Warteschlange legen, Übertragung neu armieren. Aus der Ereignisschleife (`pump`).
pub(super) fn report(hc: &mut Hc, (slot, dci): (u32, u32), trb: u64, code: u32, left: u32) -> bool {
    let mut list = NICS.lock();
    let Some(nic) = list.iter_mut().find(|n| n.hc_index == hc.index && n.slot == slot && n.in_ep.dci == dci) else {
        return false;
    };
    DBG[0].fetch_add(1, Ordering::Relaxed);
    let part = nic.in_ep.part(trb);
    if matches!(code, 1 | 13) {
        DBG[1].fetch_add(1, Ordering::Relaxed);
        let got = TD.saturating_sub(left) as usize;
        let base = nic.in_ep.buf + part as u64 * TD as u64;
        if got >= 44 && rd32(base, 0) == PACKET_MSG {
            DBG[2].fetch_add(1, Ordering::Relaxed);
            let data_off = rd32(base, 8) as usize + 8;
            let data_len = rd32(base, 12) as usize;
            if data_off + data_len <= got && data_len > 0 && nic.rx.len() < RX_MAX {
                DBG[3].fetch_add(1, Ordering::Relaxed);
                let mut f: Vec<u8> = Vec::new();
                if !crate::heap::kernel(|| f.try_reserve_exact(data_len).is_err()) {
                    // SICHERHEIT: Quelle liegt im Teil `part` der DMA-Fläche innerhalb `got`.
                    unsafe { f.extend_from_slice(core::slice::from_raw_parts((base + data_off as u64) as *const u8, data_len)) };
                    nic.rx.push_back(f);
                }
            }
        }
    }
    nic.in_ep.arm(hc, slot, part);
    w32(hc.db + slot as u64 * 4, dci);
    true
}

/// Handle in der Netzliste: hält nur Kennungen, den Sendepuffer und die MAC; den Controller holt es
/// je Zugriff aus `HC`, die Empfangs-Warteschlange aus `NICS`.
struct RndisHandle {
    name: String,
    hc_index: usize,
    slot: u32,
    out_dci: u32,
    mac: [u8; 6],
    tx: u64,
}

const TX: usize = 2048;

impl NetDev for RndisHandle {
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
        Some(true)
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let n = frame.len().min(TX - 44);
        wr32(self.tx, 0, PACKET_MSG);
        wr32(self.tx, 4, (44 + n) as u32);
        wr32(self.tx, 8, 36); // DataOffset ab Byte 8
        wr32(self.tx, 12, n as u32);
        for off in (16..44).step_by(4) {
            wr32(self.tx, off, 0);
        }
        // SICHERHEIT: tx ist eine ≥ TX Byte große, identisch abgebildete Seite.
        unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), (self.tx + 44) as *mut u8, n) };
        let mut hcs = super::HC.lock();
        let hc = hcs.iter_mut().find(|h| h.index == self.hc_index).ok_or("USB-Controller weg")?;
        hc.transfer(self.slot, self.out_dci, self.tx, 44 + n, 100).ok_or("USB-Senden fehlgeschlagen")?;
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        // Den Controller einmal pumpen, damit fertige Bulk-IN-Übertragungen in die Warteschlange kommen.
        {
            let mut hcs = super::HC.lock();
            if let Some(hc) = hcs.iter_mut().find(|h| h.index == self.hc_index) {
                hc.pump();
            }
        }
        let mut list = NICS.lock();
        let nic = list.iter_mut().find(|n| n.hc_index == self.hc_index && n.slot == self.slot)?;
        let f = nic.rx.pop_front()?;
        let n = f.len().min(buf.len());
        buf[..n].copy_from_slice(&f[..n]);
        Some(n)
    }
}

/// Steuerbefehl einkapseln (SEND_ENCAPSULATED_COMMAND, Klassen-Anfrage auf EP0).
fn send_cmd(hc: &mut Hc, slot: u32, iface: u16, buf: u64, msg: &[u8]) -> bool {
    unsafe { core::ptr::copy_nonoverlapping(msg.as_ptr(), buf as *mut u8, msg.len()) };
    hc.control(slot, 0x21, 0x00, 0, iface, msg.len() as u16, buf).is_some()
}
/// Antwort abholen (GET_ENCAPSULATED_RESPONSE), Länge zurück.
fn get_resp(hc: &mut Hc, slot: u32, iface: u16, buf: u64, len: u16) -> Option<usize> {
    hc.control(slot, 0xA1, 0x01, 0, iface, len, buf)
}

/// Daten-Schnittstelle (CDC-Daten, Klasse 0x0A) einer RNDIS-Funktion anbinden: Bulk-Endpunkte
/// einrichten, RNDIS starten (INIT → Paketfilter → MAC), Empfang armieren, Netzkarte anmelden.
pub(super) fn attach(
    hc: &mut Hc,
    slot: u32,
    ctx: (u64, u64, u32),
    data: &Desc,
    ifs: &[Desc],
    buf: u64,
) -> core::result::Result<Option<String>, String> {
    let Some(ctrl) = ifs.iter().find(|d| d.class == 0x02 && d.sub == 0x02 && d.proto == 0xFF) else {
        return Ok(None);
    };
    let ctrl_iface = ctrl.iface as u16;
    let Some(&(ina, _, inmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 2) else {
        return Ok(None);
    };
    let Some(&(outa, _, outmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 == 0 && e.1 & 3 == 2) else {
        return Ok(None);
    };
    let in_dci = (ina as u32 & 0xf) * 2 + 1;
    let out_dci = (outa as u32 & 0xf) * 2;
    super::config_ep(hc, slot, ctx.0, ctx.1, ctx.2, &[(in_dci, 6, inmp as u32, 0), (out_dci, 2, outmp as u32, 0)])
        .map_err(|_| "RNDIS: Bulk-Endpunkte nicht eingerichtet")?;

    // RNDIS INITIALIZE
    let mut msg = [0u8; 24];
    wr32(msg.as_mut_ptr() as u64, 0, INIT_MSG);
    wr32(msg.as_mut_ptr() as u64, 4, 24);
    wr32(msg.as_mut_ptr() as u64, 8, 1);
    wr32(msg.as_mut_ptr() as u64, 12, 1);
    wr32(msg.as_mut_ptr() as u64, 16, 0);
    wr32(msg.as_mut_ptr() as u64, 20, 0x4000);
    if !send_cmd(hc, slot, ctrl_iface, buf, &msg) {
        return Err("RNDIS: INIT nicht angenommen".into());
    }
    let r = get_resp(hc, slot, ctrl_iface, buf, 1025).ok_or("RNDIS: keine INIT-Antwort")?;
    if r < 16 || rd32(buf, 0) != INIT_CMPLT || rd32(buf, 12) != 0 {
        return Err("RNDIS: INIT fehlgeschlagen".into());
    }

    // Paketfilter setzen → Datenpfad an
    let mut set = [0u8; 28];
    wr32(set.as_mut_ptr() as u64, 0, SET_MSG);
    wr32(set.as_mut_ptr() as u64, 4, 28);
    wr32(set.as_mut_ptr() as u64, 8, 2);
    wr32(set.as_mut_ptr() as u64, 12, OID_PACKET_FILTER);
    wr32(set.as_mut_ptr() as u64, 16, 4);
    wr32(set.as_mut_ptr() as u64, 20, 20);
    wr32(set.as_mut_ptr() as u64, 24, 0x2F);
    if send_cmd(hc, slot, ctrl_iface, buf, &set) {
        let _ = get_resp(hc, slot, ctrl_iface, buf, 1025);
        let _ = SET_CMPLT; // Status wird nicht erzwungen (manche Geräte brauchen den Filter nicht)
    }

    // MAC abfragen
    let mut mac = [0u8; 6];
    let mut q = [0u8; 28];
    wr32(q.as_mut_ptr() as u64, 0, QUERY_MSG);
    wr32(q.as_mut_ptr() as u64, 4, 28);
    wr32(q.as_mut_ptr() as u64, 8, 3);
    wr32(q.as_mut_ptr() as u64, 12, OID_PERMANENT_ADDR);
    wr32(q.as_mut_ptr() as u64, 16, 0);
    wr32(q.as_mut_ptr() as u64, 20, 0);
    if send_cmd(hc, slot, ctrl_iface, buf, &q) {
        if let Some(rl) = get_resp(hc, slot, ctrl_iface, buf, 1025) {
            if rl >= 24 && rd32(buf, 0) == QUERY_CMPLT && rd32(buf, 12) == 0 {
                let off = rd32(buf, 20) as usize + 8;
                if rd32(buf, 16) as usize >= 6 && off + 6 <= rl {
                    unsafe { core::ptr::copy_nonoverlapping((buf + off as u64) as *const u8, mac.as_mut_ptr(), 6) };
                }
            }
        }
    }
    if mac == [0; 6] {
        mac = [0x02, 0x55, 0x53, 0x42, 0x00, slot as u8];
    }

    // Empfangsfläche + Sendepuffer, Empfang armieren
    let in_buf = alloc_pages(TDS * TD as usize / 4096).ok_or("kein Speicher")?;
    let tx = alloc_pages(1).ok_or("kein Speicher")?;
    let mut in_ep = InEp { dci: in_dci, buf: in_buf, trb: [0; TDS] };
    in_ep.arm_all(hc, slot);
    NICS.lock().push(Nic { hc_index: hc.index, slot, in_ep, rx: VecDeque::new() });

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
    nl.push(Box::new(RndisHandle { name: name.clone(), hc_index: hc.index, slot, out_dci, mac, tx }));
    Ok(Some(name))
}

/// Das Gerät in `slot` wurde abgezogen: Empfangsfläche freigeben, Zustand entfernen. (Die Netzkarte
/// bleibt vorerst in `NET`; echtes Entfernen aus der Netzliste folgt – siehe docs.)
pub(super) fn remove(hc_index: usize, slot: u32) {
    let mut list = NICS.lock();
    for n in list.iter().filter(|n| n.hc_index == hc_index && n.slot == slot) {
        free_page(n.in_ep.buf);
    }
    list.retain(|n| !(n.hc_index == hc_index && n.slot == slot));
}

/// Ob die Schnittstelle die Daten-Schnittstelle einer USB-Netzfunktion sein könnte (CDC-Daten 0x0A).
pub(super) fn is_data_iface(d: &Desc) -> bool {
    d.class == 0x0A
}
