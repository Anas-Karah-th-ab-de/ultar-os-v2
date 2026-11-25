//! Desktop-Oberfläche für Laptop und Server: Arbeitsfläche, Leiste mit Start, Fenster für jede App,
//! Startmenü mit Suche – Arabisch komplett gespiegelt (Leiste, Menü, Fensterknöpfe).
//!
//! Jedes Fenster hat einen eigenen Verlauf (wie ein eigener Bildschirm) und zeichnet die vorhandenen
//! Ultar-Bildschirme in eine eigene Zeichenfläche – dieselben Apps wie am Handy, nur in Fenstern.

use super::{App, Screen};
use crate::gfx::{rgb, Canvas, Color, Rect};
use crate::ui::{Al, Ic, Ptr, Ui};

pub struct Fenster {
    pub id: u64,
    pub stack: Vec<(Screen, f32)>,
    /// Außenmaße in Bildpunkten (mit Titelleiste)
    pub r: Rect,
    pub min: bool,
    pub max: bool,
    titel: String,
    ic: Ic,
    cv: Option<Canvas>,
}

#[derive(Clone, Copy, PartialEq)]
enum Ziel {
    Nichts,
    Leiste,
    Start,
    Flaeche,
    Fenster(u64),
    Bewegen(u64, f32, f32),
    Groesse(u64, f32, f32),
}

pub struct Desk {
    pub fenster: Vec<Fenster>,
    pub start: bool,
    ziel: Ziel,
    next: u64,
    /// zuletzt gezeichnet im Desktop-Modus (für Zurück-Taste und Mauszeiger)
    pub an: bool,
    /// gerade wird ein Fenster gezeichnet (Zurück im Fenster wirkt direkt auf dessen Verlauf)
    innen: bool,
}

impl Default for Desk {
    fn default() -> Self {
        Desk {
            fenster: Vec::new(),
            start: false,
            ziel: Ziel::Nichts,
            next: 1,
            an: false,
            innen: false,
        }
    }
}

/// Apps des Desktops (ohne reine Handy-Funktionen)
const OHNE: [&str; 3] = ["Phone", "Messages", "Camera"];
/// angeheftet in der Leiste
const LEISTE: [&str; 5] = ["Files", "Browser", "Terminal", "Ultar Store", "Settings"];

impl App {
    /// Desktop statt Handy-Oberfläche? Umgebung ULTAR_OBERFLAECHE=desktop|handy, sonst Ausgabe Desktop/Server,
    /// sonst breiter Bildschirm ab 1000 Punkten
    pub(super) fn desktop_modus(&self, ui: &Ui) -> bool {
        match std::env::var("ULTAR_OBERFLAECHE").as_deref() {
            Ok("desktop") => return true,
            Ok("handy") => return false,
            _ => {}
        }
        if matches!(
            ultar_hal_system::current_edition(),
            ultar_profile::Edition::Desktop | ultar_profile::Edition::Server
        ) {
            return true;
        }
        ui.w > ui.h && ui.w / ui.k >= 1000.0
    }

    fn desk_apps(&self) -> Vec<(Ic, &'static str, Screen)> {
        self.all_apps()
            .into_iter()
            .filter(|(_, n, _)| !OHNE.contains(n))
            .filter_map(|(ic, n, s)| s.map(|s| (ic, n, s)))
            .collect()
    }

    /// App in einem neuen Fenster öffnen (oder vorhandenes Fenster derselben App nach vorn holen)
    pub(super) fn desk_oeffnen(&mut self, ui: &Ui, ic: Ic, titel: &str, s: Screen) {
        self.desk.start = false;
        if let Some(i) = self
            .desk
            .fenster
            .iter()
            .position(|f| f.stack.first().map(|x| &x.0) == Some(&s))
        {
            let mut f = self.desk.fenster.remove(i);
            f.min = false;
            self.desk.fenster.push(f);
            return;
        }
        let n = self.desk.fenster.len() as f32;
        let (w, h) = (
            (ui.w * 0.62).max(ui.dp(560.0)).min(ui.w - ui.dp(40.0)),
            (ui.h * 0.74).max(ui.dp(420.0)).min(ui.h - ui.dp(90.0)),
        );
        let x = (ui.w - w) / 2.0 + (n % 6.0 - 2.5) * ui.dp(28.0);
        let y = ((ui.h - ui.dp(52.0) - h) / 2.0 + (n % 6.0 - 2.5) * ui.dp(22.0)).max(ui.dp(8.0));
        let id = self.desk.next;
        self.desk.next += 1;
        self.desk.fenster.push(Fenster {
            id,
            stack: vec![(s, 0.0)],
            r: Rect::new(x, y, w, h),
            min: false,
            max: false,
            titel: titel.into(),
            ic,
            cv: None,
        });
        self.focus = None;
    }

    fn desk_schliessen(&mut self, id: u64) {
        self.desk.fenster.retain(|f| f.id != id);
        self.focus = None;
    }

