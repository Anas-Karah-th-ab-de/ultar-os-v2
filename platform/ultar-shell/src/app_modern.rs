//! Modernisierte Kern-Apps im Ultar-Stil (Gold = Aktion/Auswahl, Blau = Netz, Rot = Auflegen/Löschen):
//! - Anrufbildschirm (Vollbild): Avatar, Name/Nummer, Zeit, Verbindungsweg, Steuerknöpfe, runde Annehmen/Auflegen
//! - Anrufliste mit Avataren, Richtung, Rückruf
//! - Gemeinsame Bausteine: Avatar mit Initialen, Kontaktname zur Nummer, Datei-Symbole, Zeitangaben

use super::app_phone::CallState;
use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Ic, Ui};
use std::time::Instant;

pub const RED: u32 = 0xF0566A;
pub const GREEN: u32 = 0x34C759;
const AVATAR_HUES: [u32; 8] = [
    0xC9A227, 0x5FB3D9, 0x7ED6A0, 0xF08A4B, 0xB59CF2, 0xF0566A, 0x4FD1C5, 0xE6C65C,
];

/// Runder Avatar mit Initialen; Farbe stabil aus dem Namen/der Nummer abgeleitet.
pub fn avatar(ui: &mut Ui, cx: f32, cy: f32, r: f32, name: &str) {
    let h = name
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let hue = rgb(AVATAR_HUES[(h as usize) % AVATAR_HUES.len()]);
    ui.cv.circle(cx, cy, r, hue.alpha(0.22));
    ui.cv.ring(cx, cy, r - 0.5, 1.0, hue.alpha(0.45));
    let ini = initials(name);
    let fs = r / ui.k * 0.78;
    let c = hue.mix(rgb(0xFFFFFF), 0.35);
    if ini.is_empty() {
        ui.icon(Ic::Person, cx, cy, r * 1.0, c);
    } else {
        ui.label(
            Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r),
            &ini,
            fs,
            c,
            true,
            Al::Center,
        );
    }
}

/// Bis zu zwei Initialen aus einem Namen; Nummern ergeben keine Initialen.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| w.chars().next().is_some_and(|c| c.is_alphabetic()))
        .collect();
    words
        .iter()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(|c| c.to_uppercase())
        .collect()
}

