//! Ultar-Kern – Treiber „gen-hda“ (Treiber-Werkstatt).
//! Gerät: Intel High Definition Audio (Klasse 04.03) · Norm: „High Definition Audio Specification 1.0a“ (Intel, offen):
//! Abschnitt 3 (Register), 4.4 (CORB/RIRB), 4.5 (Ströme, Pufferliste), 7 (Codec-Verben und -Parameter).
//! Umfang: Controller zurücksetzen, Codecs über CORB/RIRB befragen, Ausgabeweg Pin ← … ← DAC finden und einschalten,
//! ein Ausgabestrom 48 kHz / 16 Bit / Stereo im Abfragebetrieb. Schnittstelle: `spiele()`, `ton()`.
//! Dazu laufende Ströme für Programme (`/dev/ultar/pcm`, svcdev/pcm.rs): Ausgabe und Eingabe
//! (Mikrofon oder Line-In) als Ringe, siehe `gen_hda/strom.rs`.
//! Auftrag: werkstatt/auftraege/hda.md · Status: ausgeschrieben, in QEMU (ich9-intel-hda + hda-duplex) geprüft.

use crate::driver::{Driver, Result};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

mod strom;
use strom::Ring;

pub struct Drv;
pub static DRV: Drv = Drv;

const GCAP: u64 = 0x00;
const GCTL: u64 = 0x08;
const STATESTS: u64 = 0x0e;
const INTCTL: u64 = 0x20;
const CORBLBASE: u64 = 0x40;
const CORBUBASE: u64 = 0x44;
const CORBWP: u64 = 0x48;
const CORBRP: u64 = 0x4a;
const CORBCTL: u64 = 0x4c;
const CORBSIZE: u64 = 0x4e;
const RIRBLBASE: u64 = 0x50;
const RIRBUBASE: u64 = 0x54;
const RIRBWP: u64 = 0x58;
const RINTCNT: u64 = 0x5a;
const RIRBCTL: u64 = 0x5c;
const RIRBSTS: u64 = 0x5d;
const RIRBSIZE: u64 = 0x5e;

/// Abspielpuffer: 64 Seiten = 256 KB ≈ 1,36 s bei 48 kHz/16 Bit/Stereo
const BUF_SEITEN: usize = 64;
const RATE: u64 = 48_000;
const STROM: u8 = 1;
/// 48 kHz, 16 Bit, 2 Kanäle (Abschnitt 3.7.1)
const FORMAT: u16 = 0x0011;

struct Hda {
    m: u64,
    corb: u64,
    rirb: u64,
    n_corb: u16,
    n_rirb: u16,
    rp: u16,
    codec: u8,
    dac: u8,
    pin: u8,
    name: String,
    /// Output ring for programs (first pages of `buf`, same buffer list).
    out: Ring,
    /// Input ring and its ADC, if the codec has a way in.
    inn: Option<Ring>,
    adc: Option<u8>,
    /// A program holds `/dev/ultar/pcm`.
    offen: bool,
}

static HDA: Mutex<Option<Hda>> = Mutex::new(None);

fn r8(a: u64) -> u8 {
    unsafe { core::ptr::read_volatile(a as *const u8) }
}
fn w8(a: u64, v: u8) {
    unsafe { core::ptr::write_volatile(a as *mut u8, v) }
}
fn r16(a: u64) -> u16 {
    unsafe { core::ptr::read_volatile(a as *const u16) }
}
fn w16(a: u64, v: u16) {
    unsafe { core::ptr::write_volatile(a as *mut u16, v) }
}
fn r32(a: u64) -> u32 {
    unsafe { core::ptr::read_volatile(a as *const u32) }
}
fn w32(a: u64, v: u32) {
    unsafe { core::ptr::write_volatile(a as *mut u32, v) }
}

fn wait(mut f: impl FnMut() -> bool, ms: u64) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    while crate::arch::uptime_ms() <= end {
        if f() {
            return true;
        }
        crate::arch::pause();
    }
    f()
}

/// Ringgröße aus dem Größenregister wählen (größte unterstützte): Bits 4..6 = 2/16/256 Einträge möglich
fn ring(reg: u64) -> u16 {
    let cap = r8(reg) >> 4;
    let (sel, n) = if cap & 4 != 0 {
        (2, 256)
    } else if cap & 2 != 0 {
        (1, 16)
    } else {
        (0, 2)
    };
    w8(reg, (r8(reg) & !3) | sel);
    n
}

