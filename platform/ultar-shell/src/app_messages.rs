//! Nachrichten-App (SMS): Konversationsliste, Verlauf, Eingabe.
//! Untermodul von `app`. Versand/Empfang über ein `SmsBackend`; bis der Modem-Stack steht,
//! ein lokaler Stub (speichert in /data/ultar/sms.json). Später an das echte Modem anklemmbar.

use super::{App, Screen};
use super::app_modern::{avatar, same_number, GREEN};
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Btn, Ic, Ui};
use serde::{Deserialize, Serialize};

/// Eine Nachricht. out: true = gesendet, false = empfangen.
#[derive(Clone, Serialize, Deserialize)]
pub struct Sms {
    pub out: bool,
    pub text: String,
    pub when: String,
}

/// Eine Konversation mit einer Nummer.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Thread {
    pub number: String,
    pub msgs: Vec<Sms>,
}

/// Schnittstelle zum SMS-Teil des Modems.
pub trait SmsBackend: Send {
    fn threads(&self) -> Vec<Thread>;
    fn send(&mut self, number: &str, text: &str);
    fn ready(&self) -> bool;
}

/// Lokaler Stub ohne Modem: hält Konversationen im Speicher (und optional auf Platte).
#[derive(Default)]
pub struct StubSms {
    threads: Vec<Thread>,
}

impl StubSms {
    fn load() -> Self {
        let p = format!("{}/sms.json", crate::sys::data_dir());
        let threads = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { threads }
    }
    fn save(&self) {
        let p = format!("{}/sms.json", crate::sys::data_dir());
        if let Ok(s) = serde_json::to_string(&self.threads) {
            let _ = std::fs::create_dir_all(crate::sys::data_dir());
            let _ = std::fs::write(p, s);
        }
    }
}

impl SmsBackend for StubSms {
    fn threads(&self) -> Vec<Thread> {
        self.threads.clone()
    }
    fn send(&mut self, number: &str, text: &str) {
        if number.is_empty() || text.is_empty() {
            return;
        }
        if let Some(t) = self.threads.iter_mut().find(|t| t.number == number) {
            t.msgs.push(Sms { out: true, text: text.to_string(), when: String::new() });
        } else {
            self.threads.insert(0, Thread { number: number.to_string(), msgs: vec![Sms { out: true, text: text.to_string(), when: String::new() }] });
        }
        self.save();
    }
    fn ready(&self) -> bool {
        false
    }
}

/// SMS über die SIM: Empfang aus dem Posteingang des Funkdienstes (/data/ultar/sms-in.tsv, Zeilen
/// `zeit<TAB>steckplatz<TAB>absender<TAB>text-hex`), Versand über den Befehl `sms 0 <nummer> <text-hex>`.
/// Konversationen werden wie beim Stub in /data/ultar/sms.json gehalten.
pub struct SimSms {
    store: std::sync::Mutex<(StubSms, u64)>,
}

const SMS_INBOX: &str = "/data/ultar/sms-in.tsv";

fn unhex(s: &str) -> String {
    let b: Vec<u8> = s.as_bytes().chunks(2).filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
    String::from_utf8_lossy(&b).into_owned()
}

impl SimSms {
    fn load() -> Self {
        let off = std::fs::read_to_string(format!("{}/sms-in.off", crate::sys::data_dir())).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        Self { store: std::sync::Mutex::new((StubSms::load(), off)) }
    }
    /// Neue Zeilen des Posteingangs übernehmen.
    fn ingest(&self) {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut g) = self.store.lock() else { return };
        let Ok(mut f) = std::fs::File::open(SMS_INBOX) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < g.1 {
            g.1 = 0; // Datei neu angelegt
        }
        if len == g.1 {
            return;
        }
        let mut buf = String::new();
        if f.seek(SeekFrom::Start(g.1)).is_err() || f.read_to_string(&mut buf).is_err() {
            return;
        }
        let complete = buf.rfind('\n').map(|i| i + 1).unwrap_or(0);
        for l in buf[..complete].lines() {
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() < 4 {
                continue;
            }
            let (from, text) = (p[2].to_string(), unhex(p[3]));
            let m = Sms { out: false, text, when: p[0].to_string() };
            if let Some(i) = g.0.threads.iter().position(|t| t.number == from) {
                let mut t = g.0.threads.remove(i);
                t.msgs.push(m);
                g.0.threads.insert(0, t);
            } else {
                g.0.threads.insert(0, Thread { number: from, msgs: vec![m] });
            }
        }
        g.1 += complete as u64;
        g.0.save();
        let _ = std::fs::write(format!("{}/sms-in.off", crate::sys::data_dir()), g.1.to_string());
    }
}

