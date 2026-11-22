//! Mobilfunk in der Oberfläche (Designkonzept „Ultar OS · Mobilfunk, SIM & Funkturm"):
//! - Netz-Pille in der Statusleiste (Tippen = Funkturm-Wahl, lange drücken = Mobilfunk-Zentrum)
//! - Verbindungskarte auf dem Startbildschirm und Netz-Leiste in den Schnelleinstellungen
//! - Mobilfunk-Zentrum (Dual-SIM, Live-Signal, SIM-Rollen, PIN)
//! - Funkturm-Wahl (Radar, Auto / Smart / Manuell, Zellenliste)
//! - Verbindungsweg (Gerät → Zelle/WLAN → Anbieter → VPN → Internet)
//!
//! Farbregel: Gold = deine Wahl / aktiv, Blau = Funk und Netz, Orange = Warnung/Last.
//! Daten kommen vom Funkdienst (`ultar-ril --daemon`) über das gemeinsame Modell `ultar-radio`.
//! `ULTAR_RADIO_DEMO=1` zeigt Beispieldaten (nur für Bildschirmfotos/Gestaltungsprüfung).

use super::{App, Screen};
use crate::gfx::{rgb, Color, Rect};
use crate::quick;
use crate::ui::{Al, Btn, Ic, Ui};
use std::time::{Duration, Instant};
use ultar_radio::{self as radio, Mode, Prefs, RadioState, SimState, Tech};

pub const BLUE: u32 = 0x5FB3D9;
pub const BLUE_L: u32 = 0x9FD3EC;
pub const WARN: u32 = 0xF08A4B;
pub const GOOD: u32 = 0x7ED6A0;
pub const GOLD_L: u32 = 0xE6C65C;

#[derive(Default)]
pub struct RadioUi {
    st: RadioState,
    at: Option<Instant>,
    prefs: Option<Prefs>,
    /// im Mobilfunk-Zentrum gezeigte SIM (1/2)
    sim_view: u8,
}

fn demo_mode() -> bool {
    std::env::var("ULTAR_RADIO_DEMO").is_ok()
}

fn source() -> RadioState {
    if demo_mode() {
        radio::demo()
    } else {
        radio::read_state()
    }
}

/// Kurzname der Funktechnik für die Pille.
pub fn rat_short(rat: &str) -> &'static str {
    match rat {
        "LTE-A" | "LTE+" => "4G+",
        "LTE" => "4G",
        "HSPA" | "HSPA+" => "3G+",
        "UMTS" => "3G",
        "EDGE" => "E",
        "GSM" => "2G",
        "" => "",
        _ => "4G",
    }
}

/// Rechteck für Rechts-nach-links spiegeln.
fn mir(ui: &Ui, r: Rect) -> Rect {
    if ui.rtl {
        Rect::new(ui.w - r.x - r.w, r.y, r.w, r.h)
    } else {
        r
    }
}

/// Vier Signalbalken; liefert die Breite.
pub fn bars(ui: &mut Ui, x: f32, base: f32, h: f32, lvl: u8, on: Color, off: Color) -> f32 {
    let bw = (h * 0.24).max(2.0);
    let gap = (h * 0.12).max(1.0);
    for k in 0..4u8 {
        let bh = h * (0.32 + 0.68 * k as f32 / 3.0);
        let c = if k < lvl { on } else { off };
        ui.cv.rrect(Rect::new(x + k as f32 * (bw + gap), base - bh, bw, bh), (bw * 0.35).max(1.0), c);
    }
    4.0 * bw + 3.0 * gap
}

fn fmt_mbps(v: f32) -> String {
    if v >= 10.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v).replace('.', ",")
    }
}

fn fmt_km(m: u32) -> String {
    if m < 1000 {
        format!("{m} m")
    } else {
        format!("{},{} km", m / 1000, (m % 1000) / 100)
    }
}

impl App {
    /// Aktueller Funk-Zustand (höchstens 1 s alt).
    /// Hat das Gerät Mobilfunk (Modem an oder SIM erkannt)? Sonst wird nichts davon gezeigt.
    pub(super) fn mobilfunk_vorhanden(&mut self) -> bool {
        let st = self.radio();
        st.modem != radio::Modem::Offline || st.sims.iter().any(|s| !matches!(s.state, SimState::Absent | SimState::Unknown))
    }

    pub(super) fn radio(&mut self) -> RadioState {
        let fresh = self.radio_ui.at.is_some_and(|t| t.elapsed() < Duration::from_millis(1000));
        if !fresh {
            self.radio_ui.st = source();
            self.radio_ui.at = Some(Instant::now());
        }
        self.radio_ui.st.clone()
    }

    pub(super) fn rprefs(&mut self) -> Prefs {
        if self.radio_ui.prefs.is_none() {
            self.radio_ui.prefs = Some(radio::read_prefs());
        }
        self.radio_ui.prefs.clone().unwrap_or_default()
    }

    fn save_rprefs(&mut self, p: Prefs) {
        if !demo_mode() {
            if let Err(e) = radio::write_prefs(&p) {
                self.toast(format!("{}: {e}", self.t("Could not save")));
            }
        }
        self.radio_ui.prefs = Some(p);
    }

