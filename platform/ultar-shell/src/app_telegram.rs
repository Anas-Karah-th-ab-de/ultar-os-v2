//! Telegram-App (nativer MTProto-Client über das Crate `ultar-telegram`).
//! Untermodul von `app`. Die Oberfläche spricht nur das `TgBackend`-Trait; das echte
//! Netz-Backend (grammers) wird beim Gerätebau über das Feature aktiviert, sonst Mock.
//!
//! api_id/api_hash des Nutzers (kostenlos auf my.telegram.org) liegen in
//! /data/ultar/telegram.conf  (Zeile1: api_id, Zeile2: api_hash).

use super::App;
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Btn, Ui};
use ultar_telegram::{LoginStage, TgBackend};

pub struct TelegramState {
    pub backend: Box<dyn TgBackend>,
    pub open: i64,
    pub connected: bool,
}

impl Default for TelegramState {
    fn default() -> Self {
        // Gerätebau mit echtem Netz: Feature "telegram-real" in ultar-shell -> GrammersTg.
        #[cfg(feature = "telegram-real")]
        let backend: Box<dyn TgBackend> =
            Box::new(ultar_telegram::grammers_backend::GrammersTg::default());
        #[cfg(not(feature = "telegram-real"))]
        let backend: Box<dyn TgBackend> = Box::new(ultar_telegram::MockTg::default());
        Self {
            backend,
            open: 0,
            connected: false,
        }
    }
}

fn read_api() -> Option<(i32, String)> {
    let s = std::fs::read_to_string("/data/ultar/telegram.conf").ok()?;
    let mut it = s.lines();
    let id: i32 = it.next()?.trim().parse().ok()?;
    let hash = it.next()?.trim().to_string();
    if hash.is_empty() {
        return None;
    }
    Some((id, hash))
}

impl App {
    pub(super) fn s_telegram(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, "Telegram");
        let m = ui.dp(12.0);
        // Verbindung einmalig aufbauen
        if !self.tg.connected {
            match read_api() {
                Some((id, hash)) => {
                    self.tg
                        .backend
                        .connect(id, &hash, "/data/ultar/telegram.session");
                    self.tg.connected = true;
                }
                None => {
                    // noch nicht eingerichtet: Hinweis-Karte im Demo-Stil
                    let mm = ui.dp(16.0);
                    let r = Rect::new(mm, area.y, ui.w - 2.0 * mm, ui.dp(140.0));
                    ui.card(r);
                    ui.icon(
                        crate::ui::Ic::Send,
                        r.cx(),
                        r.y + ui.dp(34.0),
                        ui.dp(26.0),
                        rgb(0x38BDF8),
                    );
                    let c = ui.th.sub;
                    ui.para(r.x + ui.dp(16.0), r.y + ui.dp(62.0), r.w - ui.dp(32.0),
                        self.t("Set your Telegram api_id/api_hash (from my.telegram.org) in /data/ultar/telegram.conf"), 13.5, c);
                    return;
                }
            }
        }
        self.tg.backend.pump();
        if self.tg.backend.dirty() {
            ui.dirty = true;
        }

