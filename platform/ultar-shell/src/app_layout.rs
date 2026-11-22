//! Startbildschirm, App-Übersicht und Schnelleinstellungen nach dem Designkonzept
//! („Ultar OS · Mobilfunk, SIM & Funkturm“):
//! - Start: Markenzeile, große Uhr (Anfangsseite), Verbindungskarte (Netz, Download/Upload/Signal,
//!   „Turm wählen“ / „SIM & Netz“), darunter Glas-Kacheln der wichtigsten Apps + „Alle Apps“.
//! - Schnelleinstellungen: großer Akku, Verbindungs-Cluster (2×2 große Kacheln mit Zustand + Netz-Leiste),
//!   vier kleine Schalter, Helligkeit, Benachrichtigungen als Karten.
//! Kleine oder quer liegende Bildschirme behalten den klassischen Start (Uhr + volles Raster).

use super::app_radio::{bars, rat_short, BLUE, BLUE_L};
use super::{App, Screen};
use crate::brand;
use crate::gfx::{rgb, Color, Rect};
use crate::i18n::DAYS;
use crate::sys;
use crate::ui::{Al, Ic, Ui, ACCENTS};
use crate::{privacy, quick};
use std::time::Instant;
use ultar_radio as radio;

/// Reihenfolge der Apps auf dem Startbildschirm (nach den gelernten Lieblings-Apps).
const DOCK: [&str; 12] = ["Phone", "Messages", "Browser", "Camera", "Telegram", "WhatsApp", "Files", "Monitor", "Settings", "Clock", "Gallery", "Calculator"];

/// Aktuelle Down-/Upload-Rate aller Schnittstellen (außer lo) in Byte/s.
#[derive(Default)]
pub struct Rates {
    prev: Option<(Instant, u64, u64)>,
    last: Option<(f32, f32)>,
    /// Prozessorlast für das Geräte-Widget: (Zeitpunkt, gesamt, Leerlauf), letzter Wert 0..1
    cpu_prev: Option<(Instant, u64, u64)>,
    cpu_last: f32,
}

impl Rates {
    /// Prozessorlast aller Kerne (0..1), höchstens einmal pro Sekunde neu aus /proc/stat
    pub fn cpu(&mut self) -> f32 {
        if self.cpu_prev.is_some_and(|p| p.0.elapsed().as_millis() < 1000) {
            return self.cpu_last;
        }
        let st = std::fs::read_to_string("/proc/stat").unwrap_or_default();
        if let Some(&(tot, idle)) = super::monitor::parse_stat(&st).first() {
            if let Some((_, t0, i0)) = self.cpu_prev {
                let (dt, di) = (tot.saturating_sub(t0), idle.saturating_sub(i0));
                if dt > 0 {
                    self.cpu_last = (1.0f32 - di as f32 / dt as f32).clamp(0.0, 1.0);
                }
            }
            self.cpu_prev = Some((Instant::now(), tot, idle));
        }
        self.cpu_last
    }
}

/// Zähler (rote Plakette) je App aus den Benachrichtigungen: Anrufe → Telefon, SMS/Nachrichten → Nachrichten
pub fn badge_counts() -> (usize, usize) {
    let (mut calls, mut msgs) = (0, 0);
    for n in sys::notes_all() {
        match n.kind.as_str() {
            "call" => calls += 1,
            "sms" | "message" => msgs += 1,
            _ => {}
        }
    }
    (calls.min(99), msgs.min(99))
}

/// Symbolfarbe einer App wie in der Demo (APPS[..].c): Telefon/WhatsApp/Spotify grün, Nachrichten/Telegram blau,
/// YouTube rot, Store/Helfer gold, alles andere in Schriftfarbe.
pub fn app_color(ui: &Ui, ic: Ic) -> crate::gfx::Color {
    match ic {
        Ic::Phone | Ic::Svg("wa") | Ic::Svg("spot") => rgb(0x4ADE80),
        Ic::Chat | Ic::Svg("tg") => rgb(0x7AB8FF),
        Ic::Svg("yt") => rgb(0xFF6B6B),
        Ic::Svg("store") | Ic::Spark => rgb(0xD8C870),
        _ => ui.th.text,
    }
}

/// Rote Plakette mit Zahl oben an der „End"-Ecke eines Symbols (rechts, in RTL links)
pub fn badge(ui: &mut Ui, ir: Rect, n: usize) {
    if n == 0 {
        return;
    }
    let txt = n.to_string();
    let h = ui.dp(20.0);
    let w = (ui.text_w(&txt, 11.0, true) + ui.dp(12.0)).max(h);
    let x = if ui.rtl { ir.x - ui.dp(5.0) } else { ir.right() + ui.dp(5.0) - w };
    let r = Rect::new(x, ir.y - ui.dp(5.0), w, h);
    ui.cv.rrect(r.inset(-1.5), h / 2.0 + 1.5, ui.th.bg);
    ui.cv.rrect(r, h / 2.0, rgb(crate::ui::KM_RED));
    ui.label(r, &txt, 11.0, rgb(0xFFFFFF), true, Al::Center);
}

impl Rates {
    pub fn sample(&mut self) -> Option<(f32, f32)> {
        let txt = std::fs::read_to_string("/proc/net/dev").ok()?;
        let (mut rx, mut tx) = (0u64, 0u64);
        for (n, (r, t)) in super::monitor::parse_netdev(&txt) {
            if n != "lo" {
                rx += r;
                tx += t;
            }
        }
        let now = Instant::now();
        if let Some((t0, r0, x0)) = self.prev {
            let dt = now.duration_since(t0).as_secs_f32();
            if dt < 0.5 {
                return self.last;
            }
            self.last = Some((rx.saturating_sub(r0) as f32 / dt, tx.saturating_sub(x0) as f32 / dt));
        }
        self.prev = Some((now, rx, tx));
        self.last
    }
}

/// Rate lesbar: „48 Mbit/s“, „320 kbit/s“.
fn rate(bps: f32) -> (String, &'static str) {
    let bits = bps * 8.0;
    if bits >= 1_000_000.0 {
        let v = bits / 1_000_000.0;
        (if v >= 10.0 { format!("{v:.0}") } else { format!("{v:.1}").replace('.', ",") }, "Mbit/s")
    } else {
        (format!("{:.0}", bits / 1000.0), "kbit/s")
    }
}

fn mir(ui: &Ui, r: Rect) -> Rect {
    if ui.rtl {
        Rect::new(ui.w - r.x - r.w, r.y, r.w, r.h)
    } else {
        r
    }
}

