//! Bildschirme und Bausteine, die 1:1 der Web-Demo (code-craft.tech/products/ultar-os/demo) folgen:
//! Blatt von unten (wie `sheet()`), Kontakte mit Profilkarte und Buchstaben-Abschnitten, Kontaktseite.

use super::{App, Screen};
use crate::gfx::{rgb, Color, Rect};
use crate::ui::{Al, Btn, Ic, Ui};

/// Blatt von unten über der Seite (Abdunklung dahinter, über der Tastatur)
#[derive(Clone, PartialEq, Debug)]
pub enum Sheet {
    NewContact,
    /// Neuer Wecker (mittig, wie `sheet(..., { mid: true })`)
    NewAlarm,
    /// Neuer Termin im Kalender
    NewEvent,
}

/// Hijri-Datum als Text: Arabisch „9 جمادى الآخرة 1448 هـ", sonst „9. Jumada II 1448 AH"
pub fn hijri_str(lang: &str, y: i64, m: i64, d: i64) -> String {
    let (hy, hm, hd) = crate::sysy::hijri(y, m, d);
    let i = (hm - 1).clamp(0, 11) as usize;
    if lang == "ar" {
        format!("{hd} {} {hy} هـ", crate::sysy::HIJRI_MONTHS_AR[i])
    } else {
        format!("{hd}. {} {hy} AH", crate::sysy::HIJRI_MONTHS[i])
    }
}

/// Farbe für Avatare wie in der Demo
pub fn avatar_color(name: &str) -> Color {
    const P: [u32; 6] = [0x2B6CB0, 0x9B2C6C, 0x2F855A, 0xB7791F, 0xC53030, 0x5A4A1C];
    let h = name
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    rgb(P[(h % P.len() as u32) as usize])
}