impl SmsBackend for SimSms {
    fn threads(&self) -> Vec<Thread> {
        self.ingest();
        self.store.lock().map(|g| g.0.threads.clone()).unwrap_or_default()
    }
    fn send(&mut self, number: &str, text: &str) {
        if number.is_empty() || text.is_empty() {
            return;
        }
        let hex: String = text.bytes().map(|b| format!("{b:02x}")).collect();
        let _ = ultar_radio::send_cmd(&format!("sms 0 {} {}", number.replace(' ', ""), hex));
        if let Ok(mut g) = self.store.lock() {
            g.0.send(number, text);
        }
    }
    fn ready(&self) -> bool {
        super::app_phone::sim_ready()
    }
}

/// Zustand der Nachrichten-App (Feld auf `App`).
pub struct MessagesState {
    /// None = Liste, Some(i) = geöffnete Konversation i
    pub open: Option<usize>,
    /// true im „Neue Nachricht"-Modus (Nummer eingeben)
    pub composing_new: bool,
    pub sms: Box<dyn SmsBackend>,
}

impl Default for MessagesState {
    fn default() -> Self {
        let sms: Box<dyn SmsBackend> = if std::path::Path::new("/dev/umts_ipc0").exists() || std::env::var_os("ULTAR_SIM_TEL").is_some() {
            Box::new(SimSms::load())
        } else {
            Box::new(StubSms::load())
        };
        Self { open: None, composing_new: false, sms }
    }
}