    /// Zurück im aktiven Fenster (Esc/Zurück-Taste); leerer Verlauf schließt das Fenster nicht
    pub(super) fn desk_zurueck(&mut self) -> bool {
        if !self.desk.an || self.desk.innen {
            return false;
        }
        if self.desk.start {
            self.desk.start = false;
            return true;
        }
        let Some(i) = self.desk.fenster.iter().rposition(|f| !f.min) else {
            return true;
        };
        std::mem::swap(&mut self.stack, &mut self.desk.fenster[i].stack);
        self.back_stack();
        std::mem::swap(&mut self.stack, &mut self.desk.fenster[i].stack);
        true
    }

    pub(super) fn draw_desktop(&mut self, ui: &mut Ui) {
        // Prüfen/Bildschirmfotos: ULTAR_DESK_OEFFNEN="Settings,Files" öffnet diese Apps beim ersten Bild
        if !self.desk.an && self.desk.fenster.is_empty() {
            if let Ok(v) = std::env::var("ULTAR_DESK_OEFFNEN") {
                for n in v.split(',') {
                    if let Some((ic, name, s)) =
                        self.desk_apps().into_iter().find(|a| a.1 == n.trim())
                    {
                        let t = self.t(name).to_string();
                        self.desk_oeffnen(ui, ic, &t, s);
                    }
                }
                if v.contains("start") {
                    self.desk.start = true;
                }
            }
        }
        self.desk.an = true;
        let (w, h) = (ui.w, ui.h);
        let rtl = ui.rtl;
        let lh = ui.dp(52.0);
        let fx = |x: f32, bw: f32| if rtl { w - x - bw } else { x };
        // Navigation außerhalb eines Fensters (z. B. Benachrichtigung öffnet einen Bildschirm): als Fenster zeigen
        while self.stack.len() > 1 {
            let (s, _) = self.stack.pop().unwrap();
            let (ic, titel) = self
                .desk_apps()
                .into_iter()
                .find(|(_, _, x)| *x == s)
                .map(|(ic, n, _)| (ic, self.t(n).to_string()))
                .unwrap_or((Ic::Apps, String::from("Ultar")));
            self.desk_oeffnen(ui, ic, &titel, s);
        }
        // ---- Ziel der Maus bestimmen (beim Drücken), solange gedrückt bleibt es
        if ui.p.pressed {
            let (px, py) = (ui.p.x, ui.p.y);
            let start_r = self.desk_start_rect(ui);
            self.desk.ziel = if self.desk.start && start_r.contains(px, py) {
                Ziel::Start
            } else if py >= h - lh {
                Ziel::Leiste
            } else if let Some(f) = self
                .desk
                .fenster
                .iter()
                .rev()
                .find(|f| !f.min && f.r.contains(px, py))
            {
                let id = f.id;
                let r = f.r;
                let th = ui.dp(36.0);
                let ecke = ui.dp(16.0);
                let in_ecke = py > r.bottom() - ecke
                    && if rtl {
                        px < r.x + ecke
                    } else {
                        px > r.right() - ecke
                    };
                // nach vorn holen
                if let Some(i) = self.desk.fenster.iter().position(|f| f.id == id) {
                    let f = self.desk.fenster.remove(i);
                    if self.desk.fenster.last().map(|l| l.id) != Some(id) {
                        self.focus = None;
                    }
                    self.desk.fenster.push(f);
                }
                self.desk.start = false;
                let knoepfe = ui.dp(3.0 * 40.0);
                let auf_knopf = if rtl {
                    px < r.x + knoepfe
                } else {
                    px > r.right() - knoepfe
                };
                if in_ecke {
                    Ziel::Groesse(
                        id,
                        r.w - if rtl { -(px - r.x) } else { px - r.x },
                        r.h - (py - r.y),
                    )
                } else if py < r.y + th && !auf_knopf {
                    Ziel::Bewegen(id, px - r.x, py - r.y)
                } else {
                    Ziel::Fenster(id)
                }
            } else {
                self.desk.start = false;
                Ziel::Flaeche
            };
        }
        let ziel = self.desk.ziel;
        // Fenster ziehen / Größe ändern
        match ziel {
            Ziel::Bewegen(id, ox, oy) if ui.p.down => {
                if let Some(f) = self.desk.fenster.iter_mut().find(|f| f.id == id) {
                    if f.max {
                        f.max = false;
                    }
                    f.r.x = (ui.p.x - ox).clamp(-f.r.w + ui.dp(80.0), w - ui.dp(80.0));
                    f.r.y = (ui.p.y - oy).clamp(0.0, h - lh - ui.dp(36.0));
                }
                ui.dirty = true;
            }
            Ziel::Groesse(id, ow, oh) if ui.p.down => {
                if let Some(f) = self.desk.fenster.iter_mut().find(|f| f.id == id) {
                    let rel = if rtl {
                        f.r.right() - ui.p.x
                    } else {
                        ui.p.x - f.r.x
                    };
                    let neu_w = (rel + ow - f.r.w + f.r.w).max(ui.dp(360.0));
                    let neu_w = neu_w.min(w);
                    if rtl {
                        let rechts = f.r.right();
                        f.r.w = neu_w;
                        f.r.x = rechts - neu_w;
                    } else {
                        f.r.w = neu_w;
                    }
                    f.r.h =
                        (ui.p.y - f.r.y + oh - f.r.h + f.r.h).clamp(ui.dp(260.0), h - lh - f.r.y);
                    let _ = (ow, oh);
                }
                ui.dirty = true;
            }
            _ => {}
        }

        // ---- Arbeitsfläche
        ui.set_clip(None);
        ui.cv
            .vgrad(Rect::new(0.0, 0.0, w, h), rgb(0x10131A), rgb(0x05060A));
        let l = crate::brand::logo();
        let lw = (w.min(h) * 0.34).min(ui.dp(420.0));
        let lhh = lw * l.h as f32 / l.w as f32;
        ui.cv.image(
            &l,
            (w - lw) / 2.0,
            (h - lh - lhh) / 2.0 - ui.dp(20.0),
            lw,
            lhh,
        );
        ui.cv
            .rect(Rect::new(0.0, 0.0, w, h - lh), rgb(0x05060A).alpha(0.55));
        // Symbole auf der Arbeitsfläche
        let symbole: Vec<(Ic, &'static str, Screen)> = self
            .desk_apps()
            .into_iter()
            .filter(|(_, n, _)| {
                [
                    "Files",
                    "Terminal",
                    "Settings",
                    "Ultar Store",
                    "Browser",
                    "KM helper",
                ]
                .contains(n)
            })
            .collect();
        let (sw, shh) = (ui.dp(88.0), ((h - lh - ui.dp(28.0)) / 6.0).min(ui.dp(88.0)));
        let enabled_alt = ui.enabled;
        ui.enabled = ziel == Ziel::Flaeche;
        for (i, (ic, n, s)) in symbole.into_iter().enumerate() {
            let cell = Rect::new(
                fx(ui.dp(14.0), sw),
                ui.dp(14.0) + i as f32 * shh,
                sw,
                shh - ui.dp(4.0),
            );
            if ui.held(cell) || cell.contains(ui.p.x, ui.p.y) {
                ui.cv.rrect(cell, ui.dp(12.0), rgb(0xFFFFFF).alpha(0.07));
            }
            let ir = Rect::new(
                cell.cx() - ui.dp(21.0),
                cell.y + ui.dp(6.0),
                ui.dp(42.0),
                ui.dp(42.0),
            );
            ui.cv
                .rrect_vgrad(ir, ui.dp(12.0), rgb(0x232733), rgb(0x171A22));
            let fg = if n == "KM helper" || n == "Ultar Store" {
                ui.th.accent
            } else {
                rgb(0xE8ECF4)
            };
            ui.icon_w(ic, ir.cx(), ir.cy(), ui.dp(21.0), fg, 1.6);
            let lab = ui.fit(self.t(n), 12.0, false, cell.w - ui.dp(6.0));
            ui.label(
                Rect::new(cell.x, ir.bottom() + ui.dp(4.0), cell.w, ui.dp(18.0)),
                &lab,
                12.0,
                rgb(0xE8ECF4),
                false,
                Al::Center,
            );
            if ui.tap(cell) {
                let t = self.t(n).to_string();
                self.desk_oeffnen(ui, ic, &t, s);
                ui.dirty = true;
            }
        }
        ui.enabled = enabled_alt;

        // ---- Fenster (unten → oben)
        let n = self.desk.fenster.len();
        let oben = self.desk.fenster.iter().rposition(|f| !f.min);
        let mut schliessen = None;
        for i in 0..n {
            if self.desk.fenster[i].min {
                continue;
            }
            let aktiv = Some(i) == oben;
            if self.desk.fenster[i].max {
                self.desk.fenster[i].r = Rect::new(0.0, 0.0, w, h - lh);
            }
            let r = self.desk.fenster[i].r;
            let th = ui.dp(36.0);
            // Schatten, Rahmen, Titelleiste
            ui.cv.soft_shadow(
                r,
                ui.dp(10.0),
                rgb(0x000000).alpha(if aktiv { 0.55 } else { 0.35 }),
            );
            ui.cv.rrect(
                r,
                ui.dp(10.0),
                if aktiv {
                    ui.th.accent.alpha(0.55)
                } else {
                    rgb(0xFFFFFF).alpha(0.10)
                },
            );
            ui.cv.rrect(r.inset(1.0), ui.dp(9.0), rgb(0x14171E));
            let tr = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, th);
            ui.cv.rect(
                Rect::new(tr.x, tr.y + ui.dp(9.0), tr.w, th - ui.dp(9.0)),
                if aktiv { rgb(0x1C2029) } else { rgb(0x171A21) },
            );
            ui.cv.rrect(
                Rect::new(tr.x, tr.y, tr.w, ui.dp(20.0)),
                ui.dp(9.0),
                if aktiv { rgb(0x1C2029) } else { rgb(0x171A21) },
            );
            let ic = self.desk.fenster[i].ic;
            let titel = {
                let f = &self.desk.fenster[i];
                // Titel: Titel des aktuellen Bildschirms, wenn er nicht die Start-App ist
                f.titel.clone()
            };
            let ix = if rtl {
                tr.right() - ui.dp(22.0)
            } else {
                tr.x + ui.dp(22.0)
            };
            ui.icon_w(ic, ix, tr.cy(), ui.dp(16.0), ui.th.accent, 1.6);
            let tl = Rect::new(
                if rtl {
                    tr.x + ui.dp(130.0)
                } else {
                    tr.x + ui.dp(40.0)
                },
                tr.y,
                tr.w - ui.dp(170.0),
                th,
            );
            ui.label(
                tl,
                &titel,
                13.0,
                if aktiv { rgb(0xF2F4F8) } else { rgb(0x9AA3B2) },
                true,
                Al::Start,
            );
            // Knöpfe: schließen, maximieren, minimieren (am Ende; Arabisch links)
            let bw = ui.dp(40.0);
            let knopf = |k: usize| {
                Rect::new(
                    if rtl {
                        tr.x + k as f32 * bw
                    } else {
                        tr.right() - (k + 1) as f32 * bw
                    },
                    tr.y,
                    bw,
                    th,
                )
            };
            let (kz, km, kmin) = (knopf(0), knopf(1), knopf(2));
            let en = ui.enabled;
            ui.enabled = ziel == Ziel::Fenster(self.desk.fenster[i].id);
            for (kr, art) in [(kz, 0), (km, 1), (kmin, 2)] {
                let hover = kr.contains(ui.p.x, ui.p.y);
                if hover {
                    ui.cv.rect(
                        kr.inset(ui.dp(4.0)),
                        if art == 0 {
                            rgb(0xC0392B)
                        } else {
                            rgb(0xFFFFFF).alpha(0.10)
                        },
                    );
                }
                let c = rgb(0xDDE2EA);
                let (cx, cy, s) = (kr.cx(), kr.cy(), ui.dp(5.0));
                match art {
                    0 => {
                        ui.cv.line(cx - s, cy - s, cx + s, cy + s, ui.dp(1.4), c);
                        ui.cv.line(cx - s, cy + s, cx + s, cy - s, ui.dp(1.4), c);
                    }
                    1 => {
                        ui.cv.line(cx - s, cy - s, cx + s, cy - s, ui.dp(1.4), c);
                        ui.cv.line(cx - s, cy + s, cx + s, cy + s, ui.dp(1.4), c);
                        ui.cv.line(cx - s, cy - s, cx - s, cy + s, ui.dp(1.4), c);
                        ui.cv.line(cx + s, cy - s, cx + s, cy + s, ui.dp(1.4), c);
                    }
                    _ => ui
                        .cv
                        .line(cx - s, cy + s * 0.6, cx + s, cy + s * 0.6, ui.dp(1.4), c),
                }
                if ui.tap(kr) {
                    match art {
                        0 => schliessen = Some(self.desk.fenster[i].id),
                        1 => {
                            let f = &mut self.desk.fenster[i];
                            f.max = !f.max;
                            if !f.max {
                                f.r = Rect::new(w * 0.19, h * 0.08, w * 0.62, h * 0.74);
                            }
                        }
                        _ => {
                            self.desk.fenster[i].min = true;
                            self.focus = None;
                        }
                    }
                    ui.dirty = true;
                }
            }
            ui.enabled = en;
            // Inhalt in eigener Zeichenfläche
            let cr = Rect::new(r.x + 1.0, r.y + th + 1.0, r.w - 2.0, r.h - th - 2.0);
            let (cw, chh) = (cr.w.max(1.0) as i32, cr.h.max(1.0) as i32);
            let mut cv = match self.desk.fenster[i].cv.take() {
                Some(c) if c.w == cw && c.h == chh => c,
                _ => Canvas::new(cw, chh),
            };
            let mut p2 = if ziel == Ziel::Fenster(self.desk.fenster[i].id) {
                Ptr {
                    x: ui.p.x - cr.x,
                    y: ui.p.y - cr.y,
                    px: ui.p.px - cr.x,
                    py: ui.p.py - cr.y,
                    ..*ui.p
                }
            } else {
                Ptr {
                    x: -1e6,
                    y: -1e6,
                    px: -1e6,
                    py: -1e6,
                    ..Ptr::default()
                }
            };
            // Mausrad nur für das Fenster unter dem Zeiger
            if cr.contains(ui.p.x, ui.p.y) && aktiv {
                p2.rad = ui.p.rad;
                p2.x = ui.p.x - cr.x;
                p2.y = ui.p.y - cr.y;
            }
            let mut dirty = false;
            {
                let mut u2 = Ui {
                    cv: &mut cv,
                    tx: &mut *ui.tx,
                    p: &mut p2,
                    th: ui.th,
                    k: ui.k,
                    w: cw as f32,
                    h: chh as f32,
                    rtl,
                    enabled: true,
                    focus: self.focus,
                    dirty: false,
                    clip: None,
                    links: Vec::new(),
                    kb: Default::default(),
                    seen: None,
                };
                u2.cv.set_clip(None);
                u2.cv
                    .vgrad(Rect::new(0.0, 0.0, u2.w, u2.h), u2.th.bg, u2.th.bg2);
                std::mem::swap(&mut self.stack, &mut self.desk.fenster[i].stack);
                self.desk.innen = true;
                let fokus_vorher = self.focus;
                if let Some((scr, mut sc)) = self.stack.last().cloned() {
                    let area = Rect::new(0.0, 0.0, u2.w, u2.h);
                    self.draw_screen(&mut u2, &scr, area, &mut sc);
                    if let Some(l) = self.stack.last_mut() {
                        if l.0 == scr {
                            l.1 = sc;
                        }
                    }
                }
                self.desk.innen = false;
                std::mem::swap(&mut self.stack, &mut self.desk.fenster[i].stack);
                dirty |= u2.dirty;
                // Eingabefokus aus dem Fenster übernehmen (angetipptes Feld bekommt die Tastatur)
                if ziel == Ziel::Fenster(self.desk.fenster[i].id) {
                    if self.focus_reset {
                        self.focus_reset = false;
                    } else if u2.focus != fokus_vorher && self.focus == fokus_vorher {
                        // Feld per Antippen fokussiert (Bildschirme, die selbst self.focus setzen, bleiben unberührt)
                        self.focus = u2.focus;
                    }
                }
            }
            if ziel == Ziel::Fenster(self.desk.fenster[i].id) {
                ui.p.capture = p2.capture;
                ui.p.dy = p2.dy;
                ui.p.dx = p2.dx;
            }
            if cr.contains(ui.p.x, ui.p.y) && aktiv {
                ui.p.rad = p2.rad;
            }
            ui.dirty |= dirty;
            ui.cv.set_clip(None);
            ui.cv.blit_xrgb(
                &cv.px,
                cw as u32,
                chh as u32,
                Rect::new(0.0, 0.0, cw as f32, chh as f32),
                Rect::new(cr.x.round(), cr.y.round(), cw as f32, chh as f32),
                false,
            );
            // Griff zum Größe ändern
            let g = if rtl {
                (r.x + ui.dp(4.0), r.bottom() - ui.dp(4.0))
            } else {
                (r.right() - ui.dp(4.0), r.bottom() - ui.dp(4.0))
            };
            let d = if rtl { 1.0 } else { -1.0 };
            for k in 1..=3 {
                let o = ui.dp(4.0) * k as f32;
                ui.cv.line(
                    g.0,
                    g.1 - o,
                    g.0 + d * o,
                    g.1,
                    ui.dp(1.0),
                    rgb(0xFFFFFF).alpha(0.25),
                );
            }
            self.desk.fenster[i].cv = Some(cv);
            // leerer Verlauf (z. B. Zurück in der Start-App) → Fenster schließen
            if self.desk.fenster[i].stack.is_empty() {
                schliessen = Some(self.desk.fenster[i].id);
            }
        }
        if let Some(id) = schliessen {
            self.desk_schliessen(id);
        }

        // ---- Leiste
        let lr = Rect::new(0.0, h - lh, w, lh);
        ui.cv.rect(lr, rgb(0x0C0E13).alpha(0.94));
        ui.cv
            .rect(Rect::new(0.0, h - lh, w, 1.0), rgb(0xFFFFFF).alpha(0.08));
        let en = ui.enabled;
        ui.enabled = ziel == Ziel::Leiste;
        // Start
        let sr = Rect::new(
            fx(ui.dp(8.0), ui.dp(44.0)),
            h - lh + ui.dp(4.0),
            ui.dp(44.0),
            ui.dp(44.0),
        );
        if self.desk.start || sr.contains(ui.p.x, ui.p.y) {
            ui.cv.rrect(sr, ui.dp(10.0), rgb(0xFFFFFF).alpha(0.10));
        }
        let mark = crate::brand::mark();
        let ms = ui.dp(30.0);
        ui.cv.image(
            &mark,
            sr.cx() - ms / 2.0,
            sr.cy() - ms / 2.0 * mark.h as f32 / mark.w as f32,
            ms,
            ms * mark.h as f32 / mark.w as f32,
        );
        if ui.tap(sr) {
            self.desk.start = !self.desk.start;
            if self.desk.start {
                self.fields.insert("desk_q", String::new());
                self.focus = Some("desk_q");
            }
            ui.dirty = true;
        }
        // angeheftete Apps + offene Fenster
        let mut x = ui.dp(64.0);
        let apps = self.desk_apps();
        let mut eintraege: Vec<(Ic, String, Option<u64>, Option<Screen>)> = LEISTE
            .iter()
            .filter_map(|n| apps.iter().find(|a| a.1 == *n))
            .map(|(ic, n, s)| {
                let offen = self
                    .desk
                    .fenster
                    .iter()
                    .find(|f| f.stack.first().map(|x| &x.0) == Some(s))
                    .map(|f| f.id);
                (*ic, self.t(n).to_string(), offen, Some(s.clone()))
            })
            .collect();
        for f in &self.desk.fenster {
            if !eintraege.iter().any(|e| e.2 == Some(f.id)) {
                eintraege.push((f.ic, f.titel.clone(), Some(f.id), None));
            }
        }
        let oben_id = self
            .desk
            .fenster
            .iter()
            .rev()
            .find(|f| !f.min)
            .map(|f| f.id);
        for (ic, titel, offen, s) in eintraege {
            let br = Rect::new(
                fx(x, ui.dp(44.0)),
                h - lh + ui.dp(4.0),
                ui.dp(44.0),
                ui.dp(44.0),
            );
            if br.contains(ui.p.x, ui.p.y) {
                ui.cv.rrect(br, ui.dp(10.0), rgb(0xFFFFFF).alpha(0.08));
            }
            ui.icon_w(
                ic,
                br.cx(),
                br.cy() - ui.dp(2.0),
                ui.dp(20.0),
                if offen.is_some() {
                    rgb(0xF2F4F8)
                } else {
                    rgb(0xAAB2C0)
                },
                1.6,
            );
            if let Some(id) = offen {
                let aktiv = oben_id == Some(id);
                let bw = if aktiv { ui.dp(18.0) } else { ui.dp(6.0) };
                ui.cv.rrect(
                    Rect::new(br.cx() - bw / 2.0, br.bottom() - ui.dp(5.0), bw, ui.dp(3.0)),
                    ui.dp(1.5),
                    if aktiv { ui.th.accent } else { rgb(0x8A93A3) },
                );
            }
            if ui.tap(br) {
                match (offen, s) {
                    (Some(id), _) => {
                        if let Some(i) = self.desk.fenster.iter().position(|f| f.id == id) {
                            let mut f = self.desk.fenster.remove(i);
                            // aktives Fenster nochmal antippen = minimieren
                            f.min = oben_id == Some(id) && !f.min;
                            self.desk.fenster.push(f);
                        }
                    }
                    (None, Some(s)) => self.desk_oeffnen(ui, ic, &titel, s),
                    _ => {}
                }
                ui.dirty = true;
            }
            x += ui.dp(48.0);
        }
        // Ende der Leiste: Sprache/Tastatur, Netz, Uhr mit Datum
        let dt = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min);
        let uhr = self.clock_str(dt.h, dt.mi);
        let datum = format!("{:02}.{:02}.{}", dt.d, dt.mo, dt.y);
        let uw = ui.dp(96.0);
        let ur = Rect::new(fx(w - uw - ui.dp(8.0), uw), h - lh, uw, lh);
        ui.label(
            Rect::new(ur.x, ur.y + ui.dp(7.0), ur.w, ui.dp(20.0)),
            &uhr,
            14.0,
            rgb(0xF2F4F8),
            true,
            Al::Center,
        );
        ui.label(
            Rect::new(ur.x, ur.y + ui.dp(27.0), ur.w, ui.dp(18.0)),
            &datum,
            11.0,
            rgb(0x9AA3B2),
            false,
            Al::Center,
        );
        let kr = Rect::new(
            fx(w - uw - ui.dp(60.0), ui.dp(44.0)),
            h - lh + ui.dp(4.0),
            ui.dp(44.0),
            ui.dp(44.0),
        );
        let kb = crate::input::belegung_name();
        ui.label(kr, kb, 13.0, rgb(0xDDE2EA), true, Al::Center);
        if ui.tap(kr) {
            crate::input::belegung_weiter();
            ui.dirty = true;
        }
        let nr = Rect::new(
            fx(w - uw - ui.dp(104.0), ui.dp(44.0)),
            h - lh + ui.dp(4.0),
            ui.dp(44.0),
            ui.dp(44.0),
        );
        // Akkus angeschlossener Geräte (Maus, Tastatur, Kopfhörer) neben dem Netz
        let mut ax = w - uw - ui.dp(112.0);
        for (name, pct) in crate::sys::geraete_akkus().into_iter().take(3) {
            let t = format!("{} {pct}%", name.split_whitespace().next().unwrap_or(""));
            let tw = ui.text_w(&t, 11.0, false) + ui.dp(28.0);
            ax -= tw;
            let ar = Rect::new(fx(ax, tw), h - lh + ui.dp(12.0), tw, ui.dp(28.0));
            ui.cv.rrect(ar, ui.dp(14.0), rgb(0xFFFFFF).alpha(0.06));
            ui.icon_w(
                Ic::Bt,
                if rtl {
                    ar.right() - ui.dp(12.0)
                } else {
                    ar.x + ui.dp(12.0)
                },
                ar.cy(),
                ui.dp(12.0),
                if pct < 20 {
                    rgb(0xE5484D)
                } else {
                    rgb(0xC8CED8)
                },
                1.4,
            );
            let tr = if rtl {
                Rect::new(ar.x, ar.y, ar.w - ui.dp(22.0), ar.h)
            } else {
                Rect::new(ar.x + ui.dp(22.0), ar.y, ar.w - ui.dp(22.0), ar.h)
            };
            ui.label(tr, &t, 11.0, rgb(0xDDE2EA), false, Al::Start);
            ax -= ui.dp(6.0);
        }
        let online = crate::sys::online();
        ui.icon_w(
            Ic::Globe,
            nr.cx(),
            nr.cy(),
            ui.dp(18.0),
            if online { rgb(0xDDE2EA) } else { rgb(0x6B7382) },
            1.6,
        );
        if ui.tap(nr) {
            self.desk_oeffnen(ui, Ic::Wifi, &self.t("Network").to_string(), Screen::Net);
            ui.dirty = true;
        }
        ui.enabled = en;