impl Hda {
    /// Verb (20 Bit) an einen Knoten senden, Antwort aus dem RIRB holen
    fn cmd(&mut self, nid: u8, verb: u32) -> Result<u32> {
        let m = self.m;
        let wort = (self.codec as u32) << 28 | (nid as u32) << 20 | (verb & 0xf_ffff);
        let wp = ((r16(m + CORBWP) & 0xff) + 1) % self.n_corb;
        w32(self.corb + wp as u64 * 4, wort);
        w16(m + CORBWP, wp);
        let rp = self.rp;
        if !wait(|| r16(m + RIRBWP) & 0xff != rp, 100) {
            return Err(format!("Codec {} antwortet nicht (Knoten {nid}, Verb {verb:#x})", self.codec));
        }
        self.rp = (self.rp + 1) % self.n_rirb;
        let v = r32(self.rirb + self.rp as u64 * 8);
        // Antwort bestätigen (sonst hält der Controller nach RINTCNT Antworten an)
        w8(m + RIRBSTS, 0x05);
        Ok(v)
    }
    fn param(&mut self, nid: u8, id: u8) -> Result<u32> {
        self.cmd(nid, 0xF_0000 | id as u32)
    }
    /// Verbindungsliste eines Knotens (Abschnitt 7.3.3.3), Bereiche aufgelöst
    fn verbindungen(&mut self, nid: u8) -> Vec<u8> {
        let mut aus = Vec::new();
        let Ok(len) = self.param(nid, 0x0e) else { return aus };
        let lang = len & 0x80 != 0;
        let n = (len & 0x7f) as usize;
        let je = if lang { 2 } else { 4 };
        let mut i = 0;
        while i < n {
            let Ok(r) = self.cmd(nid, 0xF_0200 | i as u32) else { break };
            for k in 0..je {
                if i + k >= n {
                    break;
                }
                let e = if lang { (r >> (16 * k)) & 0xffff } else { (r >> (8 * k)) & 0xff };
                let bereich = if lang { e & 0x8000 != 0 } else { e & 0x80 != 0 };
                let id = (if lang { e & 0x7fff } else { e & 0x7f }) as u8;
                match aus.last().copied() {
                    Some(l) if bereich => (l + 1..=id).for_each(|x| aus.push(x)),
                    _ => aus.push(id),
                }
            }
            i += je;
        }
        aus
    }
    /// Weg von `nid` bis zu einem DAC suchen (Tiefensuche), unterwegs die Eingänge passend wählen
    fn suche(&mut self, nid: u8, tiefe: u8, weg: &mut Vec<u8>) -> bool {
        let typ = (self.param(nid, 0x09).unwrap_or(0xf0_0000) >> 20) & 0xf;
        weg.push(nid);
        if typ == 0 {
            return true;
        }
        if tiefe > 0 && typ != 1 {
            let c = self.verbindungen(nid);
            for (i, &n) in c.iter().enumerate() {
                if self.suche(n, tiefe - 1, weg) {
                    if c.len() > 1 && typ != 2 {
                        let _ = self.cmd(nid, 0x7_0100 | i as u32);
                    }
                    return true;
                }
            }
        }
        weg.pop();
        false
    }
    /// Knoten einschalten und Verstärker auf 0 dB, nicht stumm
    fn laut(&mut self, nid: u8, afg: u8) {
        let _ = self.cmd(nid, 0x7_0500);
        let caps = self.param(nid, 0x09).unwrap_or(0);
        let typ = (caps >> 20) & 0xf;
        let eigen = caps & (1 << 3) != 0;
        if caps & (1 << 2) != 0 {
            let a = self.param(if eigen { nid } else { afg }, 0x12).unwrap_or(0);
            let _ = self.cmd(nid, 0x3_0000 | 0xb000 | (a & 0x7f));
        }
        if caps & (1 << 1) != 0 && (typ == 2 || typ == 3) {
            let a = self.param(if eigen { nid } else { afg }, 0x0d).unwrap_or(0);
            let n = self.verbindungen(nid).len().clamp(1, 16) as u32;
            for i in 0..n {
                let _ = self.cmd(nid, 0x3_0000 | 0x7000 | i << 8 | (a & 0x7f));
            }
        }
    }
    /// Ausgabeweg in diesem Codec suchen und einschalten: (Pin, DAC)
    fn weg_einrichten(&mut self) -> Result<(u8, u8, u32)> {
        let kennung = self.param(0, 0x00)?;
        let gruppen = self.param(0, 0x04)?;
        for fg in (gruppen >> 16) as u8..((gruppen >> 16) as u8).saturating_add(gruppen as u8) {
            if self.param(fg, 0x05)? & 0xff != 1 {
                continue;
            }
            let _ = self.cmd(fg, 0x7_0500);
            crate::arch::sleep_us(10_000);
            let knoten = self.param(fg, 0x04)?;
            let (start, anzahl) = ((knoten >> 16) as u8, knoten as u8);
            // Ausgänge nach Vorrang: Lautsprecher, Kopfhörer, Line-Out, dann jeder ausgabefähige Pin
            let mut pins: Vec<(u8, u8)> = Vec::new();
            for nid in start..start.saturating_add(anzahl) {
                if (self.param(nid, 0x09).unwrap_or(0) >> 20) & 0xf != 4 || self.param(nid, 0x0c).unwrap_or(0) & (1 << 4) == 0 {
                    continue;
                }
                let cfg = self.cmd(nid, 0xF_1C00).unwrap_or(0);
                if cfg >> 30 == 1 {
                    continue; // nicht bestückt
                }
                let rang = match (cfg >> 20) & 0xf {
                    1 => 0,
                    2 => 1,
                    0 => 2,
                    _ => 3,
                };
                pins.push((rang, nid));
            }
            pins.sort();
            for (_, pin) in pins {
                let mut weg = Vec::new();
                if !self.suche(pin, 5, &mut weg) {
                    continue;
                }
                let dac = *weg.last().unwrap();
                for &n in &weg {
                    self.laut(n, fg);
                }
                // Pin: Ausgang an (Kopfhörerverstärker mit), EAPD falls vorhanden
                let _ = self.cmd(pin, 0x7_0700 | 0xc0);
                if self.param(pin, 0x0c).unwrap_or(0) & (1 << 16) != 0 {
                    let _ = self.cmd(pin, 0x7_0c00 | 0x02);
                }
                let _ = self.cmd(dac, 0x2_0000 | FORMAT as u32);
                let _ = self.cmd(dac, 0x7_0600 | (STROM as u32) << 4);
                return Ok((pin, dac, kennung));
            }
        }
        Err("kein Ausgabeweg (Pin ← DAC) gefunden".into())
    }