impl App {
    /// Blatt zeichnen: `top`/`bottom` = nutzbarer Bereich (unten = Tastatur- bzw. Leistenkante)
    pub(super) fn d_sheet(&mut self, ui: &mut Ui, top: f32, bottom: f32) {
        let Some(kind) = self.sheet.clone() else {
            return;
        };
        ui.set_clip(None);
        if kind == Sheet::NewAlarm {
            self.d_alarm_sheet(ui, top, bottom);
            return;
        }
        ui.cv
            .rect(Rect::new(0.0, 0.0, ui.w, bottom), rgb(0).alpha(0.55));
        let m = ui.dp(16.0);
        let content_h = match kind {
            Sheet::NewContact => {
                ui.dp(10.0 + 4.0 + 12.0 + 32.0 + 48.0 + 10.0 + 48.0 + 14.0 + 46.0 + 18.0)
            }
            Sheet::NewAlarm => 0.0,
            Sheet::NewEvent => {
                ui.dp(10.0 + 4.0 + 12.0 + 32.0 + 48.0 + 10.0 + 48.0 + 14.0 + 46.0 + 18.0)
            }
        };
        let h = content_h.min(bottom - top - ui.dp(20.0));
        let r = Rect::new(0.0, bottom - h, ui.w, h + ui.dp(28.0));
        // obere Ecken rund, unten bündig
        ui.cv.rrect(r, ui.dp(28.0), ui.th.line);
        ui.cv
            .rrect(Rect::new(r.x, r.y + 1.0, r.w, r.h), ui.dp(28.0), ui.th.card);
        // Griff
        ui.cv.rrect(
            Rect::new(
                ui.w / 2.0 - ui.dp(18.0),
                r.y + ui.dp(10.0),
                ui.dp(36.0),
                ui.dp(4.0),
            ),
            ui.dp(2.0),
            ui.th.line.mix(ui.th.sub, 0.3),
        );
        let mut y = r.y + ui.dp(26.0);
        let c = ui.th.text;
        // Außerhalb tippen = schließen
        if ui.p.released && !ui.p.moved && ui.p.py < r.y && ui.p.capture.is_none() {
            self.sheet = None;
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
            return;
        }
        match kind {
            Sheet::NewContact => {
                ui.label(
                    Rect::new(m + ui.dp(2.0), y, ui.w - 2.0 * m, ui.dp(28.0)),
                    self.t("New contact"),
                    19.0,
                    c,
                    true,
                    Al::Start,
                );
                y += ui.dp(36.0);
                ui.field(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                    "cname",
                    &self.f("cname"),
                    self.t("Name"),
                );
                y += ui.dp(58.0);
                ui.field(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                    "cnum",
                    &self.f("cnum"),
                    "+963 …",
                );
                y += ui.dp(62.0);
                if ui.button(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
                    self.t("Save"),
                    Btn::Primary,
                ) {
                    let (n, num) = (
                        self.f("cname").trim().to_string(),
                        self.f("cnum").trim().to_string(),
                    );
                    if n.is_empty() {
                        self.toast(self.t("Invalid input").into());
                    } else {
                        self.extra_load();
                        self.extra.contacts.list.push(super::app_extra::Contact {
                            name: n,
                            number: if num.is_empty() { "—".into() } else { num },
                            fav: false,
                        });
                        self.contacts_save();
                        self.sheet = None;
                        self.focus = None;
                        self.focus_reset = true;
                        self.toast(self.t("Saved").into());
                    }
                    ui.dirty = true;
                }
            }
            Sheet::NewAlarm => {}
            Sheet::NewEvent => {
                ui.label(
                    Rect::new(m + ui.dp(2.0), y, ui.w - 2.0 * m, ui.dp(28.0)),
                    self.t("New event"),
                    19.0,
                    c,
                    true,
                    Al::Start,
                );
                y += ui.dp(36.0);
                ui.field(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                    "evt",
                    &self.f("evt"),
                    self.t("Title"),
                );
                y += ui.dp(58.0);
                ui.field(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                    "evh",
                    &self.f("evh"),
                    "09:00",
                );
                y += ui.dp(62.0);
                if ui.button(
                    Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
                    self.t("Save"),
                    Btn::Primary,
                ) {
                    let title = {
                        let t = self.f("evt").trim().to_string();
                        if t.is_empty() {
                            self.t("Event").to_string()
                        } else {
                            t
                        }
                    };
                    let tm = self.f("evh");
                    let at = tm
                        .trim()
                        .split_once(':')
                        .and_then(|(h, mm)| {
                            Some((
                                h.trim().parse::<i32>().ok()?,
                                mm.trim().parse::<i32>().ok()?,
                            ))
                        })
                        .filter(|(h, mm)| (0..24).contains(h) && (0..60).contains(mm))
                        .map(|(h, mm)| h * 60 + mm)
                        .unwrap_or(-1);
                    let (cy, cm) = self.sx_cal_ym();
                    let d = self.sx_cal_day();
                    let mut cal: crate::sysx::Calendar = crate::sys::load("calendar.toml");
                    cal.events.push(crate::sysx::Event {
                        y: cy,
                        mo: cm,
                        d,
                        at,
                        title,
                        remind: if at < 0 { 0 } else { 15 },
                        notified: false,
                    });
                    let _ = crate::sys::save("calendar.toml", &cal);
                    self.sheet = None;
                    self.focus = None;
                    self.focus_reset = true;
                    ui.dirty = true;
                }
            }
        }
    }