impl App {
    /// Glas-Kachel mit Symbol und Beschriftung; true bei Antippen.
    pub(super) fn app_tile(&mut self, ui: &mut Ui, cell: Rect, ic: Ic, name: &str, hue: Color, s: f32) -> bool {
        // Glas-Kachel im „Ultar Noir"-Stil: neutrale Fläche, feine Lichtkante, Symbol in Schriftfarbe;
        // nur die Helfer-/Store-Kacheln tragen Gold (hue = Akzent)
        let ir = Rect::new(cell.cx() - s / 2.0, cell.y + ui.dp(4.0), s, s);
        let held = ui.held(cell);
        let rad = s * 0.32;
        let _ = hue;
        // wie .ic in der Demo: Verlauf #1E2129 → #15171C, feine helle Kante, kein Schatten
        ui.cv.rrect(ir, rad, rgb(0xFFFFFF).alpha(0.09));
        let (t, b) = if held { (rgb(0x2A2E38), rgb(0x1E2128)) } else { (rgb(0x1E2129), rgb(0x15171C)) };
        ui.cv.rrect_vgrad(ir.inset(1.0), rad - 1.0, t, b);
        let fg = app_color(ui, ic);
        ui.icon_w(ic, ir.cx(), ir.cy(), s * 0.45, fg, 1.6);
        let lab = ui.fit(name, 12.0, false, cell.w - ui.dp(4.0));
        ui.label(Rect::new(cell.x, ir.bottom() + ui.dp(6.0), cell.w, ui.dp(18.0)), &lab, 12.0, ui.th.sub.mix(ui.th.text, 0.45), false, Al::Center);
        ui.tap(cell)
    }

    fn launch(&mut self, ui: &mut Ui, name: &'static str, target: Option<Screen>) {
        match target {
            Some(s) => {
                self.count_launch(name);
                self.go(s)
            }
            None => self.power_dialog = true,
        }
        ui.dirty = true;
    }

    // ------------------------------------------------------------ Startbildschirm