    /// Abtastwerte (16 Bit, Stereo verschachtelt) abspielen; wartet bis zum Ende. Liefert gespielte Bytes.
    fn spiele(&mut self, werte: &[i16]) -> Result<usize> {
        if self.offen {
            // a program holds the streams (/dev/ultar/pcm): nothing is played
            return Ok(0);
        }
        let bytes = (werte.len() * 2).min(BUF_SEITEN * 4096) & !7;
        if bytes < 16 {
            return Ok(0);
        }
        unsafe { core::ptr::copy_nonoverlapping(werte.as_ptr() as *const u8, self.out.buf as *mut u8, bytes) };
        let (sd, bdl, buf) = (self.out.sd, self.out.bdl, self.out.buf);
        // Strom anhalten und zurücksetzen (3.3.35)
        w8(sd, r8(sd) & !2);
        wait(|| r8(sd) & 2 == 0, 20);
        w8(sd, 1);
        wait(|| r8(sd) & 1 != 0, 20);
        w8(sd, 0);
        wait(|| r8(sd) & 1 == 0, 20);
        // Pufferliste: mindestens zwei Einträge; Meldung (IOC) am Ende des letzten
        let halb = (bytes / 2) & !3;
        unsafe {
            let b = bdl as *mut u64;
            b.write_volatile(buf);
            b.add(1).write_volatile(halb as u64);
            b.add(2).write_volatile(buf + halb as u64);
            b.add(3).write_volatile((bytes - halb) as u64 | 1 << 32);
        }
        w32(sd + 0x18, bdl as u32);
        w32(sd + 0x1c, (bdl >> 32) as u32);
        w32(sd + 0x08, bytes as u32);
        w16(sd + 0x0c, 1);
        w16(sd + 0x12, FORMAT);
        w8(sd + 2, STROM << 4);
        w8(sd + 3, 0x1c);
        let _ = self.cmd(self.dac, 0x2_0000 | FORMAT as u32);
        let _ = self.cmd(self.dac, 0x7_0600 | (STROM as u32) << 4);
        w8(sd, 2);
        let dauer = bytes as u64 * 1000 / (RATE * 4);
        let fertig = wait(|| r8(sd + 3) & 0x04 != 0, dauer + 1500);
        w8(sd, 0);
        wait(|| r8(sd) & 2 == 0, 20);
        w8(sd + 3, 0x1c);
        if !fertig {
            return Err("Ausgabestrom läuft nicht (kein Pufferende gemeldet)".into());
        }
        Ok(bytes)
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-hda"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x04 && d.subclass == 0x03
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?;
        d.enable();
        let m = bar.addr;
        // Controller zurücksetzen (4.2.2): CRST 0 → 1, danach brauchen Codecs mindestens 521 µs
        w32(m + GCTL, r32(m + GCTL) & !1);
        if !wait(|| r32(m + GCTL) & 1 == 0, 100) {
            return Err("Rücksetzen hängt".into());
        }
        crate::arch::sleep_us(200);
        w32(m + GCTL, r32(m + GCTL) | 1);
        if !wait(|| r32(m + GCTL) & 1 != 0, 100) {
            return Err("Controller kommt nicht aus dem Rücksetzen".into());
        }
        crate::arch::sleep_us(2000);
        w32(m + INTCTL, 0);
        let gcap = r16(m + GCAP);
        let ringe = alloc_pages(1).ok_or("kein Speicher")?;
        let bdl = alloc_pages(1).ok_or("kein Speicher")?;
        let buf = alloc_pages(BUF_SEITEN).ok_or("kein Speicher")?;
        if gcap & 1 == 0 && (ringe | bdl | (buf + (BUF_SEITEN * 4096) as u64)) >> 32 != 0 {
            return Err("Controller kann nur 32-Bit-Adressen, Speicher liegt darüber".into());
        }
        let (corb, rirb, n_corb, n_rirb) = befehlsringe(m, ringe);
        let codecs = r16(m + STATESTS) & 0x7fff;
        if codecs == 0 {
            return Err("kein Codec angeschlossen".into());
        }
        // erster Ausgabestrom liegt hinter den Eingabeströmen
        let (iss, oss) = ((gcap >> 8) & 0xf, (gcap >> 12) & 0xf);
        if oss == 0 {
            return Err("Controller hat keinen Ausgabestrom".into());
        }
        let sd = m + 0x80 + iss as u64 * 0x20;
        // input ring on the first input stream, if the controller has one
        let inn = if iss > 0 {
            let cbdl = alloc_pages(1).ok_or("kein Speicher")?;
            let cbuf = alloc_pages(strom::RING_PAGES).ok_or("kein Speicher")?;
            Some(Ring::new(m + 0x80, cbdl, cbuf))
        } else {
            None
        };
        let out = Ring::new(sd, bdl, buf);
        let h = Hda { m, corb, rirb, n_corb, n_rirb, rp: 0, codec: 0, dac: 0, pin: 0, name: String::new(), out, inn, adc: None, offen: false };
        h.codec_suchen(codecs, iss, oss)
    }
}