        match self.tg.backend.stage() {
            LoginStage::Busy => {
                let c = ui.th.sub;
                ui.label(
                    Rect::new(m, area.y + ui.dp(40.0), ui.w - 2.0 * m, ui.dp(24.0)),
                    self.t("Working…"),
                    15.0,
                    c,
                    false,
                    Al::Center,
                );
            }
            LoginStage::Error(e) => {
                let c = ui.th.danger;
                ui.para(
                    m + ui.dp(4.0),
                    area.y + ui.dp(10.0),
                    ui.w - 2.0 * m,
                    &e,
                    14.0,
                    c,
                );
                self.tg_login_phone(ui, area, m); // Neuversuch erlauben
            }
            LoginStage::NeedPhone => self.tg_login_phone(ui, area, m),
            LoginStage::NeedCode => {
                self.tg_login_field(ui, area, m, "tg_code", self.t("Enter the code"), true)
            }
            LoginStage::NeedPassword => {
                self.tg_login_field(ui, area, m, "tg_pw", self.t("Two-step password"), false)
            }
            LoginStage::Ready => {
                if self.tg.open == 0 {
                    self.tg_chatlist(ui, area, m);
                } else {
                    self.tg_chat(ui, area, m);
                }
            }
        }
    }

    fn tg_login_phone(&mut self, ui: &mut Ui, area: Rect, m: f32) {
        let mut y = area.y + ui.dp(20.0);
        let c = ui.th.sub;
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(22.0)),
            self.t("Phone number (with country code)"),
            14.0,
            c,
            false,
            Al::Start,
        );
        y += ui.dp(28.0);
        ui.field(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            "tg_phone",
            &self.f("tg_phone"),
            "+49…",
        );
        y += ui.dp(62.0);
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
            self.t("Request code"),
            Btn::Primary,
        ) {
            let p = self.f("tg_phone").trim().to_string();
            if !p.is_empty() {
                self.tg.backend.request_code(&p);
            }
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
        }
    }

    fn tg_login_field(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        m: f32,
        id: &'static str,
        label: &str,
        is_code: bool,
    ) {
        let mut y = area.y + ui.dp(20.0);
        let c = ui.th.sub;
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(22.0)),
            label,
            14.0,
            c,
            false,
            Al::Start,
        );
        y += ui.dp(28.0);
        ui.field(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            id,
            &self.f(id),
            "",
        );
        y += ui.dp(62.0);
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
            self.t("Sign in"),
            Btn::Primary,
        ) {
            let v = self.f(id).trim().to_string();
            if is_code {
                self.tg.backend.submit_code(&v);
            } else {
                self.tg.backend.submit_password(&v);
            }
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
        }
    }

    fn tg_chatlist(&mut self, ui: &mut Ui, area: Rect, m: f32) {
        let chats = self.tg.backend.chats();
        if chats.is_empty() {
            let c = ui.th.sub;
            ui.label(
                Rect::new(m, area.y + ui.dp(30.0), ui.w - 2.0 * m, ui.dp(24.0)),
                self.t("No chats"),
                15.0,
                c,
                false,
                Al::Center,
            );
            return;
        }
        // Liste wie in der Web-Demo: eine Karte, Zeilen mit Avatar, Name, letzte Nachricht, Zähler
        let m = ui.dp(16.0);
        let rh = ui.dp(66.0);
        let mut y = area.y;
        ui.card(Rect::new(m, y, ui.w - 2.0 * m, rh * chats.len() as f32));
        for (i, ch) in chats.iter().enumerate() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
            y += rh;
            if !ui.visible(r) {
                continue;
            }
            if ui.held(r) {
                ui.cv.rrect(r.inset(1.0), ui.dp(18.0), ui.th.press);
            }
            if i > 0 {
                ui.cv
                    .rect(Rect::new(r.x + 1.0, r.y, r.w - 2.0, 1.0), ui.th.line);
            }
            let ax = if ui.rtl {
                r.right() - ui.dp(36.0)
            } else {
                r.x + ui.dp(36.0)
            };
            ui.avatar(ax, r.cy(), ui.dp(44.0), &ch.title, rgb(0x2B6CB0));
            let bw = if ch.unread > 0 { ui.dp(36.0) } else { 0.0 };
            let tx = Rect::new(
                if ui.rtl {
                    r.x + ui.dp(14.0) + bw
                } else {
                    r.x + ui.dp(70.0)
                },
                r.y,
                r.w - ui.dp(84.0) - bw,
                r.h,
            );
            let tf = ui.fit(&ch.title, 15.0, false, tx.w);
            ui.label(
                Rect::new(tx.x, r.y + ui.dp(13.0), tx.w, ui.dp(22.0)),
                &tf,
                15.0,
                ui.th.text,
                false,
                Al::Start,
            );
            let lf = ui.fit(&ch.last, 12.5, false, tx.w);
            ui.label(
                Rect::new(tx.x, r.y + ui.dp(35.0), tx.w, ui.dp(20.0)),
                &lf,
                12.5,
                ui.th.sub,
                false,
                Al::Start,
            );
            if ch.unread > 0 {
                let bx = if ui.rtl {
                    r.x + ui.dp(14.0)
                } else {
                    r.right() - ui.dp(14.0) - ui.dp(26.0)
                };
                let b = Rect::new(bx, r.cy() - ui.dp(11.0), ui.dp(26.0), ui.dp(22.0));
                ui.cv.rrect(b, ui.dp(11.0), ui.th.accent);
                ui.label(
                    b,
                    &ch.unread.to_string(),
                    12.0,
                    ui.th.on_accent,
                    true,
                    Al::Center,
                );
            }
            if ui.tap(r) {
                self.tg.open = ch.id;
                self.tg.backend.open(ch.id);
                ui.dirty = true;
            }
        }
    }

    fn tg_chat(&mut self, ui: &mut Ui, area: Rect, m: f32) {
        // Kopf: Zurück zur Liste
        if ui.button(
            Rect::new(m, area.y, ui.dp(90.0), ui.dp(40.0)),
            self.t("Back"),
            Btn::Plain,
        ) {
            self.tg.open = 0;
            ui.dirty = true;
            return;
        }
        let input_h = ui.dp(60.0);
        let top = area.y + ui.dp(48.0);
        let bottom = area.bottom() - input_h;
        let msgs = self.tg.backend.messages();
        // Nachrichten von unten
        let mut y = bottom - ui.dp(8.0);
        for msmsg in msgs.iter().rev() {
            let tw = (ui.w - 2.0 * m) * 0.74;
            let lines = 1.0 + msmsg.text.chars().filter(|&c| c == '\n').count() as f32;
            let bh = ui.dp(26.0) + lines * ui.dp(20.0);
            y -= bh + ui.dp(6.0);
            if y < top {
                break;
            }
            let bx = if msmsg.out { ui.w - m - tw } else { m };
            let r = Rect::new(bx, y, tw, bh);
            let col = if msmsg.out { ui.th.accent } else { ui.th.card };
            ui.cv.rrect(r, ui.dp(14.0), col);
            let tc = if msmsg.out { ui.th.bg } else { ui.th.text };
            ui.para(
                r.x + ui.dp(10.0),
                r.y + ui.dp(8.0),
                r.w - ui.dp(20.0),
                &msmsg.text,
                14.0,
                tc,
            );
        }
        // Eingabe + Senden
        let iy = bottom + ui.dp(6.0);
        let bw = ui.dp(90.0);
        ui.field(
            Rect::new(m, iy, ui.w - 2.0 * m - bw - ui.dp(6.0), ui.dp(48.0)),
            "tg_draft",
            &self.f("tg_draft"),
            self.t("Message"),
        );
        if ui.button(
            Rect::new(ui.w - m - bw, iy, bw, ui.dp(48.0)),
            self.t("Send"),
            Btn::Primary,
        ) {
            let txt = self.f("tg_draft").trim().to_string();
            if !txt.is_empty() {
                self.tg.backend.send(self.tg.open, &txt);
                self.fields.remove("tg_draft");
            }
            ui.dirty = true;
        }
    }
}