        // ---- Startmenü
        if self.desk.start {
            self.desk_startmenue(ui, ziel == Ziel::Start);
        }
        // Meldung
        if let Some((s, t)) = &self.toast {
            if t.elapsed().as_secs_f32() < 3.0 {
                let s = s.clone();
                let tw = ui.text_w(&s, 13.0, false) + ui.dp(36.0);
                let r = Rect::new((w - tw) / 2.0, h - lh - ui.dp(64.0), tw, ui.dp(40.0));
                ui.cv.rrect(r, ui.dp(20.0), rgb(0x232733));
                ui.label(r, &s, 13.0, rgb(0xF2F4F8), false, Al::Center);
            }
        }
        if ziel == Ziel::Start && ui.focus != self.focus && ui.focus.is_some() {
            self.focus = ui.focus;
        }
        if ui.p.released {
            self.desk.ziel = Ziel::Nichts;
        }
    }

    fn desk_start_rect(&self, ui: &Ui) -> Rect {
        let (mw, mh) = (
            ui.dp(600.0).min(ui.w - ui.dp(16.0)),
            ui.dp(560.0).min(ui.h - ui.dp(70.0)),
        );
        let x = if ui.rtl {
            ui.w - mw - ui.dp(8.0)
        } else {
            ui.dp(8.0)
        };
        Rect::new(x, ui.h - ui.dp(52.0) - mh - ui.dp(8.0), mw, mh)
    }

    fn desk_startmenue(&mut self, ui: &mut Ui, aktiv: bool) {
        let r = self.desk_start_rect(ui);
        let en = ui.enabled;
        ui.enabled = aktiv;
        ui.cv.soft_shadow(r, ui.dp(14.0), rgb(0x000000).alpha(0.6));
        ui.cv.rrect(r, ui.dp(16.0), rgb(0xFFFFFF).alpha(0.10));
        ui.cv.rrect(r.inset(1.0), ui.dp(15.0), rgb(0x12151C));
        let m = ui.dp(18.0);
        // Suche
        let fr = Rect::new(r.x + m, r.y + m, r.w - 2.0 * m, ui.dp(44.0));
        let q = self.f("desk_q");
        ui.field(fr, "desk_q", &q, self.t("Search apps"));
        if ui.tap(fr) {
            self.focus = Some("desk_q");
        }
        let q = q.trim().to_lowercase();
        let apps: Vec<(Ic, &'static str, Screen)> = self
            .desk_apps()
            .into_iter()
            .filter(|(_, n, _)| {
                q.is_empty()
                    || self.t(n).to_lowercase().contains(&q)
                    || n.to_lowercase().contains(&q)
            })
            .collect();
        // Enter startet den ersten Treffer
        if !q.is_empty() && std::mem::take(&mut self.desk_enter) {
            if let Some((ic, n, s)) = apps.first().cloned() {
                let t = self.t(n).to_string();
                self.desk_oeffnen(ui, ic, &t, s);
                self.fields.insert("desk_q", String::new());
                self.focus = None;
                ui.enabled = en;
                return;
            }
        }
        let cols = 6usize;
        let gx = r.x + m;
        let gw = (r.w - 2.0 * m) / cols as f32;
        let rh = ui.dp(92.0);
        let liste = Rect::new(
            r.x + 1.0,
            fr.bottom() + ui.dp(10.0),
            r.w - 2.0,
            r.bottom() - ui.dp(54.0) - fr.bottom() - ui.dp(10.0),
        );
        let mut sc = self.desk_start_sc;
        ui.scroll_begin(liste, &mut sc);
        let y0 = liste.y + ui.dp(4.0) - sc;
        let zeilen = apps.len().div_ceil(cols);
        for (i, (ic, n, s)) in apps.into_iter().enumerate() {
            let col = if ui.rtl {
                cols - 1 - i % cols
            } else {
                i % cols
            };
            let cell = Rect::new(gx + col as f32 * gw, y0 + (i / cols) as f32 * rh, gw, rh);
            if !ui.visible(cell) {
                continue;
            }
            if cell.contains(ui.p.x, ui.p.y) {
                ui.cv.rrect(
                    cell.inset(ui.dp(3.0)),
                    ui.dp(10.0),
                    rgb(0xFFFFFF).alpha(0.06),
                );
            }
            let ir = Rect::new(
                cell.cx() - ui.dp(22.0),
                cell.y + ui.dp(10.0),
                ui.dp(44.0),
                ui.dp(44.0),
            );
            ui.cv
                .rrect_vgrad(ir, ui.dp(12.0), rgb(0x232733), rgb(0x171A22));
            let fg = if n == "KM helper" || n == "Ultar Store" {
                ui.th.accent
            } else {
                rgb(0xE8ECF4)
            };
            ui.icon_w(ic, ir.cx(), ir.cy(), ui.dp(21.0), fg, 1.6);
            let lab = ui.fit(self.t(n), 11.5, false, cell.w - ui.dp(6.0));
            ui.label(
                Rect::new(cell.x, ir.bottom() + ui.dp(5.0), cell.w, ui.dp(18.0)),
                &lab,
                11.5,
                rgb(0xDDE2EA),
                false,
                Al::Center,
            );
            if ui.tap(cell) {
                let t = self.t(n).to_string();
                self.desk_oeffnen(ui, ic, &t, s);
                ui.dirty = true;
            }
        }
        ui.scroll_end(liste, zeilen as f32 * rh + ui.dp(8.0), &mut sc);
        self.desk_start_sc = sc;
        ui.set_clip(None);
        // Fußzeile: Name des Systems, Ein/Aus
        let fy = r.bottom() - ui.dp(52.0);
        ui.cv.rect(
            Rect::new(r.x + 1.0, fy, r.w - 2.0, 1.0),
            rgb(0xFFFFFF).alpha(0.07),
        );
        let nr = Rect::new(r.x + m, fy, r.w - 2.0 * m - ui.dp(48.0), ui.dp(52.0));
        let host: String = self.set.hostname.chars().take(24).collect();
        let name = if host.is_empty() {
            "Ultar OS".to_string()
        } else {
            format!("Ultar OS · {host}")
        };
        ui.label(
            if ui.rtl {
                Rect::new(r.x + m + ui.dp(48.0), fy, nr.w, nr.h)
            } else {
                nr
            },
            &name,
            13.0,
            rgb(0xC8CED8),
            false,
            Al::Start,
        );
        let pr = Rect::new(
            if ui.rtl {
                r.x + m
            } else {
                r.right() - m - ui.dp(44.0)
            },
            fy + ui.dp(4.0),
            ui.dp(44.0),
            ui.dp(44.0),
        );
        if pr.contains(ui.p.x, ui.p.y) {
            ui.cv.rrect(pr, ui.dp(10.0), rgb(0xFFFFFF).alpha(0.08));
        }
        ui.icon_w(Ic::Power, pr.cx(), pr.cy(), ui.dp(18.0), rgb(0xE8ECF4), 1.6);
        if ui.tap(pr) {
            self.desk.start = false;
            self.power_dialog = true;
            ui.dirty = true;
        }
        ui.enabled = en;
    }
}