/// Nummern vergleichen: nur Ziffern, die letzten 8 müssen gleich sein (+49 170… == 0170…).
pub fn same_number(a: &str, b: &str) -> bool {
    let d = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    let (a, b) = (d(a), d(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = a.len().min(b.len()).min(8);
    a[a.len() - n..] == b[b.len() - n..]
}

/// Symbol und Farbe je Dateityp.
pub fn file_kind(name: &str, is_dir: bool) -> (Ic, u32) {
    if is_dir {
        return (Ic::Folder, 0xC9A227);
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => (Ic::Image, 0x5FB3D9),
        "mp4" | "avi" | "mkv" | "mov" | "webm" | "mjpeg" => (Ic::Video, 0xB59CF2),
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => (Ic::Music, 0xF08A4B),
        "txt" | "md" | "log" | "toml" | "json" | "conf" | "csv" | "rs" | "sh" | "xml" | "html" => {
            (Ic::Note, 0x7ED6A0)
        }
        "zip" | "gz" | "tgz" | "xz" | "zst" | "tar" | "deb" | "uap" => (Ic::Box, 0xE6C65C),
        _ => (Ic::Note, 0x8A93A3),
    }
}

/// „gerade eben“, „vor 5 min“, „vor 3 h“, „gestern“, sonst Datum.
pub fn age(app: &App, secs_ago: i64, unix: i64) -> String {
    match secs_ago {
        s if s < 60 => app.t_pub("just now").to_string(),
        s if s < 3600 => format!("{} {} min", app.t_pub("ago"), s / 60),
        s if s < 86_400 => format!("{} {} h", app.t_pub("ago"), s / 3600),
        s if s < 2 * 86_400 => app.t_pub("yesterday").to_string(),
        _ => {
            let dt = crate::sys::from_unix(unix, app.set.tz_min);
            format!("{:02}.{:02}.{}", dt.d, dt.mo, dt.y)
        }
    }
}

impl App {
    pub(super) fn t_pub(&self, k: &'static str) -> &'static str {
        self.t(k)
    }

    /// Kontaktname zu einer Nummer (falls gespeichert).
    pub(super) fn contact_name(&mut self, number: &str) -> Option<String> {
        self.extra_load();
        self.extra
            .contacts
            .list
            .iter()
            .find(|c| same_number(&c.number, number))
            .map(|c| c.name.clone())
    }

    // ------------------------------------------------------------ Anrufbildschirm

    /// Vollbild-Anrufbildschirm (wählt / im Gespräch / eingehend).
    pub(super) fn call_screen(&mut self, ui: &mut Ui, area: Rect, state: CallState) {
        let (num, status) = match &state {
            CallState::Dialing(n) => (n.clone(), self.t("Dialing…").to_string()),
            CallState::Active(n) => {
                let since = *self.phone.since.get_or_insert_with(Instant::now);
                let s = since.elapsed().as_secs();
                (n.clone(), format!("{:02}:{:02}", s / 60, s % 60))
            }
            CallState::Incoming(n) => (n.clone(), self.t("Incoming call").to_string()),
            CallState::Idle => return,
        };
        if !matches!(state, CallState::Active(_)) {
            self.phone.since = None;
        }
        let name = self.contact_name(&num);
        let (c, cs) = (ui.th.text, ui.th.sub);
        // Hintergrund: leichter Farbverlauf von oben (Gold bei eingehend, sonst Blau)
        let tint = if matches!(state, CallState::Incoming(_)) {
            ui.th.accent
        } else {
            rgb(0x5FB3D9)
        };
        ui.cv.vgrad(
            Rect::new(0.0, area.y, ui.w, area.h * 0.55),
            ui.th.bg.mix(tint, 0.14),
            ui.th.bg,
        );

        // Verbindungsweg (SIM/Netz oder Ultar-Nummer über Internet)
        let via = if ultar_cloud::my_number().is_some() && !self.radio().in_service() {
            format!("Ultar · {}", self.t("Internet call"))
        } else {
            let st = self.radio();
            if st.in_service() {
                format!("SIM 1 · {}", super::app_radio::rat_short(&st.rat))
            } else {
                self.t("Mobile network").to_string()
            }
        };
        let vw = ui.text_w(&via, 12.0, true) + ui.dp(24.0);
        let vr = Rect::new((ui.w - vw) / 2.0, area.y + ui.dp(28.0), vw, ui.dp(26.0));
        ui.cv.rrect(vr, ui.dp(13.0), ui.th.text.alpha(0.07));
        ui.label(vr, &via, 12.0, cs.mix(c, 0.3), true, Al::Center);

        // Avatar, Name, Nummer, Status
        let ay = area.y + (area.h * 0.24).max(ui.dp(140.0));
        let ar = ui.dp(54.0);
        // Pulsierender Ring bei eingehend/wählend
        if !matches!(state, CallState::Active(_)) {
            let t = (Instant::now().elapsed().as_millis() as f32 / 1000.0).fract();
            let _ = t;
            ui.cv.ring(
                ui.w / 2.0,
                ay,
                ar + ui.dp(14.0),
                ui.dp(2.0),
                tint.alpha(0.25),
            );
            ui.cv.ring(
                ui.w / 2.0,
                ay,
                ar + ui.dp(28.0),
                ui.dp(1.5),
                tint.alpha(0.12),
            );
        }
        avatar(ui, ui.w / 2.0, ay, ar, name.as_deref().unwrap_or(""));
        let title = name.clone().unwrap_or_else(|| num.clone());
        let tf = ui.fit(&title, 28.0, true, ui.w - ui.dp(40.0));
        let old = ui.rtl;
        ui.rtl = false;
        ui.label(
            Rect::new(0.0, ay + ar + ui.dp(18.0), ui.w, ui.dp(38.0)),
            &tf,
            28.0,
            c,
            true,
            Al::Center,
        );
        if name.is_some() {
            ui.label(
                Rect::new(0.0, ay + ar + ui.dp(56.0), ui.w, ui.dp(22.0)),
                &num,
                15.0,
                cs,
                false,
                Al::Center,
            );
        }
        ui.rtl = old;
        let sc = if matches!(state, CallState::Active(_)) {
            rgb(GREEN)
        } else {
            cs
        };
        ui.label(
            Rect::new(
                0.0,
                ay + ar + ui.dp(if name.is_some() { 82.0 } else { 60.0 }),
                ui.w,
                ui.dp(22.0),
            ),
            &status,
            15.0,
            sc,
            matches!(state, CallState::Active(_)),
            Al::Center,
        );

        let bottom = area.bottom() - ui.dp(36.0);
        let big = ui.dp(36.0);
        if let CallState::Incoming(_) = state {
            // Ablehnen (rot) | Annehmen (grün)
            let y = bottom - big;
            let (lx, rx) = (ui.w * 0.27, ui.w * 0.73);
            let (dx, ax) = if ui.rtl { (rx, lx) } else { (lx, rx) };
            if ui.icon_btn(dx, y, big, Ic::Phone, rgb(RED), rgb(0xFFFFFF)) {
                self.phone.tel.hangup();
                ui.dirty = true;
            }
            if ui.icon_btn(ax, y, big, Ic::Phone, rgb(GREEN), rgb(0xFFFFFF)) {
                self.phone.tel.answer();
                ui.dirty = true;
            }
            ui.label(
                Rect::new(
                    dx - ui.dp(60.0),
                    y + big + ui.dp(6.0),
                    ui.dp(120.0),
                    ui.dp(18.0),
                ),
                self.t("Decline"),
                13.0,
                cs,
                false,
                Al::Center,
            );
            ui.label(
                Rect::new(
                    ax - ui.dp(60.0),
                    y + big + ui.dp(6.0),
                    ui.dp(120.0),
                    ui.dp(18.0),
                ),
                self.t("Answer"),
                13.0,
                cs,
                false,
                Al::Center,
            );
            // Mit Nachricht antworten
            let mr = Rect::new(
                (ui.w - ui.dp(220.0)) / 2.0,
                y - big - ui.dp(64.0),
                ui.dp(220.0),
                ui.dp(42.0),
            );
            let held = ui.held(mr);
            ui.cv.rrect(
                mr,
                ui.dp(21.0),
                ui.th.text.alpha(if held { 0.14 } else { 0.07 }),
            );
            ui.icon(Ic::Chat, mr.x + ui.dp(26.0), mr.cy(), ui.dp(16.0), c);
            ui.label(
                Rect::new(mr.x + ui.dp(42.0), mr.y, mr.w - ui.dp(52.0), mr.h),
                self.t("Reply with message"),
                13.0,
                c,
                false,
                Al::Start,
            );
            if ui.tap(mr) {
                self.phone.tel.hangup();
                self.open_sms_to(&num);
                ui.dirty = true;
            }
            ui.dirty = true; // Ring pulsiert
            return;
        }

        // Steuerung: Stumm, Lautsprecher, Tastatur, Nachricht
        let ctl = self.phone.tel.supports_controls();
        let items: [(Ic, &str, bool, bool); 4] = [
            (Ic::Speaker, self.t("Mute"), self.phone.mute, ctl),
            (Ic::Speaker, self.t("Speaker"), self.phone.speaker, ctl),
            (Ic::Grid, self.t("Keypad"), self.phone.keypad, ctl),
            (Ic::Chat, self.t("Message"), false, true),
        ];
        let gy = bottom - big * 2.0 - ui.dp(150.0);
        if self.phone.keypad && ctl {
            // DTMF-Tastenfeld über den Steuerknöpfen
            let keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];
            let kw = ui.dp(64.0);
            let kx0 = (ui.w - 3.0 * kw - 2.0 * ui.dp(16.0)) / 2.0;
            let ky0 = gy - ui.dp(4.0) * 0.0 - 4.0 * (kw * 0.78 + ui.dp(8.0)) - ui.dp(10.0);
            let d = self.phone.dtmf.clone();
            let old = ui.rtl;
            ui.rtl = false;
            ui.label(
                Rect::new(0.0, ky0 - ui.dp(36.0), ui.w, ui.dp(30.0)),
                &d,
                24.0,
                c,
                true,
                Al::Center,
            );
            ui.rtl = old;
            for (i, k) in keys.iter().enumerate() {
                let r = Rect::new(
                    kx0 + (i % 3) as f32 * (kw + ui.dp(16.0)),
                    ky0 + (i / 3) as f32 * (kw * 0.78 + ui.dp(8.0)),
                    kw,
                    kw * 0.78,
                );
                let held = ui.held(r);
                ui.cv.rrect(
                    r,
                    r.h / 2.0,
                    ui.th.text.alpha(if held { 0.16 } else { 0.07 }),
                );
                ui.label(r, k, 22.0, c, false, Al::Center);
                if ui.tap(r) {
                    let ch = k.chars().next().unwrap_or('0');
                    self.phone.tel.dtmf(ch);
                    self.phone.dtmf.push(ch);
                    ui.dirty = true;
                }
            }
        }
        let cw = (ui.w - ui.dp(32.0)) / 4.0;
        for (i, (ic, lab, on, en)) in items.into_iter().enumerate() {
            let col = if ui.rtl { 3 - i } else { i };
            let cx = ui.dp(16.0) + cw * (col as f32 + 0.5);
            let r = ui.dp(28.0);
            let bg = if on {
                ui.th.accent
            } else {
                ui.th.text.alpha(if en { 0.08 } else { 0.03 })
            };
            let fg = if on {
                ui.th.on_accent
            } else if en {
                c
            } else {
                cs.alpha(0.5)
            };
            let old_en = ui.enabled;
            ui.enabled = old_en && en;
            let tapped = ui.icon_btn(cx, gy, r, ic, bg, fg);
            ui.enabled = old_en;
            if i == 0 && !on {
                // Stumm: Mikrofon durchgestrichen andeuten
                ui.cv.line(
                    cx - r * 0.45,
                    gy + r * 0.45,
                    cx + r * 0.45,
                    gy - r * 0.45,
                    ui.dp(2.0),
                    fg,
                );
            }
            ui.label(
                Rect::new(cx - cw / 2.0, gy + r + ui.dp(6.0), cw, ui.dp(18.0)),
                lab,
                12.0,
                if en { cs.mix(c, 0.3) } else { cs.alpha(0.6) },
                false,
                Al::Center,
            );
            if tapped {
                match i {
                    0 => {
                        self.phone.mute = !self.phone.mute;
                        let m = self.phone.mute;
                        self.phone.tel.set_mute(m);
                    }
                    1 => {
                        self.phone.speaker = !self.phone.speaker;
                        let s = self.phone.speaker;
                        self.phone.tel.set_speaker(s);
                    }
                    2 => self.phone.keypad = !self.phone.keypad,
                    _ => self.open_sms_to(&num),
                }
                ui.dirty = true;
            }
        }
        if !ctl {
            ui.label(
                Rect::new(
                    ui.dp(20.0),
                    gy + ui.dp(56.0),
                    ui.w - ui.dp(40.0),
                    ui.dp(18.0),
                ),
                self.t("Call controls follow with the modem audio path"),
                11.0,
                cs.alpha(0.8),
                false,
                Al::Center,
            );
        }
        // Auflegen
        if ui.icon_btn(
            ui.w / 2.0,
            bottom - big,
            big,
            Ic::Phone,
            rgb(RED),
            rgb(0xFFFFFF),
        ) {
            self.phone.tel.hangup();
            self.phone.since = None;
            self.phone.mute = false;
            self.phone.speaker = false;
            self.phone.keypad = false;
            self.phone.dtmf.clear();
            ui.dirty = true;
        }
        if matches!(state, CallState::Active(_)) || matches!(state, CallState::Dialing(_)) {
            ui.dirty = true; // Gesprächszeit läuft
        }
    }

    /// Nachrichten-App mit vorbelegter Nummer öffnen (bestehende Unterhaltung oder neue).
    pub(super) fn open_sms_to(&mut self, num: &str) {
        let threads = self.msg.sms.threads();
        if let Some(i) = threads.iter().position(|t| same_number(&t.number, num)) {
            self.msg.open = Some(i);
            self.go(Screen::Messages);
        } else {
            self.fields.insert("sms_to", num.to_string());
            self.go(Screen::MessageNew);
        }
    }

    // ------------------------------------------------------------ Anruf in Statusleiste / Schnelleinstellungen

    fn call_text(&mut self) -> Option<(String, String, bool)> {
        let st = self.phone.tel.state();
        let (num, status, live) = match st {
            CallState::Dialing(n) => (n, self.t("Dialing…").to_string(), false),
            CallState::Incoming(n) => (n, self.t("Incoming call").to_string(), false),
            CallState::Active(n) => {
                let s = self.phone.since.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                (n, format!("{:02}:{:02}", s / 60, s % 60), true)
            }
            CallState::Idle => return None,
        };
        let who = self.contact_name(&num).unwrap_or(num);
        Some((who, status, live))
    }

    /// Grüne Anruf-Pille in der Statusleiste (statt der Netz-Pille, solange ein Anruf läuft).
    pub(super) fn call_pill(&mut self, ui: &mut Ui, x: f32, h: f32) -> f32 {
        let Some((_, status, _)) = self.call_text() else {
            return 0.0;
        };
        let ph = (h - ui.dp(8.0)).min(ui.dp(22.0));
        let tw = ui.text_w(&status, 11.0, true);
        let w = ui.dp(8.0) + ui.dp(12.0) + ui.dp(6.0) + tw + ui.dp(10.0);
        let r = Rect::new(if ui.rtl { x - w } else { x }, (h - ph) / 2.0, w, ph);
        ui.cv.rrect(r, ph / 2.0, rgb(GREEN));
        ui.icon(
            Ic::Phone,
            r.x + ui.dp(14.0),
            r.cy(),
            ui.dp(12.0),
            rgb(0x06210F),
        );
        let old = ui.rtl;
        ui.rtl = false;
        ui.label(
            Rect::new(r.x + ui.dp(26.0), r.y, tw + 2.0, r.h),
            &status,
            11.0,
            rgb(0x06210F),
            true,
            Al::Start,
        );
        ui.rtl = old;
        if !self.shade
            && ui.tap(r)
            && !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Phone))
        {
            self.go(Screen::Phone);
            ui.dirty = true;
        }
        w
    }

    /// Karte „Laufender Anruf“ in den Schnelleinstellungen; liefert die belegte Höhe (0 ohne Anruf).
    pub(super) fn shade_call(&mut self, ui: &mut Ui, x: f32, y: f32, w: f32) -> f32 {
        let Some((who, status, live)) = self.call_text() else {
            return 0.0;
        };
        let incoming = matches!(self.phone.tel.state(), CallState::Incoming(_));
        let h = ui.dp(72.0);
        let r = Rect::new(x, y, w, h);
        let g = rgb(GREEN);
        ui.cv.rrect(r, ui.dp(20.0), g.alpha(0.45));
        ui.cv
            .rrect(r.inset(1.0), ui.dp(20.0), ui.th.card.mix(g, 0.12));
        let ax = if ui.rtl {
            r.right() - ui.dp(36.0)
        } else {
            r.x + ui.dp(36.0)
        };
        avatar(ui, ax, r.cy(), ui.dp(22.0), &who);
        let bw = if incoming { ui.dp(104.0) } else { ui.dp(56.0) };
        let tx = Rect::new(
            if ui.rtl { r.x + bw } else { r.x + ui.dp(70.0) },
            r.y,
            r.w - ui.dp(70.0) - bw,
            r.h,
        );
        let c = ui.th.text;
        let tf = ui.fit(&who, 15.0, true, tx.w);
        ui.label(
            Rect::new(tx.x, r.y + ui.dp(14.0), tx.w, ui.dp(22.0)),
            &tf,
            15.0,
            c,
            true,
            Al::Start,
        );
        let sub = if live {
            format!("{} · {status}", self.t("In call"))
        } else {
            status
        };
        ui.label(
            Rect::new(tx.x, r.y + ui.dp(38.0), tx.w, ui.dp(18.0)),
            &sub,
            12.5,
            g.mix(rgb(0xFFFFFF), 0.25),
            live,
            Al::Start,
        );
        let mut acted = false;
        let ex = if ui.rtl {
            r.x + ui.dp(30.0)
        } else {
            r.right() - ui.dp(30.0)
        };
        if ui.icon_btn(ex, r.cy(), ui.dp(20.0), Ic::Phone, rgb(RED), rgb(0xFFFFFF)) {
            self.phone.tel.hangup();
            acted = true;
        }
        if incoming {
            let ax2 = if ui.rtl {
                ex + ui.dp(48.0)
            } else {
                ex - ui.dp(48.0)
            };
            if ui.icon_btn(ax2, r.cy(), ui.dp(20.0), Ic::Phone, g, rgb(0xFFFFFF)) {
                self.phone.tel.answer();
                self.shade = false;
                self.go(Screen::Phone);
                acted = true;
            }
        }
        if !acted && ui.tap(r) {
            self.shade = false;
            self.go(Screen::Phone);
            acted = true;
        }
        if acted {
            ui.dirty = true;
        }
        h + ui.dp(10.0)
    }

    // ------------------------------------------------------------ Anrufliste

    pub(super) fn phone_log_modern(&mut self, ui: &mut Ui, body: Rect) {
        let log = self.phone.tel.log();
        let (c, cs) = (ui.th.text, ui.th.sub);
        if log.is_empty() {
            ui.icon(
                Ic::Phone,
                body.cx(),
                body.y + ui.dp(70.0),
                ui.dp(40.0),
                cs.alpha(0.6),
            );
            ui.label(
                Rect::new(body.x, body.y + ui.dp(100.0), body.w, ui.dp(24.0)),
                self.t("No recent calls"),
                15.0,
                c,
                true,
                Al::Center,
            );
            return;
        }
        let mut y = body.y + ui.dp(4.0);
        let rh = ui.dp(68.0);
        for e in log.iter().take(60) {
            let r = Rect::new(body.x, y, body.w, rh);
            y += rh + ui.dp(6.0);
            if !ui.visible(r) {
                continue;
            }
            let name = self.contact_name(&e.number);
            let held = ui.held(r);
            ui.card_h(r, held);
            let ax = if ui.rtl {
                r.right() - ui.dp(36.0)
            } else {
                r.x + ui.dp(36.0)
            };
            avatar(ui, ax, r.cy(), ui.dp(22.0), name.as_deref().unwrap_or(""));
            let tx = Rect::new(
                if ui.rtl {
                    r.x + ui.dp(70.0)
                } else {
                    r.x + ui.dp(70.0)
                },
                r.y,
                r.w - ui.dp(140.0),
                r.h,
            );
            let missed = e.dir == 2;
            let title = name.clone().unwrap_or_else(|| e.number.clone());
            let tf = ui.fit(&title, 16.0, true, tx.w);
            ui.label(
                Rect::new(tx.x, r.y + ui.dp(13.0), tx.w, ui.dp(22.0)),
                &tf,
                16.0,
                if missed { rgb(RED) } else { c },
                true,
                Al::Start,
            );
            let (dir, sub) = match e.dir {
                0 => ("↗", self.t("Outgoing")),
                1 => ("↙", self.t("Incoming")),
                _ => ("↙", self.t("Missed")),
            };
            let mut line = format!("{dir} {sub}");
            if name.is_some() {
                line += &format!(" · {}", e.number);
            }
            if !e.when.is_empty() {
                line += &format!(" · {}", e.when);
            }
            let lf = ui.fit(&line, 12.0, false, tx.w);
            ui.label(
                Rect::new(tx.x, r.y + ui.dp(37.0), tx.w, ui.dp(18.0)),
                &lf,
                12.0,
                if missed { rgb(RED).alpha(0.85) } else { cs },
                false,
                Al::Start,
            );
            // Rückruf (grün) am Ende
            let bx = if ui.rtl {
                r.x + ui.dp(34.0)
            } else {
                r.right() - ui.dp(34.0)
            };
            if ui.icon_btn(
                bx,
                r.cy(),
                ui.dp(20.0),
                Ic::Phone,
                rgb(GREEN).alpha(0.18),
                rgb(GREEN),
            ) {
                let n = e.number.clone();
                self.phone.number = n.clone();
                self.phone.tab = 0;
                self.phone.tel.dial(&n);
                ui.dirty = true;
            } else if ui.tap(r) {
                self.phone.number = e.number.clone();
                self.phone.tab = 0;
                ui.dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_match_across_formats() {
        assert!(same_number("+49 170 1234567", "01701234567"));
        assert!(!same_number("0170 1234567", "0170 7654321"));
        assert!(!same_number("", "123"));
    }

    #[test]
    fn initials_from_names() {
        assert_eq!(initials("Mohamad Anas"), "MA");
        assert_eq!(initials("محمد"), "م");
        assert_eq!(initials("+49 170"), "");
    }
}