    pub(super) fn mode_label(&self, m: Mode) -> &'static str {
        match m {
            Mode::Auto => self.t("Auto"),
            Mode::Smart => self.t("Smart"),
            Mode::Manual => self.t("Manual"),
        }
    }

    // ------------------------------------------------------------ Netz-Pille (Statusleiste)

    /// Zeichnet die Netz-Pille ab `x` (Anfangsseite), liefert ihre Breite.
    pub(super) fn net_pill(&mut self, ui: &mut Ui, x: f32, h: f32) -> f32 {
        let st = self.radio();
        let p = self.rprefs();
        let blue = rgb(BLUE);
        let (text, col, lvl, pinned) = if quick::airplane_on() {
            (self.t("Airplane").to_string(), ui.th.sub, 0, false)
        } else if st.modem != radio::Modem::Online {
            (self.t("No service").to_string(), ui.th.sub, 0, false)
        } else if let Some(s) = st.sim(1).filter(|s| matches!(s.state, SimState::Pin | SimState::Puk)) {
            let _ = s;
            (self.t("SIM locked").to_string(), rgb(WARN), 0, false)
        } else if st.sims.iter().all(|s| s.state == SimState::Absent) && !st.sims.is_empty() {
            (self.t("No SIM").to_string(), ui.th.sub, 0, false)
        } else if !st.in_service() {
            (self.t("Searching").to_string(), ui.th.sub, 0, false)
        } else {
            let band = st.serving_cell().map(|c| c.band.clone()).unwrap_or_default();
            let t = if band.is_empty() { rat_short(&st.rat).to_string() } else { format!("{} · {band}", rat_short(&st.rat)) };
            let lvl = radio::level(st.serving_cell().and_then(|c| c.rsrp));
            (t, rgb(BLUE_L), lvl, p.mode == Mode::Manual && p.pinned.is_some())
        };
        let ph = (h - ui.dp(8.0)).min(ui.dp(22.0));
        let fs = 10.5;
        let tw = ui.text_w(&text, fs, true);
        let icon_w = ui.dp(12.0);
        let bars_w = if lvl > 0 || st.in_service() { ui.dp(14.0) } else { 0.0 };
        let dot_w = if pinned { ui.dp(9.0) } else { 0.0 };
        let pad = ui.dp(8.0);
        let w = pad + icon_w + ui.dp(5.0) + tw + if bars_w > 0.0 { ui.dp(6.0) + bars_w } else { 0.0 } + dot_w + pad;
        let r = Rect::new(if ui.rtl { x - w } else { x }, (h - ph) / 2.0, w, ph);
        let held = ui.held(r);
        ui.cv.rrect(r, ph / 2.0, if st.in_service() { blue.alpha(if held { 0.32 } else { 0.38 }) } else { ui.th.line });
        ui.cv.rrect(r.inset(1.0), ph / 2.0 - 1.0, ui.th.bg.mix(blue, if st.in_service() { 0.14 } else { 0.0 }));
        // Inhalt immer von links nach rechts (Symbolfolge ist international)
        let mut cx = r.x + pad;
        ui.icon(Ic::Tower, cx + icon_w / 2.0, r.cy(), icon_w, col);
        cx += icon_w + ui.dp(5.0);
        let old = ui.rtl;
        ui.rtl = false;
        ui.label(Rect::new(cx, r.y, tw + 2.0, r.h), &text, fs, col, true, Al::Start);
        ui.rtl = old;
        cx += tw;
        if bars_w > 0.0 {
            cx += ui.dp(6.0);
            bars(ui, cx, r.cy() + ui.dp(5.0), ui.dp(10.0), lvl, blue, blue.alpha(0.3));
            cx += bars_w;
        }
        if pinned {
            ui.cv.circle(cx + ui.dp(5.0), r.cy(), ui.dp(2.6), rgb(GOLD_L));
        }
        // Tippen = Funkturm-Wahl, lange drücken = Mobilfunk-Zentrum
        if !self.shade {
            if self.long_press(ui, r) {
                self.go(Screen::Radio);
                ui.dirty = true;
            } else if ui.tap(r) {
                self.go(Screen::Cells);
                ui.dirty = true;
            }
        }
        w
    }

    // ------------------------------------------------------------ Startbildschirm-Leiste

    /// Schmale Verbindungsleiste auf dem Startbildschirm: Netz, Zelle, Tempo, Modus. Tippen = Funkturm-Wahl.
    pub(super) fn home_radio_card(&mut self, ui: &mut Ui, r: Rect) {
        let st = self.radio();
        let p = self.rprefs();
        let blue = rgb(BLUE);
        let rad = r.h / 2.0;
        let held = ui.held(r);
        ui.cv.rrect(r, rad, blue.alpha(if held { 0.45 } else { 0.28 }));
        ui.cv.rrect(r.inset(1.0), rad - 1.0, ui.th.card.mix(blue, if held { 0.14 } else { 0.07 }));
        let pad = ui.dp(14.0);
        let ic = ui.dp(16.0);
        let icx = if ui.rtl { r.right() - pad - ic / 2.0 } else { r.x + pad + ic / 2.0 };
        let live = st.in_service();
        ui.icon(Ic::Tower, icx, r.cy(), ic, if live { rgb(BLUE_L) } else { ui.th.sub });
        // Ende: Modus-Marke (Gold) und Tempo
        let mode = self.mode_label(p.mode);
        let mw = ui.text_w(mode, 11.0, true) + ui.dp(16.0);
        let mh = ui.dp(22.0);
        let mr = if ui.rtl { Rect::new(r.x + ui.dp(10.0), r.cy() - mh / 2.0, mw, mh) } else { Rect::new(r.right() - ui.dp(10.0) - mw, r.cy() - mh / 2.0, mw, mh) };
        ui.cv.rrect(mr, mh / 2.0, ui.th.accent);
        let oa = ui.th.on_accent;
        ui.label(mr, mode, 11.0, oa, true, Al::Center);
        let speed = st.serving_cell().filter(|_| live).and_then(radio::est_mbps).map(|v| format!("{} Mbit/s", fmt_mbps(v))).unwrap_or_default();
        let sw = if speed.is_empty() { 0.0 } else { ui.text_w(&speed, 13.0, true) + ui.dp(10.0) };
        let sr = if ui.rtl { Rect::new(mr.right() + ui.dp(4.0), r.y, sw, r.h) } else { Rect::new(mr.x - ui.dp(4.0) - sw, r.y, sw, r.h) };
        let c = ui.th.text;
        if !speed.is_empty() {
            ui.label(sr, &speed, 13.0, c, true, Al::Center);
        }
        // Mitte: Anbieter · Zelle · Band
        let text = if live {
            let op = if st.operator.is_empty() { st.plmn.clone() } else { st.operator.clone() };
            match st.serving_cell() {
                Some(cl) if !cl.band.is_empty() => format!("{op} · {} {} · {}", self.t("Cell"), cl.id, cl.band),
                _ => format!("{op} · {}", rat_short(&st.rat)),
            }
        } else {
            self.t("No mobile network").to_string()
        };
        let tx0 = pad + ic + ui.dp(10.0);
        let tw = r.w - tx0 - mw - sw - ui.dp(18.0);
        let tr = if ui.rtl { Rect::new(sr.right(), r.y, tw, r.h) } else { Rect::new(r.x + tx0, r.y, tw, r.h) };
        let tf = ui.fit(&text, 13.0, false, tw);
        let tc = if live { c } else { ui.th.sub };
        ui.label(tr, &tf, 13.0, tc, false, Al::Start);
        if ui.tap(r) {
            self.go(Screen::Cells);
            ui.dirty = true;
        }
    }

    // ------------------------------------------------------------ Schnelleinstellungen

    /// Netz-Leiste im Schnelleinstellungs-Panel; liefert die belegte Höhe.
    pub(super) fn shade_radio(&mut self, ui: &mut Ui, x: f32, y: f32, w: f32) -> f32 {
        let st = self.radio();
        let mut p = self.rprefs();
        let blue = rgb(BLUE);
        let h = ui.dp(92.0);
        let r = Rect::new(x, y, w, h);
        ui.cv.rrect(r, ui.dp(16.0), blue.alpha(0.3));
        ui.cv.rrect(r.inset(1.0), ui.dp(15.0), ui.th.card.mix(blue, 0.08));
        let pad = ui.dp(12.0);
        let line = Rect::new(r.x + pad, r.y + pad, r.w - 2.0 * pad, ui.dp(20.0));
        let info = match st.serving_cell() {
            Some(c) if st.in_service() => {
                let mut s = format!("{} {}", self.t("Cell"), c.id);
                if !c.band.is_empty() {
                    s += &format!(" · {}", c.band);
                }
                if let Some(v) = c.rsrp {
                    s += &format!(" · {v} dBm");
                }
                s
            }
            _ => self.t("No mobile network").to_string(),
        };
        let link = format!("{} ›", self.t("Towers"));
        let lw = ui.text_w(&link, 13.0, true);
        let ir = if ui.rtl { line.right() - ui.dp(8.0) } else { line.x + ui.dp(8.0) };
        ui.icon(Ic::Tower, ir, line.cy(), ui.dp(14.0), rgb(BLUE_L));
        let tr = Rect::new(if ui.rtl { line.x + lw } else { line.x + ui.dp(22.0) }, line.y, line.w - ui.dp(22.0) - lw, line.h);
        let fi = ui.fit(&info, 13.0, true, tr.w);
        ui.label(tr, &fi, 13.0, rgb(BLUE_L), true, Al::Start);
        let lr = if ui.rtl { Rect::new(line.x, line.y, lw, line.h) } else { Rect::new(line.right() - lw, line.y, lw, line.h) };
        let ac = rgb(GOLD_L);
        ui.label(lr, &link, 13.0, ac, true, Al::Center);
        if ui.tap(lr.inset(-ui.dp(10.0))) {
            self.shade = false;
            self.go(Screen::Cells);
            ui.dirty = true;
        }
        // Modus-Wahl (Blau = Funk)
        let sr = Rect::new(r.x + pad, line.bottom() + ui.dp(8.0), r.w - 2.0 * pad, ui.dp(40.0));
        ui.cv.rrect(sr, ui.dp(12.0), rgb(0).alpha(0.3));
        let opts = [Mode::Auto, Mode::Smart, Mode::Manual];
        let cw = sr.w / 3.0;
        for (i, m) in opts.iter().enumerate() {
            let idx = if ui.rtl { 2 - i } else { i };
            let cr = Rect::new(sr.x + cw * idx as f32, sr.y, cw, sr.h).inset(ui.dp(4.0));
            let on = p.mode == *m;
            if on {
                ui.cv.rrect(cr, ui.dp(9.0), blue);
            }
            let col = if on { rgb(0x07131A) } else { ui.th.sub.mix(ui.th.text, 0.3) };
            let lab = self.mode_label(*m);
            ui.label(cr, lab, 13.0, col, true, Al::Center);
            if ui.tap(cr) && !on {
                p.mode = *m;
                self.save_rprefs(p.clone());
                ui.dirty = true;
            }
        }
        h
    }

    // ------------------------------------------------------------ Mobilfunk-Zentrum

    pub(super) fn s_radio(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Mobile network"));
        let st = self.radio();
        let mut p = self.rprefs();
        if self.radio_ui.sim_view == 0 {
            self.radio_ui.sim_view = 1;
        }
        let view = self.radio_ui.sim_view;
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let m = ui.dp(12.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let blue = rgb(BLUE);

        // SIM-Karten nebeneinander
        let gap = ui.dp(10.0);
        let cw = (ui.w - 2.0 * m - gap) / 2.0;
        let ch = ui.dp(96.0);
        for slot in 1..=2u8 {
            let sim = st.sim(slot).cloned();
            let r = mir(ui, Rect::new(m + (slot - 1) as f32 * (cw + gap), y, cw, ch));
            let sel = view == slot;
            let rad = ui.dp(16.0);
            ui.cv.rrect(r, rad, if sel { ui.th.accent } else { ui.th.line });
            ui.cv.rrect(r.inset(if sel { 2.0 } else { 1.0 }), rad, if sel { ui.th.card.mix(ui.th.accent, 0.1) } else { ui.th.card });
            let inr = r.inset(ui.dp(12.0));
            let state = sim.as_ref().map(|s| s.state).unwrap_or(if st.modem == radio::Modem::Online { SimState::Absent } else { SimState::Unknown });
            let locked = matches!(state, SimState::Pin | SimState::Puk);
            let mut tag = format!("SIM {slot}");
            if p.data_sim == slot {
                tag += &format!(" · {}", self.t("DATA"));
            }
            ui.label(Rect::new(inr.x, inr.y, inr.w, ui.dp(14.0)), &tag, 10.5, if locked { rgb(WARN) } else { rgb(GOLD_L) }, true, Al::Start);
            if slot == 1 && state == SimState::Ready && st.in_service() {
                let lvl = radio::level(st.serving_cell().and_then(|c| c.rsrp));
                let bx = if ui.rtl { inr.x } else { inr.right() - ui.dp(16.0) };
                bars(ui, bx, inr.y + ui.dp(12.0), ui.dp(12.0), lvl, blue, ui.th.line);
            }
            let op = match &sim {
                Some(s) if !s.operator.is_empty() => s.operator.clone(),
                Some(s) if s.state == SimState::Ready && !st.operator.is_empty() && slot == 1 => st.operator.clone(),
                Some(_) => self.t("SIM card").to_string(),
                None => "—".to_string(),
            };
            let opf = ui.fit(&op, 15.0, true, inr.w);
            ui.label(Rect::new(inr.x, inr.y + ui.dp(24.0), inr.w, ui.dp(20.0)), &opf, 15.0, c, true, Al::Start);
            let stt = match state {
                SimState::Ready if slot == 1 && st.in_service() => format!("{} · {}", self.t("Connected"), rat_short(&st.rat)),
                SimState::Ready => self.t("Ready").to_string(),
                SimState::Pin => self.t("PIN required").to_string(),
                SimState::Puk => self.t("PUK required").to_string(),
                SimState::Locked => self.t("Network locked").to_string(),
                SimState::Absent => self.t("No SIM").to_string(),
                SimState::Error => self.t("SIM error").to_string(),
                SimState::Unknown => self.t("Unknown").to_string(),
            };
            let stf = ui.fit(&stt, 12.0, false, inr.w);
            ui.label(Rect::new(inr.x, inr.y + ui.dp(48.0), inr.w, ui.dp(18.0)), &stf, 12.0, cs, false, Al::Start);
            if ui.tap(r) {
                self.radio_ui.sim_view = slot;
                ui.dirty = true;
            }
        }
        y += ch + ui.dp(12.0);

        // Inhalt der gewählten SIM
        let sim = st.sim(view).cloned();
        let state = sim.as_ref().map(|s| s.state).unwrap_or(SimState::Unknown);
        if st.modem != radio::Modem::Online {
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(10.0));
            let mut hh = ui.dp(14.0);
            let tw = r.w - ui.dp(28.0);
            let body = if st.note.is_empty() { self.t("The radio service is not running yet. It starts automatically once the modem is online.").to_string() } else { st.note.clone() };
            let ph = ui.tx.measure(&body, ui.dp(12.5), false, tw).1;
            hh += ui.dp(22.0) + ph + ui.dp(14.0);
            let r = Rect::new(r.x, r.y, r.w, hh);
            ui.card(r);
            ui.label(Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(12.0), tw, ui.dp(20.0)), self.t("Modem not ready"), 15.0, c, true, Al::Start);
            ui.para(r.x + ui.dp(14.0), r.y + ui.dp(36.0), tw, &body, 12.5, cs);
            y += hh + ui.dp(12.0);
        } else if matches!(state, SimState::Pin | SimState::Puk) {
            let warn = rgb(WARN);
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(204.0));
            ui.cv.rrect(r, ui.dp(18.0), warn.alpha(0.4));
            ui.cv.rrect(r.inset(1.0), ui.dp(17.0), ui.th.card.mix(warn, 0.07));
            let x = r.x + ui.dp(14.0);
            let w = r.w - ui.dp(28.0);
            ui.label(Rect::new(x, r.y + ui.dp(10.0), w, ui.dp(22.0)), &format!("SIM {view} · {}", self.t("is locked")), 15.0, warn.mix(rgb(0xFFFFFF), 0.3), true, Al::Start);
            ui.para(x, r.y + ui.dp(36.0), w, self.t("Enter the PIN to use this SIM. After 3 wrong attempts the PUK is needed."), 12.5, c);
            let val = self.fields.get("simpin").cloned().unwrap_or_default();
            let shown: String = "•".repeat(val.chars().count());
            ui.field(Rect::new(x, r.y + ui.dp(90.0), w, ui.dp(48.0)), "simpin", &shown, self.t("SIM PIN"));
            if ui.button(Rect::new(x, r.y + ui.dp(146.0), w, ui.dp(46.0)), self.t("Unlock"), Btn::Primary) {
                let digits: String = val.chars().filter(|ch| ch.is_ascii_digit()).collect();
                if !(4..=8).contains(&digits.len()) {
                    self.toast(self.t("The PIN has 4 to 8 digits").into());
                } else {
                    match radio::send_cmd(&format!("pin {view} {digits}")) {
                        Ok(()) => self.toast(self.t("PIN sent to the modem").into()),
                        Err(e) => self.toast(format!("{}: {e}", self.t("Error"))),
                    }
                    self.fields.insert("simpin", String::new());
                    self.focus = None;
                    self.focus_reset = true;
                }
                ui.dirty = true;
            }
            y += r.h + ui.dp(12.0);
        } else if state == SimState::Ready && view == 1 && st.in_service() {
            let sc = st.serving_cell().cloned().unwrap_or_default();
            let mets: Vec<(&str, &str, String, f32)> = [
                ("RSRP", self.t("Signal strength"), sc.rsrp.map(|v| (format!("{v} dBm"), ((v + 125) as f32 / 50.0)))),
                ("SINR", self.t("Interference margin"), sc.sinr.map(|v| (format!("{v} dB"), ((v + 5) as f32 / 30.0)))),
                ("RSRQ", self.t("Quality"), sc.rsrq.map(|v| (format!("{v} dB"), ((v + 20) as f32 / 17.0)))),
            ]
            .into_iter()
            .filter_map(|(k, n, v)| v.map(|(s, f)| (k, n, s, f.clamp(0.02, 1.0))))
            .collect();
            let hh = ui.dp(56.0) + mets.len() as f32 * ui.dp(40.0) + ui.dp(58.0);
            let r = Rect::new(m, y, ui.w - 2.0 * m, hh);
            ui.cv.rrect(r, ui.dp(18.0), blue.alpha(0.3));
            ui.cv.rrect(r.inset(1.0), ui.dp(17.0), ui.th.card.mix(blue, 0.06));
            let x = r.x + ui.dp(14.0);
            let w = r.w - ui.dp(28.0);
            let mut head = format!("{} · {}", self.t("Live"), st.rat);
            if !sc.band.is_empty() {
                head += &format!(" · {}", sc.band);
            }
            ui.label(Rect::new(x, r.y + ui.dp(12.0), w, ui.dp(18.0)), &head, 13.0, rgb(BLUE_L), true, Al::Start);
            let mut idt = format!("{} {}", self.t("Cell"), sc.id);
            if let Some(pci) = sc.pci {
                idt += &format!(" · PCI {pci}");
            }
            ui.label(Rect::new(x, r.y + ui.dp(12.0), w, ui.dp(18.0)), &idt, 11.5, cs, false, Al::End);
            let mut yy = r.y + ui.dp(44.0);
            for (k, n, v, f) in &mets {
                ui.label(Rect::new(x, yy, w, ui.dp(18.0)), &format!("{k} · {n}"), 12.5, cs, false, Al::Start);
                ui.label(Rect::new(x, yy, w, ui.dp(18.0)), v, 13.0, c, true, Al::End);
                let br = Rect::new(x, yy + ui.dp(22.0), w, ui.dp(6.0));
                ui.cv.rrect(br, ui.dp(3.0), ui.th.line);
                let fw = br.w * f;
                let fr = if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) };
                ui.cv.rrect(fr, ui.dp(3.0), blue);
                yy += ui.dp(40.0);
            }
            let lbl = format!("{} · {}", self.t("Choose tower"), self.mode_label(p.mode));
            if ui.button(Rect::new(x, yy + ui.dp(4.0), w, ui.dp(44.0)), &lbl, Btn::Primary) {
                self.go(Screen::Cells);
                ui.dirty = true;
            }
            y += hh + ui.dp(12.0);
        } else {
            let msg = match state {
                SimState::Absent => self.t("No SIM card in this slot."),
                SimState::Ready => self.t("SIM ready – searching for network."),
                SimState::Locked => self.t("This SIM is locked to another network."),
                _ => self.t("The modem reports no state for this slot yet."),
            };
            self.caption(ui, &mut y, msg);
        }

        // Welche SIM wofür
        ui.section(&mut y, self.t("Which SIM for what"));
        self.caption(ui, &mut y, self.t("Mobile data"));
        if let Some(i) = ui.chips(&mut y, &["SIM 1", "SIM 2"], (p.data_sim.max(1) - 1) as usize) {
            p.data_sim = i as u8 + 1;
            self.save_rprefs(p.clone());
        }
        let ask = self.t("Ask");
        let idx = |v: u8| if v == 0 { 2 } else { (v - 1) as usize };
        let back = |i: usize| if i == 2 { 0 } else { i as u8 + 1 };
        self.caption(ui, &mut y, self.t("Calls"));
        if let Some(i) = ui.chips(&mut y, &["SIM 1", "SIM 2", ask], idx(p.call_sim)) {
            p.call_sim = back(i);
            self.save_rprefs(p.clone());
        }
        self.caption(ui, &mut y, self.t("SMS"));
        if let Some(i) = ui.chips(&mut y, &["SIM 1", "SIM 2", ask], idx(p.sms_sim)) {
            p.sms_sim = back(i);
            self.save_rprefs(p.clone());
        }

        // Netz
        ui.section(&mut y, self.t("Network"));
        let ml = self.mode_label(p.mode);
        if ui.row(&mut y, Some(Ic::Tower), self.t("Network selection"), None, Some(ml), true) {
            self.go(Screen::Cells);
            ui.dirty = true;
        }
        self.caption(ui, &mut y, self.t("Preferred technology"));
        if let Some(i) = ui.chips(&mut y, &[self.t("Auto"), "4G", "3G", "2G"], p.tech.index()) {
            p.tech = Tech::from_index(i);
            self.save_rprefs(p.clone());
        }
        let mut roam = p.roaming;
        if ui.toggle_row(&mut y, self.t("Roaming"), Some(self.t("Use partner networks abroad (may cost extra)")), &mut roam) {
            p.roaming = roam;
            self.save_rprefs(p.clone());
        }
        if ui.row(&mut y, Some(Ic::Globe), self.t("Connection path"), Some(self.t("Device · tower · operator · VPN · internet")), None, true) {
            self.go(Screen::Route);
            ui.dirty = true;
        }
        if demo_mode() {
            self.caption(ui, &mut y, self.t("Example data (demo mode)"));
        }
        y += ui.dp(8.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Funkturm-Wahl

    pub(super) fn s_cells(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Cell towers"));
        let st = self.radio();
        let mut p = self.rprefs();
        let blue = rgb(BLUE);
        let gold = rgb(GOLD_L);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(12.0);
        // fester Knopf unten, Liste darüber scrollt
        let btn_h = ui.dp(50.0);
        let foot = btn_h + ui.dp(34.0);
        let list = Rect::new(area.x, area.y, area.w, (area.h - foot).max(ui.dp(100.0)));
        ui.scroll_begin(list, sc);
        let mut y = list.y - *sc;

        if !st.in_service() {
            let tw = ui.w - 2.0 * m - ui.dp(28.0);
            let body = if st.note.is_empty() { self.t("Cells appear here as soon as the modem is registered in the network.").to_string() } else { st.note.clone() };
            let ph = ui.tx.measure(&body, ui.dp(12.5), false, tw).1;
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(120.0) + ph);
            ui.card(r);
            ui.icon(Ic::Tower, r.cx(), r.y + ui.dp(36.0), ui.dp(34.0), cs);
            ui.label(Rect::new(r.x, r.y + ui.dp(62.0), r.w, ui.dp(22.0)), self.t("No mobile network"), 16.0, c, true, Al::Center);
            ui.para(r.x + ui.dp(14.0), r.y + ui.dp(92.0), tw, &body, 12.5, cs);
            y += r.h;
            ui.scroll_end(list, y + *sc - list.y, sc);
            return;
        }

        let n = st.cells.len();
        let op = if st.operator.is_empty() { st.plmn.clone() } else { st.operator.clone() };
        self.caption(ui, &mut y, &format!("{op} · {n} {}", self.t(if n == 1 { "cell" } else { "cells" })));

        // Radar
        let target = radio::target(&p, &st);
        let rr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(196.0));
        ui.cv.rrect(rr, ui.dp(20.0), blue.alpha(0.25));
        ui.cv.rrect(rr.inset(1.0), ui.dp(19.0), ui.th.bg.mix(blue, 0.05));
        let (cx, cy) = (rr.cx(), rr.cy());
        let big = (rr.h / 2.0 - ui.dp(14.0)).min(rr.w / 2.0 - ui.dp(14.0));
        for (k, a) in [(0.33, 0.28), (0.66, 0.2), (1.0, 0.14)] {
            ui.cv.ring(cx, cy, big * k, 1.0, blue.alpha(a));
        }
        let has_dist = st.cells.iter().any(|c| c.dist_m.is_some());
        let max_d = st.cells.iter().filter_map(|c| c.dist_m).max().unwrap_or(3000).max(1000) as f32;
        let pos: Vec<(u32, f32, f32)> = st
            .cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let ang = cell.bearing.map(|b| (b as f32).to_radians()).unwrap_or(i as f32 * std::f32::consts::TAU / n.max(1) as f32);
                let dist = match cell.dist_m {
                    Some(d) => (d as f32 / max_d).clamp(0.18, 0.95),
                    None => (1.0 - radio::level(cell.rsrp) as f32 / 5.0).clamp(0.25, 0.95),
                };
                (cell.id, cx + ang.sin() * big * dist, cy - ang.cos() * big * dist)
            })
            .collect();
        if let Some(sid) = st.serving {
            if let Some((_, x, yy)) = pos.iter().find(|p| p.0 == sid) {
                ui.cv.line(cx, cy, *x, *yy, ui.dp(1.5), blue.alpha(0.55));
            }
        }
        if let Some(t) = target.filter(|t| Some(*t) != st.serving) {
            if let Some((_, x, yy)) = pos.iter().find(|p| p.0 == t) {
                ui.cv.line(cx, cy, *x, *yy, ui.dp(1.2), gold.alpha(0.45));
            }
        }
        for (id, x, yy) in &pos {
            let is_t = Some(*id) == target;
            let col = if is_t { gold } else { blue };
            ui.cv.circle(*x, *yy, ui.dp(if is_t { 10.0 } else { 8.0 }), col.alpha(0.18));
            ui.cv.circle(*x, *yy, ui.dp(if is_t { 6.0 } else { 4.5 }), col);
            let lab = format!("{id}");
            ui.label(Rect::new(x - ui.dp(16.0), yy + ui.dp(8.0), ui.dp(32.0), ui.dp(14.0)), &lab, 10.0, if is_t { gold } else { rgb(BLUE_L) }, true, Al::Center);
        }
        ui.cv.circle(cx, cy, ui.dp(9.0), c.alpha(0.12));
        ui.cv.circle(cx, cy, ui.dp(5.0), c);
        if has_dist {
            let km = |f: f32| fmt_km((max_d * f / 100.0).round() as u32 * 100);
            let leg = format!("{} · {} · {}", km(0.33), km(0.66), km(1.0));
            ui.label(Rect::new(rr.x + ui.dp(12.0), rr.bottom() - ui.dp(22.0), rr.w - ui.dp(24.0), ui.dp(16.0)), &leg, 10.5, cs, false, Al::Start);
        }
        y += rr.h + ui.dp(12.0);

        // Modus
        let opts = [self.t("Auto"), self.t("Smart"), self.t("Manual")];
        if let Some(i) = ui.chips(&mut y, &opts, p.mode.index()) {
            p.mode = Mode::from_index(i);
            if p.mode == Mode::Manual && p.pinned.is_none() {
                p.pinned = st.serving;
            }
            self.save_rprefs(p.clone());
        }
        let target = radio::target(&p, &st);
        let cur_m = st.serving_cell().and_then(radio::est_mbps).unwrap_or(0.0);
        let hint = match p.mode {
            Mode::Auto => {
                let sid = radio::strongest(&st.cells).unwrap_or(0);
                format!("{} {sid}{}", self.t("Auto: the modem takes the strongest cell – like Android. Here: cell"), ".")
            }
            Mode::Smart => match target.and_then(|t| st.cell(t)) {
                Some(tc) if Some(tc.id) != st.serving && cur_m > 0.0 => {
                    let ratio = radio::est_mbps(tc).unwrap_or(0.0) / cur_m;
                    format!("{} {}: {} {}× {}", self.t("Smart: cell"), tc.id, self.t("weaker signal, but more free capacity – about"), format!("{:.1}", ratio).replace('.', ","), self.t("faster than now."))
                }
                _ => self.t("Smart: you are already on the fastest cell.").to_string(),
            },
            Mode::Manual => self.t("Manual: tap a cell to pin it. If it is lost, Ultar falls back to Smart.").to_string(),
        };
        let hh = ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &hint, 12.5, cs.mix(c, 0.25));
        y += hh + ui.dp(8.0);
        if p.mode == Mode::Manual && !st.pin_supported {
            let hh = ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("Note: this modem can only prefer a cell, not force it."), 12.0, rgb(WARN));
            y += hh + ui.dp(8.0);
        }
        if !st.neighbours {
            let hh = ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("The modem reports only the connected cell so far."), 12.0, cs);
            y += hh + ui.dp(8.0);
        }

        // Zellenliste
        let rh = ui.dp(70.0);
        for cell in &st.cells {
            let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
            y += rh + ui.dp(8.0);
            if !ui.visible(r) {
                continue;
            }
            let is_t = Some(cell.id) == target;
            let conn = Some(cell.id) == st.serving;
            let held = ui.held(r);
            let rad = ui.dp(16.0);
            ui.cv.rrect(r, rad, if is_t { ui.th.accent } else { ui.th.line });
            let fill = if is_t { ui.th.card.mix(ui.th.accent, 0.1) } else { ui.th.card };
            ui.cv.rrect(r.inset(if is_t { 2.0 } else { 1.0 }), rad, if held { fill.mix(rgb(0), 0.2) } else { fill });
            let inr = r.inset(ui.dp(12.0));
            // Balken (immer am Anfang)
            let bx = if ui.rtl { inr.right() - ui.dp(24.0) } else { inr.x };
            bars(ui, bx, inr.cy() + ui.dp(9.0), ui.dp(18.0), radio::level(cell.rsrp), blue, ui.th.line);
            // Tempo am Ende
            let sp = radio::est_mbps(cell);
            let spw = ui.dp(70.0);
            let spr = if ui.rtl { Rect::new(inr.x, inr.y, spw, inr.h) } else { Rect::new(inr.right() - spw, inr.y, spw, inr.h) };
            let sps = sp.map(|v| if cell.mbps.is_some() { fmt_mbps(v) } else { format!("≈{}", fmt_mbps(v)) }).unwrap_or_else(|| "—".into());
            ui.label(Rect::new(spr.x, spr.y + ui.dp(4.0), spr.w, ui.dp(24.0)), &sps, 18.0, c, true, Al::End);
            ui.label(Rect::new(spr.x, spr.y + ui.dp(28.0), spr.w, ui.dp(14.0)), "Mbit/s", 10.0, cs, false, Al::End);
            // Mitte
            let mx = ui.dp(40.0);
            let mid = Rect::new(if ui.rtl { inr.x + spw } else { inr.x + mx }, inr.y, inr.w - mx - spw - ui.dp(6.0), inr.h);
            let title = format!("{} {}", self.t("Cell"), cell.id);
            let tw = ui.text_w(&title, 14.0, true);
            ui.label(Rect::new(mid.x, mid.y, mid.w, ui.dp(18.0)), &title, 14.0, c, true, Al::Start);
            let badge = if conn {
                Some((self.t("CONNECTED"), blue.alpha(0.22), rgb(BLUE_L)))
            } else if is_t {
                Some((if p.mode == Mode::Manual { self.t("PINNED") } else { self.t("TARGET") }, ui.th.accent, ui.th.on_accent))
            } else {
                None
            };
            if let Some((bt, bg, fg)) = badge {
                let bw = ui.text_w(bt, 9.0, true) + ui.dp(10.0);
                let bxr = if ui.rtl { mid.right() - tw - ui.dp(6.0) - bw } else { mid.x + tw + ui.dp(6.0) };
                let br = Rect::new(bxr, mid.y + ui.dp(2.0), bw, ui.dp(15.0));
                ui.cv.rrect(br, ui.dp(5.0), bg);
                ui.label(br, bt, 9.0, fg, true, Al::Center);
            }
            let mut meta = Vec::new();
            if !cell.band.is_empty() {
                meta.push(if cell.mhz > 0 { format!("{} · {} MHz", cell.band, cell.mhz) } else { cell.band.clone() });
            }
            if let Some(d) = cell.dist_m {
                meta.push(fmt_km(d));
            }
            if let Some(v) = cell.rsrp {
                meta.push(format!("{v} dBm"));
            }
            let ms = ui.fit(&meta.join(" · "), 11.5, false, mid.w);
            ui.label(Rect::new(mid.x, mid.y + ui.dp(20.0), mid.w, ui.dp(16.0)), &ms, 11.5, cs, false, Al::Start);
            if let Some(l) = cell.load {
                let lr = Rect::new(mid.x, mid.y + ui.dp(40.0), mid.w, ui.dp(4.0));
                ui.cv.rrect(lr, ui.dp(2.0), ui.th.line);
                let fw = lr.w * (l.min(100) as f32 / 100.0);
                let fr = if ui.rtl { Rect::new(lr.right() - fw, lr.y, fw, lr.h) } else { Rect::new(lr.x, lr.y, fw, lr.h) };
                ui.cv.rrect(fr, ui.dp(2.0), if l >= 70 { rgb(WARN) } else { rgb(GOOD) });
            }
            if ui.tap(r) {
                p.mode = Mode::Manual;
                p.pinned = Some(cell.id);
                self.save_rprefs(p.clone());
                ui.dirty = true;
            }
        }
        ui.scroll_end(list, y + *sc - list.y, sc);

        // Fußbereich: Hauptknopf + Legende
        let fr = Rect::new(m, list.bottom() + ui.dp(8.0), ui.w - 2.0 * m, btn_h);
        let target = radio::target(&p, &st);
        match target {
            Some(t) if Some(t) != st.serving => {
                let l = format!("{} {t} {}", self.t("Switch to cell"), self.t("now"));
                if ui.button(fr, &l, Btn::Primary) {
                    match radio::send_cmd(&format!("select {t}")) {
                        Ok(()) => self.toast(self.t("Cell switch requested").into()),
                        Err(e) => self.toast(format!("{}: {e}", self.t("Error"))),
                    }
                    ui.dirty = true;
                }
            }
            Some(t) => {
                let l = format!("{} {t}", self.t("Connected to cell"));
                ui.button(fr, &l, Btn::Plain);
            }
            None => {}
        }
        let leg = self.t("Bars = signal · line = load · number = speed");
        ui.label(Rect::new(m, fr.bottom() + ui.dp(4.0), ui.w - 2.0 * m, ui.dp(18.0)), leg, 10.5, cs, false, Al::Center);
    }

    // ------------------------------------------------------------ Verbindungsweg

    pub(super) fn s_route(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Connection path"));
        let st = self.radio();
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let blue = rgb(BLUE);
        let gold = ui.th.accent;
        let wifi = quick::wifi_on() && crate::sys::online();
        let vpn = crate::sys::vpn_status();
        let mut hops: Vec<(Ic, String, String, Color, bool)> = Vec::new();
        hops.push((
            Ic::Chip,
            self.t("This device").into(),
            if self.set.priv_all { "Ultar OS".into() } else { self.t("All radios off (privacy)").into() },
            c,
            self.set.priv_all,
        ));
        if wifi {
            hops.push((Ic::Globe, self.t("Wi-Fi").into(), self.t("Local network").into(), blue, true));
        } else if let Some(cell) = st.serving_cell().filter(|_| st.in_service()) {
            let mut sub = Vec::new();
            if let Some(d) = cell.dist_m {
                sub.push(fmt_km(d));
            }
            if let Some(v) = cell.rsrp {
                sub.push(format!("{v} dBm"));
            }
            sub.push(self.t("radio link").into());
            let band = if cell.band.is_empty() { String::new() } else { format!(" · {}", cell.band) };
            hops.push((Ic::Tower, format!("{} {}{band}", self.t("Cell"), cell.id), sub.join(" · "), blue, true));
            let op = if st.operator.is_empty() { st.plmn.clone() } else { st.operator.clone() };
            hops.push((Ic::Globe, format!("{op} · {}", self.t("core network")), format!("{} · {}", st.rat, if st.roaming { self.t("Roaming") } else { self.t("Home network") }), blue, true));
        } else {
            hops.push((Ic::Tower, self.t("No connection").into(), self.t("Neither Wi-Fi nor mobile network").into(), rgb(WARN), false));
        }
        if vpn.up {
            let ep = if vpn.endpoint.is_empty() { "ultar.code-craft.tech".to_string() } else { vpn.endpoint.clone() };
            hops.push((Ic::Shield, "Ultar VPN".into(), format!("{ep} · WireGuard"), gold, true));
        } else {
            hops.push((Ic::Shield, self.t("VPN off").into(), self.t("Traffic goes directly to the internet").into(), rgb(WARN), false));
        }
        hops.push((
            Ic::Check,
            self.t("Internet").into(),
            if self.set.adblock { self.t("Ad and tracker blocking active").into() } else { self.t("No ad blocking").into() },
            rgb(GOOD),
            true,
        ));
        let m = ui.dp(22.0);
        let ib = ui.dp(42.0);
        let n = hops.len();
        for (i, (ic, title, sub, col, ok)) in hops.into_iter().enumerate() {
            let ir = mir(ui, Rect::new(m, y, ib, ib));
            ui.cv.rrect(ir, ui.dp(13.0), col.alpha(0.16));
            ui.icon(ic, ir.cx(), ir.cy(), ui.dp(20.0), col.mix(rgb(0xFFFFFF), 0.2));
            let tx = Rect::new(if ui.rtl { m } else { m + ib + ui.dp(12.0) }, y, ui.w - 2.0 * m - ib - ui.dp(12.0), ui.dp(20.0));
            ui.label(tx, &title, 15.0, c, true, Al::Start);
            let sf = ui.fit(&sub, 12.0, false, tx.w);
            ui.label(Rect::new(tx.x, y + ui.dp(21.0), tx.w, ui.dp(18.0)), &sf, 12.0, if ok { cs } else { rgb(WARN) }, false, Al::Start);
            y += ib;
            if i + 1 < n {
                let lx = ir.cx() - ui.dp(1.0);
                ui.cv.rrect(Rect::new(lx, y + ui.dp(4.0), ui.dp(2.0), ui.dp(18.0)), ui.dp(1.0), if i >= 1 && vpn.up { gold.alpha(0.5) } else { blue.alpha(0.45) });
                y += ui.dp(26.0);
            }
        }
        y += ui.dp(18.0);
        if ui.row(&mut y, Some(Ic::Chip), self.t("Data traffic"), Some(self.t("Upload and download per connection")), None, true) {
            self.mon_tab = 1;
            self.go(Screen::Monitor);
            ui.dirty = true;
        }
        if !vpn.up && ui.row(&mut y, Some(Ic::Shield), self.t("Turn on VPN"), Some(self.t("All traffic through your Ultar server")), None, true) {
            self.go(Screen::Vpn);
            ui.dirty = true;
        }
        y += ui.dp(8.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}