    pub(super) fn s_home(&mut self, ui: &mut Ui, area: Rect) {
        let wide = ui.w > ui.h;
        if wide || area.h < ui.dp(600.0) {
            return self.s_home_classic(ui, area);
        }
        let dt = sys::from_unix(sys::unix_now(), self.set.tz_min);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(20.0);
        let mut y = area.y + ui.dp(8.0);

        // ---- Kopf (wie die Web-Demo): Uhr als Punktmatrix links, Logo rechts ----
        let clock = self.clock_str(dt.h, dt.mi);
        // Uhr passt immer neben das Logo (auch im 12-Stunden-Format „9:18 PM")
        let lg0 = brand::logo();
        let lw0 = ui.dp(38.0) * lg0.w as f32 / lg0.h.max(1) as f32;
        let avail = ui.w - 2.0 * m - lw0 - ui.dp(12.0);
        let mut csz = 76.0;
        while csz > 36.0 && ui.text_w(&clock, csz, true) > avail {
            csz -= 2.0;
        }
        let cw = ui.text_w(&clock, csz, true) + ui.dp(4.0);
        let ch = ui.dp(84.0);
        y += ui.dp(10.0);
        let cr = Rect::new(if ui.rtl { ui.w - m - cw } else { m }, y, cw, ch);
        ui.label(cr, &clock, csz, c, true, Al::Start);
        let (bg, bg2, hh) = (ui.th.bg, ui.th.bg2, ui.h);
        ui.cv.dot_screen(cr, ui.dp(4.4).max(3.0), |yy| bg.mix(bg2, (yy / hh).clamp(0.0, 1.0)));
        let lg = brand::logo();
        let lh = ui.dp(38.0);
        let lw = lh * lg.w as f32 / lg.h as f32;
        let lx = if ui.rtl { m } else { ui.w - m - lw };
        ui.cv.image(&lg, lx, y + ui.dp(6.0), lw, lh);
        y += ch;
        // Datum · Hijri-Datum
        let (hy, hm, hd) = crate::sysy::hijri(dt.y, dt.mo, dt.d);
        let hmon = if self.set.lang == "ar" { crate::sysy::HIJRI_MONTHS_AR } else { crate::sysy::HIJRI_MONTHS }[(hm - 1).clamp(0, 11) as usize];
        let hijri = format!("{hd} {hmon} {hy}");
        let date = format!("{}, {:02}.{:02}. · {hijri}", self.t(DAYS[dt.wd]), dt.d, dt.mo);
        let df = ui.fit(&date, 14.5, false, ui.w - 2.0 * m);
        ui.label(Rect::new(m, y, ui.w - 2.0 * m, ui.dp(22.0)), &df, 14.5, cs, false, Al::Start);
        y += ui.dp(36.0);

        // ---- Zwei Widgets: Gerät (Prozessor, Speicher, Akku) und Als Nächstes ----
        let gap = ui.dp(12.0);
        let ww = (ui.w - 2.0 * ui.dp(18.0) - gap) / 2.0;
        let wh = ui.dp(104.0);
        let w1 = mir(ui, Rect::new(ui.dp(18.0), y, ww, wh));
        let w2 = mir(ui, Rect::new(ui.dp(18.0) + ww + gap, y, ww, wh));
        for wr in [w1, w2] {
            let held = ui.held(wr);
            ui.cv.rrect(wr, ui.dp(22.0), ui.th.line);
            ui.cv.rrect(wr.inset(1.0), ui.dp(21.0), if held { ui.th.press } else { ui.th.card });
        }
        let pad = ui.dp(14.0);
        ui.label(Rect::new(w1.x + pad, w1.y + ui.dp(12.0), w1.w - 2.0 * pad, ui.dp(16.0)), self.t("Device"), 12.0, cs, false, Al::Start);
        let cpu = self.layout_rates.cpu();
        let (mt, ma) = sys::meminfo();
        let ram = if mt > 0 { 1.0 - ma as f32 / mt as f32 } else { 0.0 };
        let bat = sys::battery_info().map(|b| b.percent as f32 / 100.0).or_else(|| sys::battery().map(|b| b.0 as f32 / 100.0));
        let rows: [(&str, Option<f32>, Color); 3] = [(self.t("CPU"), Some(cpu), ui.th.accent), ("RAM", Some(ram), ui.th.accent), (self.t("Battery"), bat, ui.th.ok)];
        for (i, (lab, v, col)) in rows.iter().enumerate() {
            let ry = w1.y + ui.dp(38.0) + i as f32 * ui.dp(20.0);
            let lw2 = ui.dp(46.0);
            let vw = ui.dp(34.0);
            let (lx2, bx, vx) = if ui.rtl { (w1.right() - pad - lw2, w1.x + pad + vw, w1.x + pad) } else { (w1.x + pad, w1.x + pad + lw2, w1.right() - pad - vw) };
            let lf = ui.fit(lab, 11.5, false, lw2 - ui.dp(2.0));
            ui.label(Rect::new(lx2, ry - ui.dp(8.0), lw2, ui.dp(16.0)), &lf, 11.5, c.mix(cs, 0.25), false, Al::Start);
            let bw = w1.w - 2.0 * pad - lw2 - vw - ui.dp(4.0);
            let br = Rect::new(bx, ry - ui.dp(3.0), bw, ui.dp(6.0));
            ui.cv.rrect(br, ui.dp(3.0), ui.th.line);
            if let Some(v) = v {
                let fw = (bw * v).max(ui.dp(6.0));
                let fr = if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) };
                ui.cv.rrect(fr, ui.dp(3.0), *col);
            }
            let vt = v.map(|v| format!("{}%", (v * 100.0).round())).unwrap_or_else(|| "—".into());
            ui.label(Rect::new(vx, ry - ui.dp(8.0), vw, ui.dp(16.0)), &vt, 11.5, c, false, Al::End);
        }
        if ui.tap(w1) {
            self.go(Screen::Monitor);
            ui.dirty = true;
        }
        // Als Nächstes: Fokus, Erinnerung oder Wecker (sonst Hinweis)
        ui.label(Rect::new(w2.x + pad, w2.y + ui.dp(12.0), w2.w - 2.0 * pad, ui.dp(16.0)), self.t("Up next"), 12.0, cs, false, Al::Start);
        let next = self.glance().filter(|g| g.1 != hijri);
        let (big, small) = match &next {
            Some((_, t)) => match t.split_once(" · ") {
                Some((a, b2)) => (b2.to_string(), a.to_string()),
                None => (t.clone(), String::new()),
            },
            None => (self.t("Nothing planned").to_string(), self.t("Calendar").to_string()),
        };
        let inner = w2.w - 2.0 * pad;
        let bf = ui.fit(&big, 16.5, true, inner);
        ui.label(Rect::new(w2.x + pad, w2.bottom() - ui.dp(52.0), inner, ui.dp(22.0)), &bf, 16.5, c, true, Al::Start);
        let sf = ui.fit(&small, 12.5, false, inner);
        ui.label(Rect::new(w2.x + pad, w2.bottom() - ui.dp(28.0), inner, ui.dp(18.0)), &sf, 12.5, cs, false, Al::Start);
        if ui.tap(w2) {
            self.go(Screen::Sys(super::app_sys::Sx::Calendar));
            ui.dirty = true;
        }
        y += wh + ui.dp(14.0);

        // ---- Suchleiste mit Mikrofon (Mikrofon ruft den Helfer) ----
        let sr = Rect::new(m - ui.dp(2.0), y, ui.w - 2.0 * m + ui.dp(4.0), ui.dp(50.0));
        let held = ui.held(sr);
        ui.cv.rrect(sr, ui.dp(25.0), ui.th.line);
        ui.cv.rrect(sr.inset(1.0), ui.dp(24.0), if held { ui.th.press } else { ui.th.card });
        let ix = if ui.rtl { sr.right() - ui.dp(24.0) } else { sr.x + ui.dp(24.0) };
        ui.icon(Ic::Search, ix, sr.cy(), ui.dp(16.0), cs);
        let mx = if ui.rtl { sr.x + ui.dp(25.0) } else { sr.right() - ui.dp(25.0) };
        let st = self.t("Search apps, files, settings");
        let stf = ui.fit(st, 14.5, false, sr.w - ui.dp(100.0));
        ui.label(Rect::new(sr.x + ui.dp(46.0), sr.y, sr.w - ui.dp(92.0), sr.h), &stf, 14.5, cs, false, Al::Start);
        let mic = ui.icon_btn(mx, sr.cy(), ui.dp(19.0), Ic::Mic, ui.th.accent.alpha(0.18), ui.th.accent);
        if mic {
            self.buddy.open = true;
            ui.dirty = true;
        } else if ui.tap(sr) {
            self.go(Screen::Sys(super::app_sys::Sx::Search));
            self.focus = Some("sx_q");
            self.focus_reset = true;
            ui.dirty = true;
        }
        y = sr.bottom() + ui.dp(18.0);

        // ---- Dock unten: die vier wichtigsten Apps auf einer Glasleiste ----
        let all = self.all_apps();
        let dock_names = ["Phone", "Messages", "Browser", "Camera"];
        let badges = badge_counts();
        let dock_h = ui.dp(80.0);
        let dw = (ui.w - ui.dp(24.0)).min(ui.dp(440.0));
        let dr = Rect::new((ui.w - dw) / 2.0, area.bottom() - dock_h - ui.dp(6.0), dw, dock_h);
        ui.cv.rrect(dr, ui.dp(30.0), ui.th.line);
        ui.cv.rrect(dr.inset(1.0), ui.dp(29.0), ui.th.card.alpha(0.94));
        let ds = ui.dp(56.0);
        for (i, name) in dock_names.iter().enumerate() {
            let col = if ui.rtl { 3 - i } else { i };
            let cx = dr.x + dr.w * (col as f32 + 0.5) / 4.0;
            let hit = Rect::new(cx - dr.w / 8.0, dr.y, dr.w / 4.0, dr.h);
            let ir = Rect::new(cx - ds / 2.0, dr.cy() - ds / 2.0, ds, ds);
            let Some((ic, key, target)) = all.iter().find(|a| a.1 == *name).cloned() else { continue };
            let phone = *name == "Phone";
            let held = ui.held(hit);
            let rad = ds * 0.33;
            if phone {
                ui.cv.rrect(ir, rad, if held { rgb(0x10291A) } else { rgb(0x173A24) });
            } else {
                ui.cv.rrect(ir, rad, rgb(0xFFFFFF).alpha(0.09));
                let (t, b) = if held { (rgb(0x2A2E38), rgb(0x1E2128)) } else { (rgb(0x1E2129), rgb(0x15171C)) };
                ui.cv.rrect_vgrad(ir.inset(1.0), rad - 1.0, t, b);
            }
            let _ = c;
            ui.icon_w(ic, ir.cx(), ir.cy(), ds * 0.46, app_color(ui, ic), 1.6);
            badge(ui, ir, if phone { badges.0 } else if *name == "Messages" { badges.1 } else { 0 });
            if ui.tap(hit) {
                self.launch(ui, key, target);
            }
        }

        // ---- App-Raster als Seiten (wischen oder Punkte antippen) – endet sauber über dem Dock ----
        let dots_h = ui.dp(22.0);
        let gr = Rect::new(0.0, y, ui.w, (dr.y - dots_h - ui.dp(4.0) - y).max(ui.dp(90.0)));
        let cols = 4usize;
        let gm = ui.dp(12.0);
        let cwid = (ui.w - 2.0 * gm) / cols as f32;
        let row_h = ui.dp(84.0);
        let rows = ((gr.h / row_h).floor() as usize).max(1);
        let row_h = gr.h / rows as f32;
        let per = rows * cols;
        // Reihenfolge wie in der Demo (HOME_GRID), danach alle übrigen Apps
        const GRID: [&str; 17] = ["Settings", "Gallery", "Files", "Clock", "Calendar", "Calculator", "Notes", "Contacts", "Tools", "Telegram", "WhatsApp", "YouTube", "Ultar Store", "Monitor", "Terminal", "KM helper", "Privacy"];
        let mut order: Vec<&'static str> = GRID.iter().filter_map(|g| all.iter().find(|a| a.1 == *g).map(|a| a.1)).collect();
        for a in &all {
            if a.2.is_some() && !order.contains(&a.1) {
                order.push(a.1);
            }
        }
        order.retain(|n| !dock_names.contains(n));
        let pages = order.len().div_ceil(per).max(1);
        let (mut page, mut pos) = self.home_pg;
        page = page.min(pages - 1);
        // Wischen: Finger zieht die Seiten mit, beim Loslassen rastet die nächste Seite ein
        let p = (ui.p.x, ui.p.y, ui.p.px, ui.p.py, ui.p.down, ui.p.released, ui.p.moved);
        let dir = if ui.rtl { -1.0 } else { 1.0 };
        let horiz = (p.0 - p.2).abs() > (p.1 - p.3).abs() * 1.2;
        let mut drag = 0.0;
        if ui.enabled && gr.contains(p.2, p.3) && p.6 && horiz {
            if p.4 {
                drag = (p.0 - p.2) / gr.w * dir;
                ui.dirty = true;
            } else if p.5 {
                let d = (p.0 - p.2) * dir;
                if d < -ui.dp(48.0) && page + 1 < pages {
                    page += 1;
                } else if d > ui.dp(48.0) && page > 0 {
                    page -= 1;
                }
                pos -= (p.0 - p.2) / gr.w * dir;
            }
        }
        if !p.4 {
            let k = 0.32;
            pos += (page as f32 - pos) * k;
            if (page as f32 - pos).abs() < 0.004 {
                pos = page as f32;
            } else {
                ui.dirty = true;
            }
        }
        self.home_pg = (page, pos);
        let shown = pos - drag;
        ui.set_clip(Some(gr));
        let s = ui.dp(56.0);
        let mut go: Option<(&'static str, Option<Screen>)> = None;
        for pg in 0..pages {
            let off = (pg as f32 - shown) * gr.w * dir;
            if off.abs() >= gr.w {
                continue;
            }
            for k in 0..per {
                let Some(name) = order.get(pg * per + k).copied() else { break };
                let Some((ic, key, target)) = all.iter().find(|a| a.1 == name).cloned() else { continue };
                let col = if ui.rtl { cols - 1 - k % cols } else { k % cols };
                let cell = Rect::new(gm + col as f32 * cwid + off, gr.y + (k / cols) as f32 * row_h, cwid, row_h);
                let lab = self.t(key);
                if self.app_tile(ui, cell, ic, lab, ui.th.accent, s) && drag == 0.0 {
                    go = Some((key, target));
                }
            }
        }
        ui.set_clip(None);
        if let Some((key, target)) = go {
            self.launch(ui, key, target);
        }
        // Seitenpunkte
        if pages > 1 {
            let dy = dr.y - dots_h / 2.0 - ui.dp(2.0);
            let gap = ui.dp(14.0);
            let x0 = ui.w / 2.0 - (pages as f32 - 1.0) * gap / 2.0;
            for i in 0..pages {
                let ix = if ui.rtl { pages - 1 - i } else { i };
                let cx = x0 + ix as f32 * gap;
                let on = i == page;
                if on {
                    ui.cv.rrect(Rect::new(cx - ui.dp(8.0), dy - ui.dp(3.0), ui.dp(16.0), ui.dp(6.0)), ui.dp(3.0), ui.th.accent);
                } else {
                    ui.cv.circle(cx, dy, ui.dp(3.0), cs.alpha(0.5));
                }
                if ui.tap(Rect::new(cx - gap / 2.0, dy - ui.dp(14.0), gap, ui.dp(28.0))) {
                    self.home_pg.0 = i;
                    ui.dirty = true;
                }
            }
        }
    }

    /// Verbindungskarte: Mobilfunk (bevorzugt) oder WLAN, Live-Rate, Signal, zwei Aktionen.
    fn conn_card(&mut self, ui: &mut Ui, r: Rect) {
        let st = self.radio();
        let p = self.rprefs();
        let blue = rgb(BLUE);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let rad = ui.dp(22.0);
        ui.cv.soft_shadow(r, rad, rgb(0));
        ui.cv.rrect(r, rad, ui.th.line);
        ui.cv.rrect(r.inset(1.0), rad, ui.th.card);
        ui.cv.rrect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, ui.dp(40.0)), rad, ui.th.text.alpha(0.025));
        let pad = ui.dp(16.0);
        let inn = r.inset(pad);
        let mobile = st.in_service();
        let wifi = sys::online() && quick::wifi_on();

        // Kopf
        let ib = ui.dp(36.0);
        let ir = if ui.rtl { Rect::new(inn.right() - ib, inn.y, ib, ib) } else { Rect::new(inn.x, inn.y, ib, ib) };
        ui.cv.rrect(ir, ui.dp(12.0), blue.alpha(if mobile || wifi { 0.16 } else { 0.06 }));
        let icol = if mobile || wifi { rgb(BLUE_L) } else { cs };
        ui.icon(if mobile || !wifi { Ic::Tower } else { Ic::Wifi }, ir.cx(), ir.cy(), ui.dp(19.0), icol);
        let mode = self.mode_label(p.mode);
        let mw = if mobile { ui.text_w(mode, 11.0, true) + ui.dp(18.0) } else { 0.0 };
        if mobile {
            let mr = if ui.rtl { Rect::new(inn.x, inn.y + ui.dp(7.0), mw, ui.dp(22.0)) } else { Rect::new(inn.right() - mw, inn.y + ui.dp(7.0), mw, ui.dp(22.0)) };
            ui.cv.rrect(mr, ui.dp(11.0), ui.th.accent);
            let oa = ui.th.on_accent;
            ui.label(mr, mode, 11.0, oa, true, Al::Center);
        }
        let tx = ib + ui.dp(12.0);
        let tw = inn.w - tx - mw - ui.dp(8.0);
        let trx = if ui.rtl { inn.x + mw + ui.dp(8.0) } else { inn.x + tx };
        let (title, sub) = if mobile {
            let op = if st.operator.is_empty() { st.plmn.clone() } else { st.operator.clone() };
            let n = st.cells.len();
            let sub = match st.serving_cell() {
                Some(cl) if !cl.band.is_empty() => {
                    let mut s = format!("{} {}", self.t("Cell"), cl.id);
                    if n > 1 {
                        s += &format!(" {} {n}", self.t("of"));
                    }
                    s += &format!(" · {} · {} MHz", cl.band, cl.mhz);
                    s
                }
                _ => rat_short(&st.rat).to_string(),
            };
            (format!("{op} · SIM {}", p.data_sim.max(1)), sub)
        } else if wifi {
            let ssid = sys::wifi_ssid().unwrap_or_default();
            let ip = sys::net_status().into_iter().find(|n| n.wifi && n.state == "verbunden").map(|n| n.ip).unwrap_or_default();
            (if ssid.is_empty() { self.t("Wi-Fi").to_string() } else { format!("{} · {ssid}", self.t("Wi-Fi")) }, ip)
        } else {
            (self.t("No connection").to_string(), if st.note.is_empty() { self.t("Neither Wi-Fi nor mobile network").to_string() } else { st.note.clone() })
        };
        let tf = ui.fit(&title, 15.0, true, tw);
        ui.label(Rect::new(trx, inn.y, tw, ui.dp(20.0)), &tf, 15.0, c, true, Al::Start);
        let sf = ui.fit(&sub, 12.0, false, tw);
        ui.label(Rect::new(trx, inn.y + ui.dp(20.0), tw, ui.dp(16.0)), &sf, 12.0, cs, false, Al::Start);

        // Kennzahlen: Download, Upload, Signal
        let ky = inn.y + ib + ui.dp(12.0);
        let gap = ui.dp(8.0);
        let kw = (inn.w - 2.0 * gap) / 3.0;
        let kh = ui.dp(54.0);
        let rates = self.layout_rates.sample();
        let (d, u) = rates.map(|(d, u)| (rate(d), rate(u))).unwrap_or((("—".into(), ""), ("—".into(), "")));
        let sig = if mobile {
            let l = radio::level(st.serving_cell().and_then(|c| c.rsrp));
            ([self.t("None"), self.t("Weak"), self.t("Fair"), self.t("Good"), self.t("Excellent")][l as usize].to_string(), "", rgb(BLUE_L))
        } else if wifi {
            (self.t("Wi-Fi").to_string(), "", rgb(BLUE_L))
        } else {
            ("—".to_string(), "", cs)
        };
        let ks: [(&str, String, &str, Color); 3] = [(self.t("Download"), d.0, d.1, c), (self.t("Upload"), u.0, u.1, c), (self.t("Signal"), sig.0, sig.1, sig.2)];
        for (i, (k, v, unit, vc)) in ks.iter().enumerate() {
            let kr = mir(ui, Rect::new(inn.x + i as f32 * (kw + gap), ky, kw, kh));
            ui.cv.rrect(kr, ui.dp(14.0), ui.th.text.alpha(0.045));
            let kin = kr.inset(ui.dp(10.0));
            ui.label(Rect::new(kin.x, kin.y, kin.w, ui.dp(13.0)), k, 10.5, cs, false, Al::Start);
            let vw = ui.text_w(v, 19.0, true);
            let vf = ui.fit(v, 19.0, true, kin.w);
            ui.label(Rect::new(kin.x, kin.y + ui.dp(15.0), kin.w, ui.dp(22.0)), &vf, 19.0, *vc, true, Al::Start);
            if !unit.is_empty() && vw + ui.dp(4.0) + ui.text_w(unit, 10.0, false) <= kin.w {
                let ux = if ui.rtl { kin.right() - vw - ui.dp(4.0) - ui.text_w(unit, 10.0, false) } else { kin.x + vw + ui.dp(4.0) };
                let old = ui.rtl;
                ui.rtl = false;
                ui.label(Rect::new(ux, kin.y + ui.dp(19.0), kin.w, ui.dp(16.0)), unit, 10.0, cs, false, Al::Start);
                ui.rtl = old;
            }
            if i == 2 && mobile {
                let l = radio::level(st.serving_cell().and_then(|c| c.rsrp));
                let bx = if ui.rtl { kin.x } else { kin.right() - ui.dp(16.0) };
                bars(ui, bx, kin.y + ui.dp(11.0), ui.dp(11.0), l, blue, ui.th.line);
            }
        }

        // Aktionen
        let by = ky + kh + ui.dp(10.0);
        let bw = (inn.w - gap) / 2.0;
        let b1 = mir(ui, Rect::new(inn.x, by, bw, ui.dp(44.0)));
        let b2 = mir(ui, Rect::new(inn.x + bw + gap, by, bw, ui.dp(44.0)));
        if ui.button(b1, self.t("Choose tower"), crate::ui::Btn::Primary) {
            self.go(Screen::Cells);
            ui.dirty = true;
        }
        if ui.button(b2, self.t("SIM & network"), crate::ui::Btn::Plain) {
            self.go(Screen::Radio);
            ui.dirty = true;
        }
    }

    // ------------------------------------------------------------ Alle Apps

    pub(super) fn s_all_apps(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("All apps"));
        ui.scroll_begin(area, sc);
        let y0 = area.y - *sc + ui.dp(4.0);
        let gm = ui.dp(12.0);
        let cols = (((ui.w - 2.0 * gm) / ui.dp(88.0)) as usize).clamp(4, 8);
        let cw = (ui.w - 2.0 * gm) / cols as f32;
        let row_h = ui.dp(104.0);
        let all = self.all_apps();
        let n = all.len();
        let s = ui.dp(56.0);
        for (i, (ic, key, target)) in all.into_iter().enumerate() {
            let col = if ui.rtl { cols - 1 - i % cols } else { i % cols };
            let cell = Rect::new(gm + col as f32 * cw, y0 + (i / cols) as f32 * row_h, cw, row_h);
            if !ui.visible(cell) {
                continue;
            }
            let hue = if i == 0 { ui.th.accent } else { rgb(ACCENTS[(self.set.accent + i) % ACCENTS.len()]) };
            let lab = self.t(key);
            if self.app_tile(ui, cell, ic, lab, hue, s) {
                self.launch(ui, key, target);
            }
        }
        let h = n.div_ceil(cols) as f32 * row_h + ui.dp(12.0);
        ui.scroll_end(area, h, sc);
    }

    // ------------------------------------------------------------ Schnelleinstellungen

    /// Schnelleinstellungen wie in der Web-Demo: Punktuhr mit Datum, Aus/Einstellungen, zwei große Kacheln,
    /// 12 runde Schalter, breiter Helligkeitsregler, Benachrichtigungen als Karten
    pub(super) fn d_shade(&mut self, ui: &mut Ui) {
        ui.set_clip(None);
        ui.enabled = true;
        let panel = Rect::new(0.0, 0.0, ui.w, ui.h - ui.dp(52.0));
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0x060709));
        let m = ui.dp(16.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let mut y = ui.dp(34.0);
        // Kopf: Uhr (Punktmatrix) + Datum, rechts Aus und Einstellungen
        let dt = sys::from_unix(sys::unix_now(), self.set.tz_min);
        let clock = self.clock_str(dt.h, dt.mi);
        // Uhr so groß wie möglich, aber Datum (mind. 96 dp) und die zwei Knöpfe (96 dp) passen daneben
        let room = ui.w - 2.0 * m - ui.dp(96.0 + 10.0 + 96.0);
        let mut cfs = 42.0;
        while cfs > 26.0 && ui.text_w(&clock, cfs, true) > room {
            cfs -= 2.0;
        }
        let cw = ui.text_w(&clock, cfs, true) + ui.dp(2.0);
        let cx0 = if ui.rtl { ui.w - m - cw } else { m };
        let cr = Rect::new(cx0, y, cw, ui.dp(50.0));
        ui.label(cr, &clock, cfs, c, true, Al::Start);
        ui.cv.dot_screen(cr, ui.dp(3.0).max(2.0), |_| rgb(0x060709));
        let mon = self.t(super::app_sys::MONTHS[(dt.mo as usize).clamp(1, 12) - 1]);
        let date = if self.set.lang == "ar" { format!("{} {}", mon, dt.d) } else { format!("{}, {}. {}", &self.t(DAYS[dt.wd]).chars().take(2).collect::<String>(), dt.d, mon.chars().take(3).collect::<String>()) };
        let dw = (ui.w - 2.0 * m - ui.dp(96.0 + 10.0) - cw).max(ui.dp(60.0));
        let dx = if ui.rtl { cx0 - ui.dp(10.0) - dw } else { cx0 + cw + ui.dp(10.0) };
        let date = ui.fit(&date, 14.0, false, dw);
        ui.label(Rect::new(dx, y + ui.dp(18.0), dw, ui.dp(24.0)), &date, 14.0, cs, false, Al::Start);
        let (b1, b2) = if ui.rtl { (m + ui.dp(22.0), m + ui.dp(74.0)) } else { (ui.w - m - ui.dp(74.0), ui.w - m - ui.dp(22.0)) };
        if ui.icon_btn(b1, cr.cy(), ui.dp(20.0), Ic::Power, ui.th.press, c) {
            self.shade = false;
            self.power_dialog = true;
            ui.dirty = true;
            return;
        }
        if ui.icon_btn(b2, cr.cy(), ui.dp(20.0), Ic::Gear, ui.th.press, c) {
            self.shade = false;
            self.go(Screen::Settings);
            ui.dirty = true;
            return;
        }
        y += ui.dp(64.0);
        // Laufender Anruf ganz oben
        y += self.shade_call(ui, m, y, ui.w - 2.0 * m);
        // Zwei große Kacheln: WLAN, Mobile Daten
        let st = self.radio();
        let p = self.rprefs();
        let wifi_on = quick::wifi_on();
        let air = quick::airplane_on();
        let ssid = if wifi_on { sys::wifi_ssid().unwrap_or_default() } else { String::new() };
        let mob = quick::mobile_on();
        let wsub = if air { self.t("Airplane mode").to_string() } else if !wifi_on { self.t("Off").to_string() } else if sys::online() && !ssid.is_empty() { ssid } else { self.t("Not connected").to_string() };
        let msub = if air { "—".to_string() } else if !mob { self.t("Off").to_string() } else if st.in_service() { format!("{} · {}", if st.operator.is_empty() { format!("SIM {}", p.data_sim.max(1)) } else { st.operator.clone() }, rat_short(&st.rat)) } else { self.t("No service").to_string() };
        let gap = ui.dp(10.0);
        let bw = (ui.w - 2.0 * m - gap) / 2.0;
        let bigs: [(Ic, &str, String, bool); 2] = [(Ic::Wifi, self.t("Wi-Fi"), wsub, wifi_on && !air), (Ic::UpDown, self.t("Mobile data"), msub, mob && !air)];
        for (i, (ic, name, sub, on)) in bigs.into_iter().enumerate() {
            let col = if ui.rtl { 1 - i } else { i };
            let r = Rect::new(m + col as f32 * (bw + gap), y, bw, ui.dp(70.0));
            let held = ui.held(r);
            let bg = if on { ui.th.accent } else { ui.th.press };
            ui.cv.rrect(r, ui.dp(24.0), if held { bg.mix(rgb(0), 0.15) } else { bg });
            let fg = if on { ui.th.on_accent } else { c };
            let ix = if ui.rtl { r.right() - ui.dp(26.0) } else { r.x + ui.dp(26.0) };
            ui.icon(ic, ix, r.cy(), ui.dp(22.0), fg);
            let tx = Rect::new(if ui.rtl { r.x + ui.dp(10.0) } else { r.x + ui.dp(50.0) }, r.y, r.w - ui.dp(60.0), r.h);
            let nf = ui.fit(name, 15.0, true, tx.w);
            ui.label(Rect::new(tx.x, r.y + ui.dp(15.0), tx.w, ui.dp(20.0)), &nf, 15.0, fg, true, Al::Start);
            let sf = ui.fit(&sub, 12.0, false, tx.w);
            ui.label(Rect::new(tx.x, r.y + ui.dp(37.0), tx.w, ui.dp(18.0)), &sf, 12.0, fg.alpha(0.75), false, Al::Start);
            if self.long_press(ui, r) {
                self.shade = false;
                self.go(if i == 0 { Screen::Net } else { Screen::Radio });
                ui.dirty = true;
                return;
            }
            if ui.tap(r) {
                if air {
                    quick::set_airplane(false);
                }
                if i == 0 { quick::set_wifi(!on) } else { quick::set_mobile(!on) }
                ui.dirty = true;
            }
        }
        y += ui.dp(70.0) + ui.dp(12.0);
        // 12 runde Schalter
        let bt = quick::bt_on();
        let vpn_on = crate::sys::vpn_status().up;
        let qs: [(Ic, &str, bool); 12] = [
            (Ic::Bt, "Bluetooth", bt && !air),
            (Ic::Plane, self.t("Airplane"), air),
            (Ic::Torch, self.t("Torch"), quick::torch_on()),
            (Ic::Lock, "VPN", vpn_on),
            (Ic::Speaker, self.t("Sound"), self.set.sound),
            (Ic::Moon, self.t("Do not disturb"), self.sx_dnd()),
            (Ic::Sun, self.t("Night light"), self.sx_night()),
            (Ic::Heart, self.t("Battery saver"), self.sx_saver()),
            (Ic::Shield, self.t("Privacy"), !self.set.priv_all),
            (Ic::Sun, self.t("Light mode"), !self.set.dark),
            (Ic::Spark, self.t("Helper"), !self.buddy.away),
            (Ic::Image, self.t("Screenshot"), false),
        ];
        let colw = (ui.w - 2.0 * m) / 4.0;
        let qd = ui.dp(62.0).min(colw - ui.dp(10.0));
        for (i, (ic, name, on)) in qs.iter().enumerate() {
            let col = if ui.rtl { 3 - i % 4 } else { i % 4 };
            let cx = m + col as f32 * colw + colw / 2.0;
            let ry = y + (i / 4) as f32 * (qd + ui.dp(36.0));
            let r = Rect::new(cx - qd / 2.0, ry, qd, qd);
            let held = ui.held(r);
            let bg = if *on { ui.th.accent } else { ui.th.press };
            ui.cv.rrect(r, ui.dp(22.0), if held { bg.mix(rgb(0), 0.15) } else { bg });
            ui.icon(*ic, r.cx(), r.cy(), ui.dp(22.0), if *on { ui.th.on_accent } else { c });
            let lf = ui.fit(name, 11.0, false, colw - ui.dp(4.0));
            ui.label(Rect::new(cx - colw / 2.0, r.bottom() + ui.dp(5.0), colw, ui.dp(16.0)), &lf, 11.0, cs, false, Al::Center);
            if ui.tap(r) {
                match i {
                    0 => quick::set_bt(!on),
                    1 => {
                        quick::set_airplane(!on);
                        if !on {
                            self.toast(self.t("Airplane mode: all radios off").into());
                        }
                    }
                    2 => quick::set_torch(!on),
                    3 => {
                        if *on {
                            let _ = { crate::sys::memo_clear(); ultar_vpn::disconnect() };
                        } else if ultar_vpn::load_saved().is_some() {
                            let _ = { crate::sys::memo_clear(); ultar_vpn::connect_saved() };
                        } else {
                            self.toast(self.t("Sign in first (Account).").into());
                        }
                    }
                    4 => {
                        self.set.sound = !self.set.sound;
                        self.save();
                    }
                    5 => self.sx_set_dnd(!on),
                    6 => self.sx_set_night(!on),
                    7 => self.sx_set_saver(!on),
                    8 => {
                        self.set.priv_all = !self.set.priv_all;
                        self.save();
                        privacy::apply(&self.set);
                    }
                    9 => {
                        self.set.dark = !self.set.dark;
                        self.save();
                    }
                    10 => {
                        self.buddy.away = !self.buddy.away;
                    }
                    _ => {
                        self.shade = false;
                        self.mx_shot_soon();
                        self.toast(self.t("Screenshot in 3 s").into());
                    }
                }
                ui.dirty = true;
            }
        }
        y += 3.0 * (qd + ui.dp(36.0)) + ui.dp(2.0);
        // Helligkeit: breite Pille, goldene Füllung, Sonne vorn
        let br = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(52.0));
        ui.cv.rrect(br, ui.dp(26.0), ui.th.press);
        let mut v = ((self.set.brightness - 0.08) / 0.92).clamp(0.0, 1.0);
        if ui.p.down && ui.p.capture.is_none() && br.contains(ui.p.px, ui.p.py) && !self.buddy.blocks(ui.p) {
            let mut nv = ((ui.p.x - br.x) / br.w).clamp(0.0, 1.0);
            if ui.rtl {
                nv = 1.0 - nv;
            }
            v = nv;
            self.set.brightness = 0.08 + 0.92 * v;
            self.apply_brightness();
            ui.p.moved = true;
            ui.dirty = true;
        }
        let fw = (br.w * v).max(ui.dp(52.0));
        let fr = if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) };
        ui.cv.rrect(fr, ui.dp(26.0), ui.th.accent);
        let ix = if ui.rtl { br.right() - ui.dp(26.0) } else { br.x + ui.dp(26.0) };
        ui.icon(Ic::Sun, ix, br.cy(), ui.dp(20.0), ui.th.on_accent);
        y += ui.dp(52.0) + ui.dp(18.0);
        // Benachrichtigungen
        let mut notes: Vec<ultar_notify::Note> = Vec::new();
        notes.extend(sys::notes_all());
        notes.sort_by_key(|n| std::cmp::Reverse(n.ts));
        ui.label(Rect::new(m + ui.dp(4.0), y, ui.w - 2.0 * m, ui.dp(20.0)), &self.t("Notifications").to_uppercase(), 12.0, cs, false, Al::Start);
        if !notes.is_empty() {
            let lab = self.t("Clear all");
            let lw = ui.text_w(lab, 13.0, false) + ui.dp(26.0);
            let px = if ui.rtl { m } else { ui.w - m - lw };
            let (_, t) = ui.pill(px, y - ui.dp(6.0), lab, false);
            if t {
                crate::sysd::clear_notifications();
                ui.dirty = true;
            }
        }
        y += ui.dp(32.0);
        if notes.is_empty() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(90.0));
            ui.icon(Ic::Bell, r.cx(), r.y + ui.dp(30.0), ui.dp(26.0), cs);
            ui.label(Rect::new(r.x, r.y + ui.dp(52.0), r.w, ui.dp(20.0)), self.t("No notifications"), 14.0, cs, false, Al::Center);
        } else {
            let now = sys::unix_now();
            let mut shown = 0;
            for n in notes.iter() {
                let nh = ui.dp(74.0);
                if y + nh > panel.bottom() - ui.dp(34.0) {
                    break;
                }
                shown += 1;
                let r = Rect::new(m, y, ui.w - 2.0 * m, nh);
                ui.card(r);
                let ix = if ui.rtl { r.right() - ui.dp(31.0) } else { r.x + ui.dp(31.0) };
                ui.cv.rrect(Rect::new(ix - ui.dp(17.0), r.y + ui.dp(12.0), ui.dp(34.0), ui.dp(34.0)), ui.dp(11.0), rgb(0x2A2D35));
                let nic = match n.kind.as_str() { "call" => Ic::Phone, "sms" | "message" => Ic::Chat, "store" => Ic::Box, _ => Ic::Bell };
                ui.icon(nic, ix, r.y + ui.dp(29.0), ui.dp(17.0), ui.th.accent);
                let tx = Rect::new(if ui.rtl { r.x + ui.dp(40.0) } else { r.x + ui.dp(60.0) }, r.y, r.w - ui.dp(100.0), r.h);
                let raw = if n.app.is_empty() { n.kind.clone() } else { n.app.clone() };
                let cap: String = raw.chars().next().map(|ch| ch.to_uppercase().collect::<String>() + &raw[ch.len_utf8()..]).unwrap_or_default();
                ui.label(Rect::new(tx.x, r.y + ui.dp(10.0), tx.w, ui.dp(16.0)), &self.t_dyn(&cap), 12.0, cs, false, Al::Start);
                if n.ts > 0 {
                    let mins = (now - n.ts).max(0) / 60;
                    let (v, u) = if mins < 60 { (mins, 0) } else { (mins / 60, 1) };
                    let ago = match self.set.lang.as_str() {
                        "de" => format!("vor {v} {}", if u == 0 { "Min." } else { "Std." }),
                        "ar" => format!("منذ {v} {}", if u == 0 { "د" } else { "س" }),
                        _ => format!("{v} {} ago", if u == 0 { "min" } else { "h" }),
                    };
                    ui.label(Rect::new(tx.x, r.y + ui.dp(10.0), tx.w, ui.dp(16.0)), &ago, 12.0, cs, false, Al::End);
                }
                let tf = ui.fit(&n.title, 14.0, true, tx.w);
                ui.label(Rect::new(tx.x, r.y + ui.dp(27.0), tx.w, ui.dp(20.0)), &tf, 14.0, c, true, Al::Start);
                let bf = ui.fit(&n.body, 13.0, false, tx.w);
                ui.label(Rect::new(tx.x, r.y + ui.dp(47.0), tx.w, ui.dp(18.0)), &bf, 13.0, cs, false, Al::Start);
                y += nh + ui.dp(8.0);
            }
            if shown < notes.len() && y + ui.dp(18.0) <= panel.bottom() - ui.dp(30.0) {
                let more = format!("+{} · {}", notes.len() - shown, self.t("Notification history"));
                let mr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(18.0));
                let ac = ui.th.accent;
                ui.label(mr, &more, 12.0, ac, true, Al::Center);
                if ui.tap(mr.inset(-ui.dp(8.0))) {
                    self.shade = false;
                    self.go(Screen::Sys(super::app_sys::Sx::NotifHist));
                }
            }
        }
        self.shade_handle(ui, panel);
    }

    /// Griff unten am Panel; Tippen darunter oder auf den Griff schließt.
    fn shade_handle(&mut self, ui: &mut Ui, panel: Rect) {
        let cs = ui.th.sub;
        let handle_y = panel.bottom() - ui.dp(18.0);
        ui.cv.rrect(Rect::new((ui.w - ui.dp(44.0)) / 2.0, handle_y, ui.dp(44.0), ui.dp(5.0)), ui.dp(2.5), cs.alpha(0.6));
        if ui.p.released && !ui.p.moved && (ui.p.py > panel.bottom() || ui.p.py > handle_y - ui.dp(14.0)) {
            self.shade = false;
            ui.dirty = true;
        }
        ui.dirty = true;
    }

    /// Große Schnellschalter-Kachel: Symbol, AN/AUS, Name, Zustandszeile.
    fn big_tile(&mut self, ui: &mut Ui, r: Rect, ic: Ic, name: &str, sub: &str, on: bool) -> bool {
        let held = ui.held(r);
        let rad = ui.dp(18.0);
        let bg = if on { ui.th.accent } else { ui.th.text.alpha(0.05) };
        let bg = if held { bg.mix(rgb(0), 0.2) } else { bg };
        if on {
            ui.cv.rrect(r, rad, bg);
            ui.cv.rrect(Rect::new(r.x, r.y, r.w, r.h * 0.45), rad, ui.th.accent2.alpha(0.35));
        } else {
            ui.cv.rrect(r, rad, ui.th.line);
            ui.cv.rrect(r.inset(1.0), rad, if held { ui.th.press } else { ui.th.card });
        }
        let fg = if on { ui.th.on_accent } else { ui.th.text };
        let sfg = if on { ui.th.on_accent.alpha(0.75) } else { ui.th.sub };
        let inn = r.inset(ui.dp(10.0));
        let compact = r.h < ui.dp(72.0);
        let icx = if ui.rtl { inn.right() - ui.dp(10.0) } else { inn.x + ui.dp(10.0) };
        ui.icon(ic, icx, inn.y + ui.dp(10.0), ui.dp(19.0), fg);
        let stt = if on { self.t("ON") } else { self.t("OFF") };
        ui.label(Rect::new(inn.x, inn.y, inn.w, ui.dp(22.0)), stt, 10.5, sfg, true, Al::End);
        let nf = ui.fit(name, 15.0, true, inn.w);
        if compact {
            // niedrige Kachel: nur Name (Zustand steht oben als AN/AUS)
            ui.label(Rect::new(inn.x, inn.bottom() - ui.dp(20.0), inn.w, ui.dp(20.0)), &nf, 14.5, fg, true, Al::Start);
        } else {
            ui.label(Rect::new(inn.x, inn.bottom() - ui.dp(35.0), inn.w, ui.dp(20.0)), &nf, 15.0, fg, true, Al::Start);
            let sf = ui.fit(sub, 12.0, false, inn.w);
            ui.label(Rect::new(inn.x, inn.bottom() - ui.dp(16.0), inn.w, ui.dp(16.0)), &sf, 12.0, sfg, false, Al::Start);
        }
        ui.tap(r)
    }
}
