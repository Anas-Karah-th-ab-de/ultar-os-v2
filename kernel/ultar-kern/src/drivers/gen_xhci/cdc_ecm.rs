//! USB-Netzwerk über CDC-ECM (Ethernet Networking Control Model): der Weg vieler USB-C-zu-Ethernet-
//! Dongles. Wie `rndis` bindet dieses Geschwister-Modul an [`NetDev`], nur **ohne** RNDIS-Kopf: rohe
//! Ethernet-Rahmen über die Bulk-Endpunkte der Daten-Schnittstelle.
//!
//! Eine ECM-Funktion hat eine Steuer-Schnittstelle (CDC, Klasse 0x02, Unterklasse 0x06) und eine
//! Daten-Schnittstelle (Klasse 0x0A) mit zwei Alternativen (0 ohne, 1 mit Bulk-Endpunkten). Der
//! Union-Deskriptor nennt die Daten-Schnittstelle, der „Ethernet Networking Functional Descriptor"
//! den Zeiger auf die MAC-Zeichenkette (iMACAddress, ein USB-Zeichenketten-Deskriptor). Vor dem
//! Datenbetrieb schaltet der Treiber per `SET_INTERFACE` auf die Daten-Alternative. Der Interrupt-
//! Endpunkt der Steuer-Schnittstelle meldet `NETWORK_CONNECTION`/`SPEED_CHANGE` → Linkzustand und
//! Geschwindigkeit.

use super::net_common::{rd16, rd32, rd8, InEp, RX_MAX, TD};
use super::{config_ep, w32, Desc, Hc, TRB_NORMAL};
use crate::driver::{NetDev, Result, NET};
use crate::mem::{alloc_pages, free_page};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Bytes Batterie-Rahmen je Bulk-IN-Übertragung.
pub(super) const TX: usize = 4096;

/// Zustand einer ECM-Netzkarte: armierter Empfang, Sendepuffer, Meldungs-Endpunkt und Warteschlange.
struct Nic {
    hc_index: usize,
    slot: u32,
    name: String,
    in_ep: InEp,
    tx: u64,
    /// Interrupt-IN (Meldungen); `0`: das Gerät hat keinen.
    intr_dci: u32,
    intr_buf: u64,
    intr_mp: u32,
    link: bool,
    speed: u32,
    rx: VecDeque<Vec<u8>>,
}
static NICS: Mutex<Vec<Nic>> = Mutex::new(Vec::new());

/// Handle in der Netzliste: hält Kennungen, Sendepuffer und MAC; Link/Geschwindigkeit kommen aus
/// `NICS`, die Warteschlange ebenso.
struct EcmHandle {
    name: String,
    hc_index: usize,
    slot: u32,
    in_dci: u32,
    out_dci: u32,
    mac: [u8; 6],
    tx: u64,
}