/// Farbe des Mauszeigers (Pfeil) – zeichnet main.rs über alles
pub fn zeiger(cv: &mut Canvas, x: f32, y: f32, s: f32) {
    cv.set_clip(None);
    let pts = [
        (0.0, 0.0),
        (0.0, 17.0),
        (4.2, 13.2),
        (7.0, 19.5),
        (9.6, 18.4),
        (6.9, 12.2),
        (12.5, 12.2),
    ];
    let c_rand: Color = rgb(0x000000);
    let c_innen: Color = rgb(0xFFFFFF);
    // gefüllter Pfeil über ein Abstandsfeld (Punkt-in-Polygon)
    let bb = Rect::new(x - s, y - s, 15.0 * s, 22.0 * s);
    let inside = |px: f32, py: f32, d: f32| -> bool {
        let (u, v) = ((px - x) / s, (py - y) / s);
        let mut c = false;
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + n - 1) % n]);
            if (a.1 > v) != (b.1 > v) && u < (b.0 - a.0) * (v - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
        }
        let _ = d;
        c
    };
    for py in bb.y as i32..bb.bottom() as i32 {
        for px in bb.x as i32..bb.right() as i32 {
            let (fxp, fyp) = (px as f32 + 0.5, py as f32 + 0.5);
            if inside(fxp, fyp, 0.0) {
                let rand = !inside(fxp - s, fyp, 0.0)
                    || !inside(fxp + s, fyp, 0.0)
                    || !inside(fxp, fyp - s, 0.0)
                    || !inside(fxp, fyp + s, 0.0);
                cv.point(px, py, if rand { c_rand } else { c_innen }, 255);
            }
        }
    }
}