impl App {
    pub(super) fn s_messages(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let threads = self.msg.sms.threads();
        if let Some(i) = self.msg.open {
            if let Some(t) = threads.get(i).cloned() {
                self.messages_thread(ui, area, &t);
                return;
            }
            self.msg.open = None;
        }
        let area = self.header(ui, area, self.t("Messages"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(14.0);
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if !self.msg.sms.ready() {
            let txt = self.t("No mobile network yet (modem/SIM not active)");
            let w = (ui.text_w(txt, 11.5, false) + ui.dp(28.0)).min(ui.w - 2.0 * m);
            let r = Rect::new((ui.w - w) / 2.0, y, w, ui.dp(28.0));
            ui.cv.rrect(r, ui.dp(14.0), rgb(0xF08A4B).alpha(0.14));
            ui.label(r, txt, 11.5, rgb(0xF6B489), false, Al::Center);
            y += ui.dp(38.0);
        }
        if threads.is_empty() {
            ui.icon(Ic::Chat, ui.w / 2.0, y + ui.dp(90.0), ui.dp(44.0), cs.alpha(0.5));
            ui.label(Rect::new(0.0, y + ui.dp(124.0), ui.w, ui.dp(24.0)), self.t("No messages"), 16.0, c, true, Al::Center);
            ui.label(Rect::new(0.0, y + ui.dp(150.0), ui.w, ui.dp(20.0)), self.t("Start a conversation with the pen button"), 12.5, cs, false, Al::Center);
            y += ui.dp(190.0);
        }
        // Liste wie in der Web-Demo: eine Karte, Zeilen mit Trennlinie, farbiger Avatar
        let rh = ui.dp(66.0);
        let m = ui.dp(16.0);
        if !threads.is_empty() {
            ui.card(Rect::new(m, y, ui.w - 2.0 * m, rh * threads.len() as f32));
        }
        for (i, t) in threads.iter().enumerate() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
            y += rh;
            if !ui.visible(r) {
                continue;
            }
            let name = self.contact_name(&t.number);
            if ui.held(r) {
                ui.cv.rrect(r.inset(1.0), ui.dp(18.0), ui.th.press);
            }
            if i > 0 {
                ui.cv.rect(Rect::new(r.x + 1.0, r.y, r.w - 2.0, 1.0), ui.th.line);
            }
            let ax = if ui.rtl { r.right() - ui.dp(14.0) - ui.dp(22.0) } else { r.x + ui.dp(14.0) + ui.dp(22.0) };
            let shown = name.clone().unwrap_or_else(|| "#".into());
            ui.avatar(ax, r.cy(), ui.dp(44.0), &shown, super::app_demo::avatar_color(&shown));
            let last = t.msgs.last();
            let when = last.map(|s| s.when.clone()).unwrap_or_default();
            let ww = if when.is_empty() { 0.0 } else { ui.text_w(&when, 11.0, false) + ui.dp(8.0) };
            let tx = Rect::new(if ui.rtl { r.x + ui.dp(14.0) } else { r.x + ui.dp(70.0) }, r.y, r.w - ui.dp(84.0), r.h);
            let title = name.clone().unwrap_or_else(|| t.number.clone());
            let tw = tx.w - ww;
            let tf = ui.fit(&title, 15.0, false, tw);
            let nx = if ui.rtl { tx.x + ww } else { tx.x };
            ui.label(Rect::new(nx, r.y + ui.dp(13.0), tw, ui.dp(22.0)), &tf, 15.0, c, false, Al::Start);
            let prev = last.map(|s| if s.out { format!("{}: {}", self.t("You"), s.text) } else { s.text.clone() }).unwrap_or_default();
            let pf = ui.fit(&prev, 12.5, false, tw);
            ui.label(Rect::new(nx, r.y + ui.dp(35.0), tw, ui.dp(20.0)), &pf, 12.5, cs, false, Al::Start);
            if !when.is_empty() {
                ui.label(Rect::new(tx.x, r.y, tx.w, r.h), &when, 11.0, cs, false, Al::End);
            }
            if ui.tap(r) {
                self.msg.open = Some(i);
                ui.dirty = true;
            }
        }
        ui.scroll_end(area, y + *sc - area.y + ui.dp(80.0), sc);
        // „Schreiben" oben in der Kopfzeile (wie in der Web-Demo)
        if std::mem::take(&mut self.hdr_tap) {
            self.msg.composing_new = true;
            self.msg.open = None;
            self.go(Screen::MessageNew);
            ui.dirty = true;
        }
    }

    fn messages_thread(&mut self, ui: &mut Ui, area: Rect, t: &Thread) {
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(12.0);
        let name = self.contact_name(&t.number);
        // Kopf: Zurück, Avatar, Name/Nummer, Anrufen
        let hh = ui.dp(64.0);
        let bx = if ui.rtl { ui.w - ui.dp(30.0) } else { ui.dp(30.0) };
        if ui.icon_btn(bx, area.y + hh / 2.0, ui.dp(18.0), Ic::Back, rgb(0).alpha(0.0), c) {
            self.msg.open = None;
            ui.dirty = true;
            return;
        }
        let ax = if ui.rtl { ui.w - ui.dp(78.0) } else { ui.dp(78.0) };
        avatar(ui, ax, area.y + hh / 2.0, ui.dp(20.0), name.as_deref().unwrap_or(""));
        let tx = Rect::new(if ui.rtl { ui.dp(64.0) } else { ui.dp(108.0) }, area.y, ui.w - ui.dp(172.0), hh);
        let title = name.clone().unwrap_or_else(|| t.number.clone());
        let tf = ui.fit(&title, 17.0, true, tx.w);
        ui.label(Rect::new(tx.x, area.y + ui.dp(if name.is_some() { 10.0 } else { 20.0 }), tx.w, ui.dp(24.0)), &tf, 17.0, c, true, Al::Start);
        if name.is_some() {
            let old = ui.rtl;
            ui.label(Rect::new(tx.x, area.y + ui.dp(34.0), tx.w, ui.dp(18.0)), &t.number, 12.0, cs, false, Al::Start);
            ui.rtl = old;
        }
        let cx = if ui.rtl { ui.dp(32.0) } else { ui.w - ui.dp(32.0) };
        if ui.icon_btn(cx, area.y + hh / 2.0, ui.dp(19.0), Ic::Phone, rgb(GREEN).alpha(0.18), rgb(GREEN)) {
            let n = t.number.clone();
            self.phone.number = n.clone();
            self.phone.tel.dial(&n);
            self.go(Screen::Phone);
            ui.dirty = true;
            return;
        }
        ui.cv.rect(Rect::new(0.0, area.y + hh, ui.w, 1.0), ui.th.line);

        // Eingabezeile unten: Feld + runder Senden-Knopf
        let iny = area.bottom() - ui.dp(58.0);
        let fw = ui.w - 2.0 * m - ui.dp(58.0);
        let fx = if ui.rtl { ui.w - m - fw } else { m };
        ui.field(Rect::new(fx, iny, fw, ui.dp(48.0)), "sms_draft", &self.f("sms_draft"), self.t("Message"));
        let sx = if ui.rtl { m + ui.dp(24.0) } else { ui.w - m - ui.dp(24.0) };
        let draft = self.f("sms_draft");
        let (a, oa) = (ui.th.accent, ui.th.on_accent);
        let en = !draft.trim().is_empty();
        if ui.icon_btn(sx, iny + ui.dp(24.0), ui.dp(24.0), Ic::Send, if en { a } else { ui.th.card }, if en { oa } else { cs }) && en {
            let num = t.number.clone();
            self.msg.sms.send(&num, draft.trim());
            self.fields.remove("sms_draft");
            ui.dirty = true;
        }

        // Verlauf: Sprechblasen mit Umbruch, von unten nach oben
        let top = area.y + hh + ui.dp(4.0);
        ui.set_clip(Some(Rect::new(0.0, top, ui.w, iny - top - ui.dp(6.0))));
        let mut y = iny - ui.dp(10.0);
        let maxw = (ui.w * 0.76).min(ui.dp(380.0));
        let pad = ui.dp(12.0);
        for sms in t.msgs.iter().rev() {
            let fs = ui.dp(14.5);
            let (tw, th) = ui.tx.measure(&sms.text, fs, false, maxw - 2.0 * pad);
            let bw = (tw + 2.0 * pad).max(ui.dp(48.0));
            let has_when = !sms.when.is_empty();
            let bh = th + 2.0 * pad - ui.dp(4.0) + if has_when { ui.dp(14.0) } else { 0.0 };
            y -= bh + ui.dp(6.0);
            if y + bh < top {
                break;
            }
            let right = sms.out != ui.rtl;
            let bx = if right { ui.w - m - bw } else { m };
            let r = Rect::new(bx, y, bw, bh);
            let bg = if sms.out { ui.th.accent } else { ui.th.card };
            ui.cv.rrect(r, ui.dp(18.0), bg);
            // kleine „Ecke“ zur Absenderseite
            let cr = if right { Rect::new(r.right() - ui.dp(14.0), r.bottom() - ui.dp(14.0), ui.dp(14.0), ui.dp(14.0)) } else { Rect::new(r.x, r.bottom() - ui.dp(14.0), ui.dp(14.0), ui.dp(14.0)) };
            ui.cv.rrect(cr, ui.dp(4.0), bg);
            let tc = if sms.out { ui.th.on_accent } else { c };
            ui.tx.draw(ui.cv, &sms.text, r.x + pad, r.y + pad - ui.dp(2.0), fs, tc, false, maxw - 2.0 * pad);
            if has_when {
                let wc = if sms.out { ui.th.on_accent.alpha(0.6) } else { cs };
                ui.label(Rect::new(r.x + pad, r.bottom() - ui.dp(20.0), r.w - 2.0 * pad, ui.dp(14.0)), &sms.when, 10.0, wc, false, Al::End);
            }
        }
        ui.set_clip(None);
    }

    /// „Neue Nachricht“: Empfänger (mit Kontakt-Vorschlägen) + Text, dann senden.
    pub(super) fn s_message_new(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("New message"));
        let m = ui.dp(14.0);
        let mut y = area.y;
        ui.field(Rect::new(m, y, area.w - 2.0 * m, ui.dp(48.0)), "sms_to", &self.f("sms_to"), self.t("Name or number"));
        y += ui.dp(56.0);
        // Kontakt-Vorschläge passend zur Eingabe
        self.extra_load();
        let q = self.f("sms_to").to_lowercase();
        let sugg: Vec<(String, String)> = self
            .extra
            .contacts
            .list
            .iter()
            .filter(|ct| q.is_empty() || ct.name.to_lowercase().contains(&q) || ct.number.contains(q.trim()))
            .take(4)
            .map(|ct| (ct.name.clone(), ct.number.clone()))
            .collect();
        let (c, cs) = (ui.th.text, ui.th.sub);
        for (n, num) in sugg {
            if same_number(&num, &self.f("sms_to")) {
                continue;
            }
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(52.0));
            let held = ui.held(r);
            ui.card_h(r, held);
            let ax = if ui.rtl { r.right() - ui.dp(28.0) } else { r.x + ui.dp(28.0) };
            avatar(ui, ax, r.cy(), ui.dp(17.0), &n);
            let tx = Rect::new(if ui.rtl { r.x + ui.dp(12.0) } else { r.x + ui.dp(56.0) }, r.y, r.w - ui.dp(68.0), r.h);
            ui.label(Rect::new(tx.x, r.y + ui.dp(7.0), tx.w, ui.dp(20.0)), &n, 14.5, c, true, Al::Start);
            ui.label(Rect::new(tx.x, r.y + ui.dp(27.0), tx.w, ui.dp(18.0)), &num, 12.0, cs, false, Al::Start);
            if ui.tap(r) {
                self.fields.insert("sms_to", num);
                ui.dirty = true;
            }
            y += ui.dp(58.0);
        }
        y += ui.dp(4.0);
        ui.field(Rect::new(m, y, area.w - 2.0 * m, ui.dp(48.0)), "sms_newtext", &self.f("sms_newtext"), self.t("Message"));
        y += ui.dp(58.0);
        if ui.button(Rect::new(m, y, area.w - 2.0 * m, ui.dp(46.0)), self.t("Send"), Btn::Primary) {
            let to = self.f("sms_to");
            let txt = self.f("sms_newtext");
            if !to.trim().is_empty() && !txt.trim().is_empty() {
                self.msg.sms.send(to.trim(), txt.trim());
                self.fields.remove("sms_to");
                self.fields.remove("sms_newtext");
                self.focus = None;
                self.focus_reset = true;
                let threads = self.msg.sms.threads();
                self.msg.open = threads.iter().position(|t| same_number(&t.number, to.trim()));
                self.go(Screen::Messages);
            } else {
                self.toast(self.t("Enter number and message").into());
            }
            ui.dirty = true;
        }
    }
}