    /// Mittiges Fenster „Neuer Wecker": Stunde/Minute mit Pfeilen, Speichern
    fn d_alarm_sheet(&mut self, ui: &mut Ui, top: f32, bottom: f32) {
        use crate::ui::Ic;
        ui.cv
            .rect(Rect::new(0.0, 0.0, ui.w, bottom), rgb(0).alpha(0.55));
        let w = (ui.w - ui.dp(48.0)).min(ui.dp(420.0));
        let h = ui.dp(300.0);
        let r = Rect::new((ui.w - w) / 2.0, top + (bottom - top - h) / 2.0, w, h);
        if ui.p.released && !ui.p.moved && !r.contains(ui.p.px, ui.p.py) && ui.p.capture.is_none() {
            self.sheet = None;
            ui.dirty = true;
            return;
        }
        ui.cv.rrect(r, ui.dp(26.0), ui.th.line);
        ui.cv.rrect(r.inset(1.0), ui.dp(25.0), ui.th.card);
        let c = ui.th.text;
        let pad = ui.dp(16.0);
        ui.label(
            Rect::new(
                r.x + pad + ui.dp(2.0),
                r.y + ui.dp(16.0),
                r.w - 2.0 * pad,
                ui.dp(28.0),
            ),
            self.t("New alarm"),
            19.0,
            c,
            true,
            Al::Start,
        );
        let old = ui.rtl;
        ui.rtl = false;
        let cy = r.y + ui.dp(140.0);
        let col_w = ui.dp(80.0);
        for (k, cx) in [
            (0, r.cx() - col_w / 2.0 - ui.dp(16.0)),
            (1, r.cx() + col_w / 2.0 + ui.dp(16.0)),
        ] {
            let v = if k == 0 { self.tl.new_h } else { self.tl.new_m };
            if ui.icon_btn(
                cx,
                cy - ui.dp(62.0),
                ui.dp(22.0),
                Ic::Chevron,
                ui.th.press,
                c,
            ) {
                if k == 0 {
                    self.tl.new_h = (self.tl.new_h + 1) % 24
                } else {
                    self.tl.new_m = (self.tl.new_m + 5) % 60
                }
                ui.dirty = true;
            }
            ui.label(
                Rect::new(cx - col_w / 2.0, cy - ui.dp(30.0), col_w, ui.dp(60.0)),
                &format!("{v:02}"),
                48.0,
                c,
                false,
                Al::Center,
            );
            if ui.icon_btn(
                cx,
                cy + ui.dp(62.0),
                ui.dp(22.0),
                Ic::Chevron,
                ui.th.press,
                c,
            ) {
                if k == 0 {
                    self.tl.new_h = (self.tl.new_h + 23) % 24
                } else {
                    self.tl.new_m = (self.tl.new_m + 55) % 60
                }
                ui.dirty = true;
            }
        }
        ui.label(
            Rect::new(
                r.cx() - ui.dp(20.0),
                cy - ui.dp(30.0),
                ui.dp(40.0),
                ui.dp(60.0),
            ),
            ":",
            48.0,
            c,
            false,
            Al::Center,
        );
        ui.rtl = old;
        let br = Rect::new(
            r.x + pad,
            r.bottom() - pad - ui.dp(46.0),
            r.w - 2.0 * pad,
            ui.dp(46.0),
        );
        if ui.button(br, self.t("Save"), Btn::Primary) {
            self.tl.alarms.list.push(super::tools::Alarm {
                h: self.tl.new_h,
                m: self.tl.new_m,
                on: true,
                days: String::new(),
            });
            let _ = crate::sys::save("alarms.toml", &self.tl.alarms);
            let dt = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min);
            let diff = ((self.tl.new_h as i64 * 60 + self.tl.new_m as i64)
                - (dt.h as i64 * 60 + dt.mi as i64))
                .rem_euclid(1440);
            self.toast(format!(
                "{} {} h {} min",
                self.t("Alarm in"),
                diff / 60,
                diff % 60
            ));
            self.sheet = None;
            ui.dirty = true;
        }
    }

    /// Blatt „Neuer Kontakt" öffnen (Name fokussiert, Tastatur kommt)
    pub(super) fn new_contact_sheet(&mut self) {
        self.fields.insert("cname", String::new());
        self.fields.insert("cnum", String::new());
        self.sheet = Some(Sheet::NewContact);
        self.focus = Some("cname");
        self.focus_reset = true;
    }

    /// Kontakte wie in der Demo: Profilkarte, Buchstaben-Abschnitte, Karten mit Avatar und Name
    pub(super) fn s_contacts_demo(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        self.extra_load();
        let area = self.header(ui, area, self.t("Contacts"));
        if std::mem::take(&mut self.hdr_tap) {
            self.new_contact_sheet();
            ui.dirty = true;
        }
        let mut list: Vec<(usize, super::app_extra::Contact)> = self
            .extra
            .contacts
            .list
            .iter()
            .cloned()
            .enumerate()
            .collect();
        list.sort_by_key(|a| a.1.name.to_lowercase());
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        // Mein Profil
        let pr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(76.0));
        ui.card(pr);
        let ax = if ui.rtl {
            pr.right() - ui.dp(14.0) - ui.dp(23.0)
        } else {
            pr.x + ui.dp(14.0) + ui.dp(23.0)
        };
        ui.avatar(ax, pr.cy(), ui.dp(46.0), "I", rgb(0x5A4A1C));
        let tx = if ui.rtl {
            pr.x + ui.dp(14.0)
        } else {
            pr.x + ui.dp(72.0)
        };
        let tw = pr.w - ui.dp(86.0);
        ui.label(
            Rect::new(tx, pr.y + ui.dp(16.0), tw, ui.dp(22.0)),
            self.t("My profile"),
            15.0,
            c,
            true,
            Al::Start,
        );
        let num = ultar_cloud::my_number().unwrap_or_else(|| "UOS-—".into());
        ui.label(
            Rect::new(tx, pr.y + ui.dp(39.0), tw, ui.dp(20.0)),
            &num,
            13.0,
            cs,
            false,
            Al::Start,
        );
        if ui.tap(pr) {
            self.go(Screen::Account);
            ui.dirty = true;
        }
        y += ui.dp(76.0 + 14.0);
        let mut last = String::new();
        for (i, ct) in &list {
            let l: String = ct
                .name
                .chars()
                .next()
                .map(|ch| ch.to_uppercase().collect())
                .unwrap_or_default();
            if l != last {
                ui.label(
                    Rect::new(
                        m + ui.dp(8.0),
                        y + ui.dp(6.0),
                        ui.w - 2.0 * m - ui.dp(16.0),
                        ui.dp(16.0),
                    ),
                    &l,
                    12.0,
                    cs,
                    false,
                    Al::Start,
                );
                y += ui.dp(24.0);
                last = l;
            }
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(56.0));
            y += ui.dp(62.0);
            if !ui.visible(r) {
                continue;
            }
            // wie in der Demo: flache Zeile mit feiner Linie oben
            if ui.held(r) {
                ui.cv.rrect(r, ui.dp(16.0), ui.th.press);
            }
            ui.cv.rect(
                Rect::new(r.x + ui.dp(8.0), r.y, r.w - ui.dp(16.0), 1.0),
                ui.th.line,
            );
            let ax = if ui.rtl {
                r.right() - ui.dp(14.0) - ui.dp(20.0)
            } else {
                r.x + ui.dp(14.0) + ui.dp(20.0)
            };
            ui.avatar(ax, r.cy(), ui.dp(40.0), &ct.name, avatar_color(&ct.name));
            let tx = if ui.rtl {
                r.x + ui.dp(14.0)
            } else {
                r.x + ui.dp(66.0)
            };
            let nf = ui.fit(&ct.name, 15.0, false, r.w - ui.dp(80.0));
            ui.label(
                Rect::new(tx, r.y, r.w - ui.dp(80.0), r.h),
                &nf,
                15.0,
                c,
                false,
                Al::Start,
            );
            if ui.tap(r) {
                self.extra.del_armed = false;
                self.go(Screen::ContactEdit(Some(*i)));
                ui.dirty = true;
            }
        }
        ui.scroll_end(area, y + *sc - area.y + ui.dp(24.0), sc);
    }

    /// Kontaktseite wie in der Demo: großer Avatar, Name, Anrufen/SMS/Kopieren, Nummer
    pub(super) fn s_contact_demo(&mut self, ui: &mut Ui, area: Rect, i: usize, sc: &mut f32) {
        self.extra_load();
        let Some(ct) = self.extra.contacts.list.get(i).cloned() else {
            self.back();
            ui.dirty = true;
            return;
        };
        let area = self.header(ui, area, "");
        if std::mem::take(&mut self.hdr_tap) {
            if self.extra.del_armed {
                self.extra.contacts.list.remove(i);
                self.contacts_save();
                self.extra.del_armed = false;
                self.back();
                self.toast(self.t("Deleted").into());
            } else {
                self.extra.del_armed = true;
                self.toast(self.t("Tap again to delete").into());
            }
            ui.dirty = true;
            return;
        }
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let mut y = area.y - *sc + ui.dp(10.0);
        let c = ui.th.text;
        ui.avatar(
            ui.w / 2.0,
            y + ui.dp(48.0),
            ui.dp(96.0),
            &ct.name,
            avatar_color(&ct.name),
        );
        y += ui.dp(106.0);
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(32.0)),
            &ct.name,
            24.0,
            c,
            true,
            Al::Center,
        );
        y += ui.dp(52.0);
        let gap = ui.dp(10.0);
        let cw = (ui.w - 2.0 * m - 2.0 * gap) / 3.0;
        let acts = [
            (Ic::Phone, self.t("Call")),
            (Ic::Chat, "SMS"),
            (Ic::Copy, self.t("Copy")),
        ];
        for (k, (ic, lab)) in acts.iter().enumerate() {
            let col = if ui.rtl { 2 - k } else { k };
            let r = Rect::new(m + col as f32 * (cw + gap), y, cw, ui.dp(70.0));
            let held = ui.held(r);
            ui.card_h(r, held);
            ui.icon(*ic, r.cx(), r.y + ui.dp(25.0), ui.dp(22.0), ui.th.accent);
            ui.label(
                Rect::new(r.x, r.y + ui.dp(42.0), r.w, ui.dp(18.0)),
                lab,
                12.0,
                c,
                false,
                Al::Center,
            );
            if ui.tap(r) {
                match k {
                    0 => {
                        self.phone.number = ct.number.clone();
                        self.phone.tab = 0;
                        self.go(Screen::Phone);
                        self.phone.tel.dial(&ct.number);
                    }
                    1 => {
                        self.fields.insert("sms_to", ct.number.clone());
                        self.msg.composing_new = true;
                        self.msg.open = None;
                        self.go(Screen::MessageNew);
                    }
                    _ => self.copy_text(&ct.number.clone()),
                }
                ui.dirty = true;
                return;
            }
        }
        y += ui.dp(86.0);
        let sub = if ct.number.starts_with("UOS") {
            self.t("Ultar number · encrypted")
        } else {
            self.t("Mobile")
        };
        let old = ui.rtl;
        let _ = old;
        if ui.row(&mut y, Some(Ic::Phone), &ct.number, Some(sub), None, false) {
            self.phone.number = ct.number.clone();
            self.go(Screen::Phone);
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y + ui.dp(20.0), sc);
    }

    /// Hijri-Datum wie in der Demo: Karte mit Kalender-Symbol, Hijri groß, Datum, arabische Schreibweise
    pub(super) fn s_hijri(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Hijri date"));
        let m = ui.dp(16.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let dt = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min);
        let (hy, hm, hd) = crate::sysy::hijri(dt.y, dt.mo, dt.d);
        let mi = (hm - 1).clamp(0, 11) as usize;
        let r = Rect::new(m, area.y, ui.w - 2.0 * m, ui.dp(250.0));
        ui.card(r);
        ui.icon(
            Ic::Clock,
            r.cx(),
            r.y + ui.dp(54.0),
            ui.dp(40.0),
            ui.th.accent,
        );
        let hmon = if self.set.lang == "ar" {
            crate::sysy::HIJRI_MONTHS_AR[mi]
        } else {
            crate::sysy::HIJRI_MONTHS[mi]
        };
        ui.label(
            Rect::new(r.x, r.y + ui.dp(92.0), r.w, ui.dp(36.0)),
            &format!("{hd}. {hmon} {hy} AH"),
            26.0,
            c,
            true,
            Al::Center,
        );
        let greg = format!(
            "{}, {}. {} {}",
            self.t(crate::i18n::DAYS[dt.wd as usize % 7]),
            dt.d,
            self.t(super::app_sys::MONTHS[(dt.mo as usize).clamp(1, 12) - 1]),
            dt.y
        );
        ui.label(
            Rect::new(r.x, r.y + ui.dp(134.0), r.w, ui.dp(22.0)),
            &greg,
            14.0,
            cs,
            false,
            Al::Center,
        );
        let ar = format!("{hd} {} {hy} هـ", crate::sysy::HIJRI_MONTHS_AR[mi]);
        let old = ui.rtl;
        ui.rtl = true;
        ui.label(
            Rect::new(r.x, r.y + ui.dp(176.0), r.w, ui.dp(30.0)),
            &ar,
            20.0,
            c,
            false,
            Al::Center,
        );
        ui.rtl = old;
    }

    /// Wie viele Adressen der Helfer schon geprüft hat
    pub(super) fn buddy_links_checked(&self) -> usize {
        self.buddy.guard.len()
    }

    /// Helfer öffnen, um einen Link zu prüfen
    pub(super) fn buddy_open_linkcheck(&mut self) {
        self.buddy.away = false;
        self.buddy.open = true;
        self.focus = Some("buddy");
        self.focus_reset = true;
    }

    /// Vorschaltseite einer Web-App wie in der Demo: großes Symbol, Name, Text, Profil-Hinweis, Öffnen-Knopf
    /// Liefert die y-Position unter dem Knopf (für Zusätze wie Spotify Connect).
    pub(super) fn webapp_page(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        sc: &mut f32,
        name: &str,
        ic: Ic,
        col: u32,
        url: &str,
        text: &str,
    ) -> f32 {
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(16.0);
        let mut y = area.y - *sc + ui.dp(40.0);
        let d = ui.dp(90.0);
        let ir = Rect::new(ui.w / 2.0 - d / 2.0, y, d, d);
        ui.cv.rrect(ir, ui.dp(28.0), rgb(col).alpha(0.14));
        ui.icon(ic, ir.cx(), ir.cy(), ui.dp(46.0), rgb(col));
        y += d + ui.dp(14.0);
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(28.0)),
            name,
            20.0,
            c,
            true,
            Al::Center,
        );
        y += ui.dp(36.0);
        let tw = (ui.w - 2.0 * m).min(ui.dp(300.0));
        let lines: Vec<String> = wrap(ui, text, 14.0, tw);
        for l in &lines {
            ui.label(
                Rect::new((ui.w - tw) / 2.0, y, tw, ui.dp(22.0)),
                l,
                14.0,
                cs,
                false,
                Al::Center,
            );
            y += ui.dp(22.0);
        }
        y += ui.dp(14.0);
        let note = self.t("Own persistent profile – sign in once. Ads blocked.");
        let nl = wrap(ui, note, 13.0, ui.w - 2.0 * m - ui.dp(66.0));
        let nh = ui.dp(24.0) + nl.len() as f32 * ui.dp(19.0);
        let nr = Rect::new(m, y, ui.w - 2.0 * m, nh);
        ui.card(nr);
        let ix = if ui.rtl {
            nr.right() - ui.dp(26.0)
        } else {
            nr.x + ui.dp(26.0)
        };
        ui.icon(Ic::Shield, ix, nr.cy(), ui.dp(18.0), ui.th.accent);
        for (i, l) in nl.iter().enumerate() {
            ui.label(
                Rect::new(
                    if ui.rtl {
                        nr.x + ui.dp(14.0)
                    } else {
                        nr.x + ui.dp(50.0)
                    },
                    nr.y + ui.dp(12.0) + i as f32 * ui.dp(19.0),
                    nr.w - ui.dp(64.0),
                    ui.dp(19.0),
                ),
                l,
                13.0,
                c,
                false,
                Al::Start,
            );
        }
        y += nh + ui.dp(14.0);
        let host = url
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        let lab = format!("{}  {host}", self.t("Open"));
        let bw = (ui.text_w(&lab, 15.0, true) + ui.dp(70.0)).min(ui.w - 2.0 * m);
        let br = Rect::new((ui.w - bw) / 2.0, y, bw, ui.dp(46.0));
        if ui.button(br, &lab, Btn::Primary) {
            self.go(Screen::Web(url.to_string()));
            ui.dirty = true;
        }
        y + ui.dp(60.0)
    }

    pub(super) fn s_webapp(&mut self, ui: &mut Ui, area: Rect, k: String, sc: &mut f32) {
        let (name, ic, col, url, txt) = match k.as_str() {
            "youtube" => (
                "YouTube",
                Ic::Play,
                0xFF4E45u32,
                "https://m.youtube.com/",
                self.t("No ads, no tracking, with background playback."),
            ),
            _ => (
                "WhatsApp",
                Ic::Chat,
                0x25D366u32,
                "https://web.whatsapp.com/",
                self.t("Runs as a web app with a separate profile."),
            ),
        };
        let area = self.header(ui, area, name);
        ui.scroll_begin(area, sc);
        let y = self.webapp_page(ui, area, sc, name, ic, col, url, txt);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// KM-Helfer als App wie in der Demo: große Figur, Begrüßung, Zustände zum Ansehen, Unterhaltung starten
    pub(super) fn s_helper(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("KM helper"));
        if std::mem::take(&mut self.hdr_tap) {
            self.go(Screen::AgentSet);
            ui.dirty = true;
            return;
        }
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let mut y = area.y - *sc + ui.dp(10.0);
        // Bühne für die Figur (gezeichnet vom Helfer selbst, damit Mimik und Bewegung dieselben sind)
        let fr = ui.dp(70.0);
        self.buddy.stage = Some((ui.w / 2.0, y + ui.dp(105.0), fr));
        y += ui.dp(230.0);
        ui.label(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(28.0)),
            self.t("Hi! How can I help?"),
            20.0,
            c,
            true,
            Al::Center,
        );
        y += ui.dp(34.0);
        let ar = "أنا KM Code Craft مساعدك الشخصي، أعمل على جهازك وأحفظ بياناتك وأبقيها آمنة";
        let old = ui.rtl;
        ui.rtl = true;
        let lines = wrap(ui, ar, 15.0, (ui.w - 2.0 * m).min(ui.dp(320.0)));
        for l in &lines {
            ui.label(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(24.0)),
                l,
                15.0,
                cs,
                false,
                Al::Center,
            );
            y += ui.dp(24.0);
        }
        ui.rtl = old;
        y += ui.dp(10.0);
        let states = [
            (1u8, "Hello"),
            (2, "Listening"),
            (3, "Thinking"),
            (4, "Speaking"),
            (5, "Calling"),
            (6, "Unknown voice"),
            (7, "Sleeping"),
            (0, "Idle"),
        ];
        let gap = ui.dp(8.0);
        let cw = (ui.w - 2.0 * m - gap) / 2.0;
        for (i, (k, lab)) in states.iter().enumerate() {
            let col = if ui.rtl { 1 - i % 2 } else { i % 2 };
            let r = Rect::new(
                m + col as f32 * (cw + gap),
                y + (i / 2) as f32 * ui.dp(40.0),
                cw,
                ui.dp(32.0),
            );
            let on = *k != 0 && self.buddy.preview == *k;
            let held = ui.held(r);
            if on {
                ui.cv.rrect(r, ui.dp(16.0), ui.th.accent);
            } else {
                ui.cv.rrect(r, ui.dp(16.0), ui.th.line);
                ui.cv.rrect(
                    r.inset(1.0),
                    ui.dp(15.0),
                    if held { ui.th.press } else { ui.th.bg },
                );
            }
            ui.label(
                r,
                self.t(lab),
                13.0,
                if on { ui.th.on_accent } else { c },
                on,
                Al::Center,
            );
            if ui.tap(r) {
                self.buddy.preview = *k;
                ui.dirty = true;
            }
        }
        y += 4.0 * ui.dp(40.0) + ui.dp(8.0);
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
            self.t("Start conversation"),
            Btn::Primary,
        ) {
            self.buddy.preview = 0;
            self.buddy_open_linkcheck();
            ui.dirty = true;
        }
        y += ui.dp(66.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}

/// Text auf eine Breite umbrechen (wortweise)
pub fn wrap(ui: &mut Ui, text: &str, size: f32, w: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let t = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if ui.text_w(&t, size, false) > w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = t;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