impl Hda {
    /// Den ersten Codec mit Ausgabeweg (und, wenn vorhanden, Eingang) einrichten und eintragen.
    fn codec_suchen(mut self, codecs: u16, iss: u16, oss: u16) -> Result<()> {
        let mut fehler = String::from("kein Codec mit Ausgabe");
        for c in 0..15u8 {
            if codecs & (1 << c) == 0 {
                continue;
            }
            self.codec = c;
            match self.weg_einrichten() {
                Ok((pin, dac, kennung)) => {
                    self.pin = pin;
                    self.dac = dac;
                    self.name = format!("Codec {c} {:04x}:{:04x}", kennung >> 16, kennung & 0xffff);
                    crate::kprintln!("[hda] {}: Ausgang Pin {pin} ← DAC {dac}, 48 kHz 16 Bit Stereo ({iss} Ein-/{oss} Ausgabeströme)", self.name);
                    if self.inn.is_some() {
                        match self.afg().and_then(|fg| self.ein_einrichten(fg)) {
                            Some((adc, ein)) => {
                                self.adc = Some(adc);
                                crate::kprintln!("[hda] {}: Eingang Pin {ein} → ADC {adc}", self.name);
                            }
                            None => crate::kprintln!("[hda] {}: kein Eingang (Mikrofon/Line-In) gefunden", self.name),
                        }
                    }
                    *HDA.lock() = Some(self);
                    return Ok(());
                }
                Err(e) => fehler = e,
            }
        }
        Err(fehler)
    }

    /// The audio function group of the codec.
    fn afg(&mut self) -> Option<u8> {
        let gruppen = self.param(0, 0x04).ok()?;
        let erste = (gruppen >> 16) as u8;
        (erste..erste.saturating_add(gruppen as u8)).find(|&fg| self.param(fg, 0x05).is_ok_and(|t| t & 0xff == 1))
    }
}

/// Why `/dev/ultar/pcm` cannot be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmError {
    /// No sound card with output.
    Gone,
    /// Another program holds it.
    Busy,
}