impl NetDev for EcmHandle {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn driver(&self) -> &'static str {
        "usb-ecm"
    }
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    fn link(&self) -> Option<bool> {
        let link = NICS.lock().iter().find(|n| n.hc_index == self.hc_index && n.slot == self.slot).map(|n| n.link).unwrap_or(false);
        Some(link)
    }
    fn speed_mbit(&self) -> Option<u32> {
        let s = NICS.lock().iter().find(|n| n.hc_index == self.hc_index && n.slot == self.slot).map(|n| n.speed).unwrap_or(0);
        (s != 0).then_some(s)
    }
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let n = frame.len().min(TX);
        // SICHERHEIT: tx ist eine entspeicherte, identisch abgebildete DMA-Seite ≥ TX Byte.
        unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx as *mut u8, n) };
        let mut hcs = super::HC.lock();
        let hc = hcs.iter_mut().find(|h| h.index == self.hc_index).ok_or("USB-Controller weg")?;
        hc.transfer(self.slot, self.out_dci, self.tx, n, 100).ok_or("USB-Senden fehlgeschlagen")?;
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        {
            let mut hcs = super::HC.lock();
            if let Some(hc) = hcs.iter_mut().find(|h| h.index == self.hc_index) {
                w32(hc.db + self.slot as u64 * 4, self.in_dci);
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

/// Eine fertige Bulk-IN- oder Interrupt-IN-Übertragung: Rahmen (rohes Ethernet) in die Warteschlange
/// legen bzw. die Meldung auswerten, Übertragung neu armieren.
pub(super) fn report(hc: &mut Hc, (slot, dci): (u32, u32), trb: u64, code: u32, left: u32) -> bool {
    let mut list = NICS.lock();
    let Some(nic) = list.iter_mut().find(|n| n.hc_index == hc.index && n.slot == slot && (n.in_ep.dci == dci || (n.intr_dci != 0 && n.intr_dci == dci))) else {
        return false;
    };
    let is_intr = dci == nic.intr_dci && nic.intr_dci != 0;
    if is_intr {
        if matches!(code, 1 | 13) {
            let t = rd8(nic.intr_buf, 1);
            match t {
                0x00 => nic.link = rd16(nic.intr_buf, 2) != 0,
                0x2a => {
                    // 4 Byte Upstream + 4 Byte Downstream (Bit/s), Little-Endian
                    if rd16(nic.intr_buf, 6) >= 8 {
                        nic.speed = rd32(nic.intr_buf, 12) / 1_000_000;
                    }
                }
                _ => {}
            }
        }
        let ring = hc.ring(slot, dci);
        ring.push(nic.intr_buf, nic.intr_mp.max(8), (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
        w32(hc.db + slot as u64 * 4, dci);
        return true;
    }
    let part = nic.in_ep.part(trb);
    if matches!(code, 1 | 13) {
        let got = TD.saturating_sub(left) as usize;
        if got > 0 && nic.rx.len() < RX_MAX {
            let base = nic.in_ep.base(part);
            let mut f: Vec<u8> = Vec::new();
            if !crate::heap::kernel(|| f.try_reserve_exact(got).is_err()) {
                // SICHERHEIT: Quelle liegt im Teil `part` der DMA-Seite innerhalb `got`.
                unsafe { f.extend_from_slice(core::slice::from_raw_parts(base as *const u8, got)) };
                nic.rx.push_back(f);
            }
        }
    }
    nic.in_ep.arm(hc, slot, part);
    w32(hc.db + slot as u64 * 4, dci);
    true
}

/// Ob die Schnittstelle eine ECM-Steuer-Schnittstelle ist (CDC, Unterklasse 0x06).
pub(super) fn is_ctrl(d: &Desc) -> bool {
    d.class == 0x02 && d.sub == 0x06
}

/// Steuer-Schnittstelle einer ECM-Funktion anbinden: Daten-Alternative wählen, Endpunkte einrichten,
/// MAC aus der Zeichenkette, Empfang armieren, Netzkarte anmelden.
pub(super) fn attach(
    hc: &mut Hc,
    slot: u32,
    ctx: (u64, u64, u32),
    ctrl: &Desc,
    ifs: &[Desc],
    cd: &[u8],
    buf: u64,
) -> core::result::Result<Option<String>, String> {
    // Daten-Schnittstelle (Klasse 0x0A) mit einer Daten-Alternative, die Bulk-Endpunkte trägt
    let Some(data) = ifs.iter().find(|d| d.class == 0x0a && d.alt != 0 && d.eps.iter().any(|e| e.1 & 3 == 2)) else {
        return Ok(None);
    };
    let data_iface = data.iface as u16;
    let data_alt = data.alt as u16;
    let Some(&(ina, _, inmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 2) else { return Ok(None) };
    let Some(&(outa, _, outmp, _)) = data.eps.iter().find(|e| e.0 & 0x80 == 0 && e.1 & 3 == 2) else { return Ok(None) };
    let in_dci = (ina as u32 & 0xf) * 2 + 1;
    let out_dci = (outa as u32 & 0xf) * 2;

    // Interrupt-IN (Meldung) der Steuer-Schnittstelle, falls vorhanden
    let intr = ctrl.eps.iter().find(|e| e.0 & 0x80 != 0 && e.1 & 3 == 3).copied();
    let (intr_dci, intr_mp, intr_iv) = match intr {
        Some((a, _, mp, iv)) => ((a as u32 & 0xf) * 2 + 1, (mp & 0x7ff) as u32, iv),
        None => (0, 0, 0),
    };

    let mut eps: Vec<(u32, u32, u32, u8)> = alloc::vec![(in_dci, 6, inmp as u32, 0), (out_dci, 2, outmp as u32, 0)];
    if intr_dci != 0 {
        eps.push((intr_dci, 7, intr_mp, intr_iv));
    }
    config_ep(hc, slot, ctx.0, ctx.1, ctx.2, &eps).map_err(|_| "ECM: Endpunkte nicht eingerichtet")?;

    // Daten-Alternative einschalten
    hc.control(slot, 0x01, 0x0b, data_alt, data_iface, 0, 0).ok_or("ECM: SET_INTERFACE fehlgeschlagen")?;

    // MAC aus der iMACAddress-Zeichenkette
    let mut mac = [0u8; 6];
    if let Some(idx) = mac_string_index(cd) {
        if let Some(s) = read_string(hc, slot, idx, buf) {
            if let Some(m) = parse_mac(&s) {
                mac = m;
            }
        }
    }
    if mac == [0; 6] {
        mac = [0x02, 0x55, 0x53, 0x42, 0x00, slot as u8];
    }

    let in_ep = InEp::new(in_dci).ok_or("kein Speicher")?;
    let tx = alloc_pages(1).ok_or("kein Speicher")?;
    let intr_buf = if intr_dci != 0 { alloc_pages(1).ok_or("kein Speicher")? } else { 0 };

    let mut nl = NET.lock();
    let name = format!("usb{}", nl.len());
    crate::kprintln!(
        "[usb-net] {name}: CDC-ECM (slot {slot}) MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    nl.push(Box::new(EcmHandle { name: name.clone(), hc_index: hc.index, slot, in_dci, out_dci, mac, tx }));
    drop(nl);
    {
        let mut list = NICS.lock();
        let mut nic = Nic { hc_index: hc.index, slot, name: name.clone(), in_ep, tx, intr_dci, intr_buf, intr_mp, link: true, speed: 0, rx: VecDeque::new() };
        nic.in_ep.arm_all(hc, slot);
        if intr_dci != 0 {
            let ring = hc.ring(slot, intr_dci);
            ring.push(nic.intr_buf, nic.intr_mp.max(8), (TRB_NORMAL << 10) | (1 << 5) | (1 << 2));
            w32(hc.db + slot as u64 * 4, intr_dci);
        }
        list.push(nic);
    }
    Ok(Some(name))
}

/// Das Gerät in `slot` wurde abgezogen: Netzkarte aus `NET` nehmen, DMA-Seiten freigeben.
pub(super) fn remove(hc_index: usize, slot: u32) {
    let names: Vec<String> = NICS.lock().iter().filter(|n| n.hc_index == hc_index && n.slot == slot).map(|n| n.name.clone()).collect();
    // erst aus der Netzliste nehmen (unter deren Sperre), dann die DMA-Seiten freigeben.
    for name in &names {
        let _ = crate::driver::remove_net(name);
        crate::kprintln!("[usb-net] {name}: entfernt");
    }
    let mut list = NICS.lock();
    for n in list.iter().filter(|n| n.hc_index == hc_index && n.slot == slot) {
        n.in_ep.free();
        free_page(n.tx);
        if n.intr_buf != 0 {
            free_page(n.intr_buf);
        }
    }
    list.retain(|n| !(n.hc_index == hc_index && n.slot == slot));
}

/// iMACAddress aus dem „Ethernet Networking Functional Descriptor" (CS_INTERFACE, Untertyp 0x0F).
fn mac_string_index(cd: &[u8]) -> Option<u8> {
    let mut o = 0usize;
    while o + 3 <= cd.len() {
        let l = cd[o] as usize;
        if l < 3 || o + l > cd.len() {
            break;
        }
        if cd[o + 1] == 0x24 && cd[o + 2] == 0x0f && l >= 5 {
            return Some(cd[o + 3]);
        }
        o += l;
    }
    None
}

/// USB-Zeichenketten-Deskriptor lesen (Sprachkennung zuerst abfragen), Inhalt als UTF-16-Einheiten.
fn read_string(hc: &mut Hc, slot: u32, idx: u8, buf: u64) -> Option<Vec<u16>> {
    let n = hc.control(slot, 0x80, 6, 0x0300, 0, 255, buf)?;
    if n < 4 {
        return None;
    }
    let lang = rd16(buf, 2);
    let m = hc.control(slot, 0x80, 6, 0x0300 | idx as u16, lang, 255, buf)?;
    if m < 2 {
        return None;
    }
    let count = (m - 2) / 2;
    let mut s: Vec<u16> = Vec::with_capacity(count);
    for i in 0..count {
        s.push(rd16(buf, 2 + i * 2));
    }
    Some(s)
}

/// MAC aus einer Zeichenkette aus (üblicherweise 12) Hex-Ziffern; Trennzeichen wie `:`/`-` werden
/// übersprungen.
fn parse_mac(s: &[u16]) -> Option<[u8; 6]> {
    let mut hex: Vec<u8> = Vec::new();
    for &c in s {
        if c <= 0x7f && (c as u8).is_ascii_hexdigit() {
            hex.push(c as u8);
        }
    }
    if hex.len() < 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = (hexval(hex[i * 2])? << 4) | hexval(hex[i * 2 + 1])?;
    }
    Some(mac)
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}