/// Opens the running streams for one program.
pub fn pcm_open() -> core::result::Result<(), PcmError> {
    let mut g = HDA.lock();
    let h = g.as_mut().ok_or(PcmError::Gone)?;
    if h.offen {
        return Err(PcmError::Busy);
    }
    h.offen = true;
    h.pcm_start();
    Ok(())
}

/// The program let go: the streams stop.
pub fn pcm_close() {
    if let Some(h) = HDA.lock().as_mut() {
        h.pcm_stop();
        h.offen = false;
    }
}

/// Queues sound (48 kHz, 16 bit, stereo interleaved, little endian); returns the bytes taken.
pub fn pcm_write(data: &[u8]) -> usize {
    HDA.lock().as_mut().map_or(0, |h| h.pcm_write(data))
}

/// Reads recorded sound (same format); returns the bytes read.
pub fn pcm_read(out: &mut [u8]) -> usize {
    HDA.lock().as_mut().map_or(0, |h| h.pcm_read(out))
}

/// Whether recorded sound is waiting.
pub fn pcm_readable() -> bool {
    HDA.lock().as_mut().is_some_and(|h| h.pcm_readable())
}

/// Whether the output has room.
pub fn pcm_writable() -> bool {
    HDA.lock().as_mut().is_some_and(|h| h.pcm_writable())
}

/// Whether a sound card is there (the node exists).
pub fn pcm_present() -> bool {
    HDA.lock().is_some()
}

/// Beschreibung der Tonausgabe (für `ton` ohne Angaben und die Geräteliste)
pub fn info() -> Option<String> {
    HDA.lock().as_ref().map(|h| format!("{} · Pin {} ← DAC {} · 48 kHz 16 Bit Stereo", h.name, h.pin, h.dac))
}

/// Abtastwerte (16 Bit, Stereo verschachtelt, 48 kHz) ausgeben – höchstens ~1,36 s je Aufruf
pub fn spiele(werte: &[i16]) -> Result<usize> {
    HDA.lock().as_mut().ok_or("keine Tonausgabe (gen-hda nicht aktiv)")?.spiele(werte)
}

/// Prüfton: `hz` Hertz für `ms` Millisekunden (Sinus-Näherung, weiches Ein-/Ausblenden)
pub fn ton(hz: u64, ms: u64) -> Result<usize> {
    let hz = hz.clamp(20, 20_000);
    let n = (RATE * ms.clamp(10, 1300) / 1000) as usize;
    let halb = RATE / 2;
    let mut w = Vec::with_capacity(n * 2);
    for i in 0..n {
        let ph = (i as u64 * hz) % RATE;
        let (vz, q) = if ph < halb { (1i64, ph) } else { (-1i64, ph - halb) };
        let y = (4 * q * (halb - q)) as i64 * 9000 / (halb * halb) as i64;
        let rand = i.min(n - 1 - i).min(240) as i64;
        let s = (vz * y * rand / 240) as i16;
        w.push(s);
        w.push(s);
    }
    spiele(&w)
}

/// CORB (Befehle) und RIRB (Antworten) in der Seite `ringe` einrichten und starten.
fn befehlsringe(m: u64, ringe: u64) -> (u64, u64, u16, u16) {
    // CORB (Befehle) und RIRB (Antworten) in einer Seite: 1 KB + 2 KB
    w8(m + CORBCTL, 0);
    w8(m + RIRBCTL, 0);
    wait(|| r8(m + CORBCTL) & 2 == 0, 20);
    let n_corb = ring(m + CORBSIZE);
    let n_rirb = ring(m + RIRBSIZE);
    let (corb, rirb) = (ringe, ringe + 2048);
    w32(m + CORBLBASE, corb as u32);
    w32(m + CORBUBASE, (corb >> 32) as u32);
    w16(m + CORBWP, 0);
    w16(m + CORBRP, 0x8000);
    wait(|| r16(m + CORBRP) & 0x8000 != 0, 10);
    w16(m + CORBRP, 0);
    wait(|| r16(m + CORBRP) & 0x8000 == 0, 10);
    w32(m + RIRBLBASE, rirb as u32);
    w32(m + RIRBUBASE, (rirb >> 32) as u32);
    w16(m + RIRBWP, 0x8000);
    w16(m + RINTCNT, 1);
    w8(m + RIRBSTS, 0x05);
    // RINTCTL an: der Controller zählt Antworten und meldet sie in RIRBSTS (wird nach jeder Antwort quittiert);
    // eine Unterbrechung entsteht nicht, weil INTCTL aus ist (Abfragebetrieb)
    w8(m + RIRBCTL, 3);
    w8(m + CORBCTL, 2);
    (corb, rirb, n_corb, n_rirb)
}
