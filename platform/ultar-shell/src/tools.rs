//! Eingebaute Apps: Rechner, Uhr (Wecker, Timer, Stoppuhr), Dateien, Galerie/Betrachter und die Werbesperre.
//! Untermodul von `app`, damit die Bildschirme direkt auf den Zustand der Oberfläche zugreifen können.

use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::sys;
use crate::ui::{Al, Btn, Ui};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Alarm {
    pub h: u8,
    pub m: u8,
    pub on: bool,
    /// Wochentage wie in der Demo: "12345" = Mo–Fr, "67" = Wochenende, leer = einmal
    #[serde(default)]
    pub days: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Alarms {
    pub list: Vec<Alarm>,
}

/// Dekodiertes Bild (RGBA)
pub struct Pic {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Tools {
    // Rechner
    pub expr: String,
    pub result: String,
    /// Rechner-Reiter: 0 Standard, 1 Einheiten, 2 Zahlensysteme
    pub calc_tab: usize,
    // Uhr
    pub tab: usize,
    pub alarms: Alarms,
    pub new_h: u8,
    pub new_m: u8,
    pub last_fired: i64,
    pub timer_min: u32,
    pub timer_end: Option<Instant>,
    pub sw_start: Option<Instant>,
    pub sw_acc: Duration,
    /// Runden der Stoppuhr (neueste zuerst)
    pub sw_laps: Vec<Duration>,
    /// Timer: Gesamtdauer (für den Ring) und Restzeit bei Pause
    pub timer_total: u64,
    pub timer_paused: Option<u64>,
    /// Klingelt gerade: Text der Meldung
    pub ringing: Option<String>,
    // Dateien
    pub cwd: String,
    pub files: Vec<(String, bool, u64)>,
    pub files_for: String,
    pub del_armed: bool,
    /// Sortierung in Dateien: 0 Name, 1 Datum (neueste zuerst), 2 Größe
    pub fsort: u8,
    pub vol_saved: f32,
    /// Kontextmenü für einen Eintrag: (voller Pfad, Ordner?)
    pub menu: Option<(String, bool)>,
    /// Zwischenablage: (Pfad, verschieben statt kopieren)
    pub clip: Option<(String, bool)>,
    /// Wie oft jede App vom Startbildschirm geöffnet wurde (für „Häufig genutzt“)
    pub launches: Launches,
    // Betrachter / Galerie
    pub view: Option<(String, Result<Pic, String>)>,
    pub text: Option<(String, Vec<String>)>,
    // Werbesperre
    pub blocked: usize,
}

pub const BLOCKLIST_URL: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";
const BLOCKLIST_FILES: [&str; 2] = ["/data/ultar/blocklist.hosts", "/compat/var/tmp/ultar-blocklist.hosts"];
const MARK: &str = "# --- ultar-adblock ---";
/// Kleine eingebaute Liste, damit die Sperre auch ohne heruntergeladene Liste wirkt.
const BUILTIN: &[&str] = &[
    "doubleclick.net", "ad.doubleclick.net", "googleads.g.doubleclick.net", "pagead2.googlesyndication.com", "googlesyndication.com",
    "googleadservices.com", "www.googleadservices.com", "adservice.google.com", "google-analytics.com", "www.google-analytics.com",
    "ssl.google-analytics.com", "googletagmanager.com", "www.googletagmanager.com", "googletagservices.com", "connect.facebook.net",
    "pixel.facebook.com", "an.facebook.com", "graph.instagram.com", "ads.twitter.com", "analytics.twitter.com", "ads.linkedin.com",
    "bat.bing.com", "ads.yahoo.com", "adnxs.com", "ib.adnxs.com", "criteo.com", "static.criteo.net", "taboola.com", "cdn.taboola.com",
    "outbrain.com", "widgets.outbrain.com", "scorecardresearch.com", "sb.scorecardresearch.com", "quantserve.com", "hotjar.com",
    "static.hotjar.com", "amazon-adsystem.com", "aax.amazon-adsystem.com", "moatads.com", "adsrvr.org", "rubiconproject.com",
    "pubmatic.com", "openx.net", "app-measurement.com", "appsflyer.com", "adjust.com", "branch.io", "mixpanel.com", "segment.io",
];

/// App-Dateien, die uapp installieren kann (.uap, .deb, .apk, .exe, .msi, AppImage, Flatpak, .rpm)
pub fn installable(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    [".uap", ".deb", ".apk", ".exe", ".msi", ".appimage", ".flatpak", ".flatpakref", ".rpm"].iter().any(|e| l.ends_with(e))
}

pub fn is_image(p: &str) -> bool {
    let l = p.to_lowercase();
    l.ends_with(".png") || l.ends_with(".jpg") || l.ends_with(".jpeg")
}

pub fn decode(path: &str, max: u32) -> Result<Pic, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 40 * 1024 * 1024 {
        return Err("Datei zu groß".into());
    }
    let im = image::open(path).map_err(|e| e.to_string())?;
    let im = if im.width() > max || im.height() > max { im.thumbnail(max, max) } else { im };
    let rgba = im.to_rgba8();
    Ok(Pic { w: rgba.width(), h: rgba.height(), rgba: rgba.into_raw() })
}

// ---------------------------------------------------------------- Dateiarbeiten

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Launches {
    pub count: std::collections::BTreeMap<String, u32>,
}

/// Schreibende Dateiarbeiten sind nur im Nutzerbereich erlaubt.
pub fn writable(p: &str) -> bool {
    p.starts_with("/data/") && !p.contains("/../") && !p.ends_with("/..")
}

/// Freien Namen im Zielordner finden: "name", "name (2)", "name (3)" …
pub fn free_name(dir: &str, name: &str) -> String {
    let base = format!("{dir}/{name}");
    if !std::path::Path::new(&base).exists() {
        return base;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..1000).map(|n| format!("{dir}/{stem} ({n}){ext}")).find(|p| !std::path::Path::new(p).exists()).unwrap_or(base)
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path, budget: &mut u32) -> std::io::Result<()> {
    if *budget == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "zu viele Dateien"));
    }
    *budget -= 1;
    let md = std::fs::symlink_metadata(src)?;
    if md.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_tree(&e.path(), &dst.join(e.file_name()), budget)?;
        }
        Ok(())
    } else if md.is_file() {
        std::fs::copy(src, dst).map(|_| ())
    } else {
        Ok(()) // Verknüpfungen und Gerätedateien auslassen
    }
}

/// Kopieren oder Verschieben nach `dir`. Liefert den neuen Pfad.
pub fn paste(src: &str, dir: &str, mv: bool) -> Result<String, String> {
    if !writable(&format!("{dir}/")) || (mv && !writable(src)) {
        return Err("nur im Nutzerbereich /data möglich".into());
    }
    let name = std::path::Path::new(src).file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("kein Name")?;
    if format!("{dir}/").starts_with(&format!("{src}/")) {
        return Err("Ziel liegt im Ordner selbst".into());
    }
    let dst = free_name(dir, &name);
    if mv && std::fs::rename(src, &dst).is_ok() {
        return Ok(dst);
    }
    let mut budget = 20_000;
    copy_tree(std::path::Path::new(src), std::path::Path::new(&dst), &mut budget).map_err(|e| e.to_string())?;
    if mv {
        let p = std::path::Path::new(src);
        if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }.map_err(|e| e.to_string())?;
    }
    Ok(dst)
}

/// Umbenennen innerhalb desselben Ordners.
pub fn rename(src: &str, new_name: &str) -> Result<String, String> {
    let new_name = new_name.trim();
    if new_name.is_empty() || new_name.contains('/') || new_name == "." || new_name == ".." {
        return Err("ungültiger Name".into());
    }
    if !writable(src) {
        return Err("nur im Nutzerbereich /data möglich".into());
    }
    let dir = std::path::Path::new(src).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let dst = format!("{dir}/{new_name}");
    if std::path::Path::new(&dst).exists() {
        return Err("Name ist schon vergeben".into());
    }
    std::fs::rename(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

// ---------------------------------------------------------------- Rechner

/// Rechnet einen Ausdruck mit + − × ÷ % und Klammern aus (Punkt vor Strich).
pub fn calc(expr: &str) -> Result<f64, String> {
    let toks: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).map(|c| match c {
        '×' => '*',
        '÷' => '/',
        '−' => '-',
        ',' => '.',
        o => o,
    }).collect();
    let mut pos = 0;
    let v = sum(&toks, &mut pos)?;
    if pos != toks.len() {
        return Err("?".into());
    }
    if !v.is_finite() {
        return Err("∞".into());
    }
    Ok(v)
}

fn sum(t: &[char], p: &mut usize) -> Result<f64, String> {
    let mut v = product(t, p)?;
    while *p < t.len() && (t[*p] == '+' || t[*p] == '-') {
        let op = t[*p];
        *p += 1;
        let r = product(t, p)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Ok(v)
}

fn product(t: &[char], p: &mut usize) -> Result<f64, String> {
    let mut v = unary(t, p)?;
    while *p < t.len() && (t[*p] == '*' || t[*p] == '/') {
        let op = t[*p];
        *p += 1;
        let r = unary(t, p)?;
        if op == '/' && r == 0.0 {
            return Err("÷0".into());
        }
        v = if op == '*' { v * r } else { v / r };
    }
    Ok(v)
}

fn unary(t: &[char], p: &mut usize) -> Result<f64, String> {
    if *p < t.len() && t[*p] == '-' {
        *p += 1;
        return Ok(-unary(t, p)?);
    }
    let mut v = if *p < t.len() && t[*p] == '(' {
        *p += 1;
        let v = sum(t, p)?;
        if *p >= t.len() || t[*p] != ')' {
            return Err("()".into());
        }
        *p += 1;
        v
    } else {
        let s = *p;
        while *p < t.len() && (t[*p].is_ascii_digit() || t[*p] == '.') {
            *p += 1;
        }
        t[s..*p].iter().collect::<String>().parse::<f64>().map_err(|_| "?".to_string())?
    };
    while *p < t.len() && t[*p] == '%' {
        *p += 1;
        v /= 100.0;
    }
    Ok(v)
}

pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ---------------------------------------------------------------- Werbesperre

/// Sperrliste lesen: heruntergeladene Liste (hosts-Format) oder die eingebaute.
fn blocklist() -> Vec<String> {
    for f in BLOCKLIST_FILES {
        if let Ok(t) = std::fs::read_to_string(f) {
            let v: Vec<String> = t
                .lines()
                .filter_map(|l| {
                    let l = l.split('#').next().unwrap_or("").trim();
                    let mut it = l.split_whitespace();
                    match (it.next(), it.next()) {
                        (Some("0.0.0.0" | "127.0.0.1"), Some(h)) if h.contains('.') && h != "0.0.0.0" && !h.starts_with("localhost") => Some(h.to_string()),
                        _ => None,
                    }
                })
                .collect();
            if v.len() > 100 {
                return v;
            }
        }
    }
    BUILTIN.iter().map(|s| s.to_string()).collect()
}

/// Sperre systemweit setzen oder aufheben: Namen der Werbe-/Tracker-Server lösen sich zu 0.0.0.0 auf.
/// Geschrieben wird in die hosts-Datei des Kernsystems und des Ubuntu-Bereichs (dort laufen die Apps).
/// Liefert die Zahl der gesperrten Namen.
pub fn adblock_apply(on: bool) -> usize {
    let list = if on { blocklist() } else { Vec::new() };
    for target in ["/etc/hosts", "/compat/etc/hosts"] {
        if target.starts_with("/compat") && !std::path::Path::new("/compat/etc").is_dir() {
            continue;
        }
        let old = std::fs::read_to_string(target).unwrap_or_default();
        let mut base: String = old.split(MARK).next().unwrap_or("").trim_end().to_string();
        if base.is_empty() {
            base = "127.0.0.1 localhost\n::1 localhost".into();
        }
        let mut out = String::with_capacity(base.len() + list.len() * 28 + 64);
        out.push_str(&base);
        out.push('\n');
        if on {
            out.push_str(MARK);
            out.push('\n');
            for h in &list {
                out.push_str("0.0.0.0 ");
                out.push_str(h);
                out.push('\n');
            }
        }
        let tmp = format!("{target}.ultar-tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, target);
        }
    }
    list.len()
}

// ---------------------------------------------------------------- Bildschirme

impl App {
    pub(super) fn tools_init(&mut self) {
        self.tl.alarms = sys::load("alarms.toml");
        self.tl.cwd = "/data".into();
        self.tl.timer_min = 5;
        self.tl.new_h = 7;
        self.tl.vol_saved = self.set.volume;
        self.tl.launches = sys::load("launches.toml");
        // ULTAR_DRY: Probelauf ohne Eingriffe ins System (Bildschirmfotos auf dem Entwicklungsrechner)
        if self.set.adblock && std::env::var("ULTAR_DRY").is_err() {
            self.tl.blocked = adblock_apply(true);
        }
    }

    pub(super) fn tools_needs_tick(&self) -> bool {
        self.tl.ringing.is_some() || self.tl.timer_end.is_some() || matches!(self.stack.last().map(|s| &s.0), Some(Screen::Clock))
    }

    /// Wecker und Timer prüfen; true = neu zeichnen.
    pub(super) fn tools_tick(&mut self) -> bool {
        let mut r = matches!(self.stack.last().map(|s| &s.0), Some(Screen::Clock));
        let now = sys::unix_now();
        let dt = sys::from_unix(now, self.set.tz_min);
        let minute = (now + self.set.tz_min as i64 * 60) / 60;
        if minute != self.tl.last_fired && self.tl.alarms.list.iter().any(|a| a.on && a.h as i64 == dt.h && a.m as i64 == dt.mi) {
            self.tl.last_fired = minute;
            let msg = format!("{} {:02}:{:02}", self.t("Alarm"), dt.h, dt.mi);
            ultar_notify::post("system", "clock", &msg, "", ultar_notify::Urgency::High);
            self.tl.ringing = Some(msg);
            r = true;
        }
        if self.tl.timer_end.is_some_and(|e| Instant::now() >= e) {
            self.tl.timer_end = None;
            let msg = self.t("Timer finished").to_string();
            ultar_notify::post("system", "clock", &msg, "", ultar_notify::Urgency::High);
            self.tl.ringing = Some(msg);
            r = true;
        }
        if self.tl.ringing.is_some() {
            if self.set.sound {
                crate::audio::alarm(self.set.volume);
            }
            // Vibrieren über den Weg, den der Geräteschalter gefunden hat (beide bekannten Fälle; fehlende schaden nicht)
            let _ = std::fs::write("/sys/class/timed_output/vibrator/enable", "350");
            let _ = std::fs::write("/sys/class/leds/vibrator/duration", "350").and_then(|_| std::fs::write("/sys/class/leds/vibrator/activate", "1"));
            r = true;
        }
        r
    }

    /// Klingel-Dialog (über allem)
    pub(super) fn d_ring(&mut self, ui: &mut Ui) {
        let Some(msg) = self.tl.ringing.clone() else { return };
        ui.set_clip(None);
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0).alpha(0.6));
        let w = (ui.w - ui.dp(48.0)).min(ui.dp(420.0));
        let r = Rect::new((ui.w - w) / 2.0, ui.h * 0.36, w, ui.dp(190.0));
        ui.card(r);
        let c = ui.th.text;
        ui.label(Rect::new(r.x, r.y + ui.dp(26.0), r.w, ui.dp(44.0)), &msg, 26.0, c, true, Al::Center);
        let old = ui.enabled;
        ui.enabled = true;
        let bw = (r.w - ui.dp(60.0)) / 2.0;
        let (bs, bz) = if ui.rtl { (r.x + ui.dp(24.0), r.x + ui.dp(36.0) + bw) } else { (r.x + ui.dp(36.0) + bw, r.x + ui.dp(24.0)) };
        if ui.button(Rect::new(bz, r.bottom() - ui.dp(76.0), bw, ui.dp(46.0)), self.t("Snooze 5 min"), Btn::Plain) {
            self.sys_snooze(msg.clone(), 5);
            self.tl.ringing = None;
            ui.dirty = true;
        }
        if ui.button(Rect::new(bs, r.bottom() - ui.dp(76.0), bw, ui.dp(46.0)), self.t("Stop"), Btn::Primary) {
            self.tl.ringing = None;
            ui.dirty = true;
        }
        ui.enabled = old;
    }

    /// Rechner wie in der Web-Demo: Reiter Standard / Einheiten / Zahlensysteme, Anzeige unten rechts,
    /// Tasten 4×5 (C ( ) ÷ · 7 8 9 × · 4 5 6 − · 1 2 3 + · % 0 . =), darunter breite Löschtaste.
    pub(super) fn s_calc(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Calculator"));
        if std::mem::take(&mut self.hdr_tap) {
            self.go(Screen::Sys(super::app_sys::Sx::CalcHist));
            ui.dirty = true;
            return;
        }
        let mut y = area.y;
        let tabs = [self.t("Standard"), self.t("Units"), self.t("Number systems")];
        if let Some(i) = ui.chips(&mut y, &tabs, self.tl.calc_tab) {
            self.tl.calc_tab = i;
            *sc = 0.0;
            ui.dirty = true;
        }
        let body = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
        if self.tl.calc_tab != 0 {
            self.no_hdr = true;
            if self.tl.calc_tab == 1 {
                self.s_units(ui, body, sc);
            } else {
                self.s_bases(ui, body);
            }
            self.no_hdr = false;
            return;
        }
        let (c, cs) = (ui.th.text, ui.th.sub);
        let old_rtl = ui.rtl;
        ui.rtl = false; // Zahlen und Rechenzeichen immer von links nach rechts
        let keys = ["C", "(", ")", "÷", "7", "8", "9", "×", "4", "5", "6", "–", "1", "2", "3", "+", "%", "0", ".", "="];
        let px = ui.dp(14.0);
        let gap = ui.dp(10.0);
        let gw = (ui.w - 2.0 * px).min(ui.dp(520.0));
        let gx = (ui.w - gw) / 2.0;
        let cw = (gw - 3.0 * gap) / 4.0;
        let kh = ui.dp(66.0).min((body.h - ui.dp(150.0)) / 5.0 - gap).max(ui.dp(40.0));
        let del_h = ui.dp(44.0);
        let grid_h = 5.0 * kh + 4.0 * gap;
        let del_y = body.bottom() - ui.dp(10.0) - del_h;
        let top = del_y - gap - grid_h;
        // Anzeige: Ausdruck groß, Ergebnis gedämpft darunter, rechtsbündig über den Tasten
        let disp = Rect::new(ui.dp(22.0), body.y, ui.w - ui.dp(44.0), (top - ui.dp(12.0) - body.y).max(ui.dp(60.0)));
        let shown = if self.tl.expr.is_empty() { "0".to_string() } else { self.tl.expr.clone() };
        let big = if shown.chars().count() > 12 { 30.0 } else { 44.0 };
        let sf = ui.fit(&shown, big, false, disp.w);
        ui.label(Rect::new(disp.x, disp.bottom() - ui.dp(30.0) - ui.dp(58.0), disp.w, ui.dp(58.0)), &sf, big, c, false, Al::End);
        if !self.tl.result.is_empty() {
            ui.label(Rect::new(disp.x, disp.bottom() - ui.dp(30.0), disp.w, ui.dp(30.0)), &format!("= {}", self.tl.result), 22.0, cs, false, Al::End);
        }
        for (i, k) in keys.iter().enumerate() {
            let r = Rect::new(gx + (i % 4) as f32 * (cw + gap), top + (i / 4) as f32 * (kh + gap), cw, kh);
            let held = ui.held(r);
            let is_op = matches!(*k, "÷" | "×" | "–" | "+");
            let (bgc, fg) = match *k {
                "=" => (ui.th.accent, ui.th.on_accent),
                "C" => (ui.th.card, rgb(0xF87171)),
                _ if is_op => (ui.th.press, ui.th.accent),
                _ => (ui.th.card, ui.th.text),
            };
            let rad = ui.dp(22.0).min(r.h / 2.0);
            ui.cv.rrect(r, rad, if held { bgc.mix(rgb(0xFFFFFF), 0.12) } else { bgc });
            ui.label(r, k, 24.0, fg, false, Al::Center);
            if ui.tap(r) {
                match *k {
                    "C" => {
                        self.tl.expr.clear();
                        self.tl.result.clear();
                    }
                    "=" => {
                        if let Ok(v) = calc(&self.tl.expr.replace('–', "-")) {
                            let res = fmt_num(v);
                            self.sys_calc_history(&self.tl.expr.clone(), &res);
                            self.tl.expr = res;
                            self.tl.result.clear();
                        }
                    }
                    o => {
                        if self.tl.expr.chars().count() < 64 {
                            self.tl.expr.push_str(o);
                        }
                    }
                }
                if *k != "=" && *k != "C" {
                    self.tl.result = calc(&self.tl.expr.replace('–', "-")).map(fmt_num).unwrap_or_default();
                }
                ui.dirty = true;
            }
        }
        // breite Löschtaste
        let dr = Rect::new(gx, del_y, gw, del_h);
        let held = ui.held(dr);
        ui.cv.rrect(dr, ui.dp(22.0), if held { ui.th.press } else { ui.th.card });
        ui.icon(crate::ui::Ic::Back, dr.cx(), dr.cy(), ui.dp(20.0), c);
        if ui.tap(dr) {
            self.tl.expr.pop();
            self.tl.result = calc(&self.tl.expr.replace('–', "-")).map(fmt_num).unwrap_or_default();
            ui.dirty = true;
        }
        ui.rtl = old_rtl;
    }

    /// Uhr wie in der Web-Demo: Reiter Wecker / Weltuhr / Timer / Stoppuhr
    pub(super) fn s_clock(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        use crate::ui::Ic;
        let area = self.header(ui, area, self.t("Clock"));
        let mut y = area.y;
        let tabs = [self.t("Alarm"), self.t("World"), self.t("Timer"), self.t("Stopwatch")];
        if let Some(i) = ui.chips(&mut y, &tabs, self.tl.tab) {
            self.tl.tab = i;
            *sc = 0.0;
            ui.dirty = true;
        }
        let body = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(16.0);
        match self.tl.tab {
            0 => {
                ui.scroll_begin(body, sc);
                let mut y = body.y - *sc;
                let mut changed = false;
                let mut remove = None;
                let n = self.tl.alarms.list.len();
                let rh = ui.dp(76.0);
                if n > 0 {
                    let lr = Rect::new(m, y, ui.w - 2.0 * m, rh * n as f32);
                    ui.card(lr);
                }
                for (i, a) in self.tl.alarms.list.clone().into_iter().enumerate() {
                    let r = Rect::new(m, y, ui.w - 2.0 * m, rh);
                    if i > 0 {
                        ui.cv.rect(Rect::new(r.x + 1.0, r.y, r.w - 2.0, 1.0), ui.th.line);
                    }
                    let pad = ui.dp(14.0);
                    let tx = if ui.rtl { r.right() - pad - ui.dp(160.0) } else { r.x + pad };
                    let old = ui.rtl;
                    ui.rtl = false;
                    ui.label(Rect::new(tx, r.y + ui.dp(8.0), ui.dp(160.0), ui.dp(42.0)), &format!("{:02}:{:02}", a.h, a.m), 34.0, if a.on { c } else { cs }, false, if old { Al::End } else { Al::Start });
                    ui.rtl = old;
                    let sub = match a.days.as_str() {
                        "12345" => self.t("Mon–Fri"),
                        "67" => self.t("Weekend"),
                        _ => self.t("Once"),
                    };
                    ui.label(Rect::new(if ui.rtl { r.right() - pad - ui.dp(160.0) } else { r.x + pad }, r.y + ui.dp(50.0), ui.dp(160.0), ui.dp(18.0)), sub, 12.5, cs, false, Al::Start);
                    // Schalter und Papierkorb am Ende
                    let (tw, th) = (ui.dp(44.0), ui.dp(26.0));
                    let tx2 = if ui.rtl { r.x + pad } else { r.right() - pad - tw };
                    let t = Rect::new(tx2, r.cy() - th / 2.0, tw, th);
                    ui.cv.rrect(t, th / 2.0, if a.on { ui.th.accent } else { ui.th.line.mix(cs, 0.25) });
                    let kx = if a.on != ui.rtl { t.right() - th / 2.0 } else { t.x + th / 2.0 };
                    ui.cv.circle(kx, t.cy(), th / 2.0 - ui.dp(3.0), if a.on { ui.th.on_accent } else { cs });
                    if ui.tap(t.inset(-ui.dp(8.0))) {
                        self.tl.alarms.list[i].on = !a.on;
                        changed = true;
                    }
                    let bx = if ui.rtl { tx2 + tw + ui.dp(30.0) } else { tx2 - ui.dp(30.0) };
                    if ui.icon_btn(bx, r.cy(), ui.dp(15.0), Ic::Trash, ui.th.card.alpha(0.0), c) {
                        remove = Some(i);
                    }
                    y += rh;
                }
                if n > 0 {
                    y += ui.dp(14.0);
                }
                if let Some(i) = remove {
                    self.tl.alarms.list.remove(i);
                    changed = true;
                }
                let br = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0));
                if ui.button(br, &format!("+  {}", self.t("Add alarm")), Btn::Primary) && self.tl.alarms.list.len() < 20 {
                    self.tl.new_h = 7;
                    self.tl.new_m = 0;
                    self.sheet = Some(super::app_demo::Sheet::NewAlarm);
                    ui.dirty = true;
                }
                y += ui.dp(60.0);
                if changed {
                    let _ = sys::save("alarms.toml", &self.tl.alarms);
                    ui.dirty = true;
                }
                ui.scroll_end(body, y + *sc - body.y, sc);
            }
            1 => {
                // Weltuhr: Städte mit Wochentag und Uhrzeit
                let now = sys::unix_now();
                let utc = sys::from_unix(now, 0);
                let eu_dst = (4..=9).contains(&utc.mo) || (utc.mo == 3 && utc.d >= 25) || (utc.mo == 10 && utc.d < 25);
                let us_dst = (4..=10).contains(&utc.mo) || (utc.mo == 3 && utc.d >= 9);
                let cities: [(&str, i64); 5] = [
                    ("Berlin", if eu_dst { 120 } else { 60 }),
                    ("Damascus", 180),
                    ("Dubai", 240),
                    ("Riyadh", 180),
                    ("New York", if us_dst { -240 } else { -300 }),
                ];
                ui.scroll_begin(body, sc);
                let mut y = body.y - *sc;
                for (name, off) in cities {
                    let dt = sys::from_unix(now, off as i32);
                    let wd = self.t(crate::i18n::DAYS[dt.wd as usize % 7]);
                    let t = format!("{:02}:{:02}", dt.h, dt.mi);
                    let y0 = y;
                    ui.row(&mut y, None, self.t(name), Some(wd), Some("        "), false);
                    let r = Rect::new(m + ui.dp(14.0), y0, ui.w - 2.0 * m - ui.dp(28.0), y - y0);
                    let old = ui.rtl;
                    ui.rtl = false;
                    ui.label(r, &t, 30.0, c, false, if old { Al::Start } else { Al::End });
                    ui.rtl = old;
                }
                ui.scroll_end(body, y + *sc - body.y, sc);
            }
            2 => {
                // Timer: Ring mit Restzeit; ohne Lauf Schnellwahl 1–25 min
                let left = match (self.tl.timer_paused, self.tl.timer_end) {
                    (Some(p), _) => Some(p),
                    (None, Some(e)) => Some(e.saturating_duration_since(Instant::now()).as_secs()),
                    _ => None,
                };
                if let Some(l) = left {
                    if self.tl.timer_total < l {
                        self.tl.timer_total = l.max(1);
                    }
                }
                let total = self.tl.timer_total.max(1);
                let frac = left.map(|l| l as f32 / total as f32).unwrap_or(0.0);
                let d = ui.dp(240.0).min(body.w - ui.dp(40.0));
                let (cx, cy) = (ui.w / 2.0, body.y + ui.dp(20.0) + d / 2.0);
                let (ro, ri) = (d / 2.0, d / 2.0 - ui.dp(12.0));
                ui.cv.ring(cx, cy, (ro + ri) / 2.0, ro - ri, ui.th.line);
                if frac > 0.0 {
                    let ac = ui.th.accent;
                    let bb = Rect::new(cx - ro - 1.0, cy - ro - 1.0, 2.0 * ro + 2.0, 2.0 * ro + 2.0);
                    let mid = (ro + ri) / 2.0;
                    let hw = (ro - ri) / 2.0;
                    ui.cv.sdf(bb, ac, |x, y| {
                        let (dx, dy) = (x - cx, y - cy);
                        // Winkel im Uhrzeigersinn ab 12 Uhr (0..1)
                        let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
                        if a > frac {
                            return 9.0;
                        }
                        ((dx * dx + dy * dy).sqrt() - mid).abs() - hw
                    });
                }
                let l = left.unwrap_or(0);
                let old = ui.rtl;
                ui.rtl = false;
                ui.label(Rect::new(cx - ri, cy - ui.dp(30.0), 2.0 * ri, ui.dp(60.0)), &format!("{:02}:{:02}", l / 60, l % 60), 40.0, c, false, Al::Center);
                ui.rtl = old;
                let by = cy + ro + ui.dp(18.0);
                if left.is_some() {
                    let bw = ui.dp(130.0);
                    let gap = ui.dp(12.0);
                    let b1 = Rect::new(ui.w / 2.0 - bw - gap / 2.0, by, bw, ui.dp(46.0));
                    let b2 = Rect::new(ui.w / 2.0 + gap / 2.0, by, bw, ui.dp(46.0));
                    let (b1, b2) = if ui.rtl { (b2, b1) } else { (b1, b2) };
                    let running = self.tl.timer_paused.is_none();
                    if ui.button(b1, if running { self.t("Pause") } else { self.t("Resume") }, Btn::Primary) {
                        if running {
                            self.tl.timer_paused = left;
                            self.tl.timer_end = None;
                        } else {
                            self.tl.timer_end = Some(Instant::now() + Duration::from_secs(left.unwrap_or(0)));
                            self.tl.timer_paused = None;
                        }
                        ui.dirty = true;
                    }
                    if ui.button(b2, self.t("Reset"), Btn::Plain) {
                        self.tl.timer_end = None;
                        self.tl.timer_paused = None;
                        self.tl.timer_total = 0;
                        ui.dirty = true;
                    }
                } else {
                    // Schnellwahl als Pillen, zentriert umbrechend
                    let mins = [1u64, 3, 5, 10, 15, 25];
                    let labs: Vec<String> = mins.iter().map(|m| format!("{m} min")).collect();
                    let gap = ui.dp(8.0);
                    let ws: Vec<f32> = labs.iter().map(|l| ui.text_w(l, 13.0, false) + ui.dp(26.0)).collect();
                    let mut rows: Vec<Vec<usize>> = vec![vec![]];
                    let mut acc = 0.0;
                    for (i, w) in ws.iter().enumerate() {
                        if acc + w > ui.w - 2.0 * m && !rows.last().unwrap().is_empty() {
                            rows.push(vec![]);
                            acc = 0.0;
                        }
                        rows.last_mut().unwrap().push(i);
                        acc += w + gap;
                    }
                    let mut yy = by;
                    for row in rows {
                        let tw: f32 = row.iter().map(|i| ws[*i]).sum::<f32>() + gap * (row.len() as f32 - 1.0);
                        let mut x = (ui.w - tw) / 2.0;
                        for i in row {
                            let (_, t) = ui.pill(x, yy, &labs[i], false);
                            if t {
                                self.tl.timer_total = mins[i] * 60;
                                self.tl.timer_paused = None;
                                self.tl.timer_end = Some(Instant::now() + Duration::from_secs(mins[i] * 60));
                                ui.dirty = true;
                            }
                            x += ws[i] + gap;
                        }
                        yy += ui.dp(44.0);
                    }
                }
            }
            _ => {
                // Stoppuhr: große Zeit, Start/Stopp und Runde/Zurücksetzen, Rundenliste
                let el = self.tl.sw_acc + self.tl.sw_start.map(|s| s.elapsed()).unwrap_or_default();
                let fmt = |d: Duration| {
                    let t = d.as_millis();
                    format!("{:02}:{:02}.{:02}", t / 60000, (t / 1000) % 60, (t / 10) % 100)
                };
                let mut y = body.y + ui.dp(30.0);
                let old = ui.rtl;
                ui.rtl = false;
                ui.label(Rect::new(0.0, y, ui.w, ui.dp(70.0)), &fmt(el), 58.0, c, false, Al::Center);
                ui.rtl = old;
                y += ui.dp(88.0);
                let running = self.tl.sw_start.is_some();
                let bw = ui.dp(130.0);
                let gap = ui.dp(12.0);
                let b1 = Rect::new(ui.w / 2.0 - bw - gap / 2.0, y, bw, ui.dp(46.0));
                let b2 = Rect::new(ui.w / 2.0 + gap / 2.0, y, bw, ui.dp(46.0));
                let (b1, b2) = if ui.rtl { (b2, b1) } else { (b1, b2) };
                if ui.button(b1, if running { self.t("Stop") } else { self.t("Start") }, Btn::Primary) {
                    match self.tl.sw_start.take() {
                        Some(s) => self.tl.sw_acc += s.elapsed(),
                        None => self.tl.sw_start = Some(Instant::now()),
                    }
                    ui.dirty = true;
                }
                if ui.button(b2, if running { self.t("Lap") } else { self.t("Reset") }, Btn::Plain) {
                    if running {
                        self.tl.sw_laps.insert(0, el);
                    } else {
                        self.tl.sw_start = None;
                        self.tl.sw_acc = Duration::ZERO;
                        self.tl.sw_laps.clear();
                    }
                    ui.dirty = true;
                }
                y += ui.dp(64.0);
                let n = self.tl.sw_laps.len();
                if n > 0 {
                    let lh = ui.dp(42.0);
                    let lr = Rect::new(m, y, ui.w - 2.0 * m, lh * n.min(8) as f32);
                    ui.card(lr);
                    for (i, l) in self.tl.sw_laps.clone().iter().take(8).enumerate() {
                        let r = Rect::new(m + ui.dp(14.0), y + i as f32 * lh, ui.w - 2.0 * m - ui.dp(28.0), lh);
                        if i > 0 {
                            ui.cv.rect(Rect::new(m + 1.0, r.y, ui.w - 2.0 * m - 2.0, 1.0), ui.th.line);
                        }
                        ui.label(r, &format!("{} {}", self.t("Lap"), n - i), 14.0, cs, false, Al::Start);
                        ui.label(r, &fmt(*l), 14.0, c, false, Al::End);
                    }
                }
            }
        }
    }

    fn files_refresh(&mut self) {
        let mut v: Vec<(String, bool, u64)> = std::fs::read_dir(&self.tl.cwd)
            .map(|rd| {
                rd.flatten()
                    .map(|e| {
                        let md = e.metadata().ok();
                        (e.file_name().to_string_lossy().into_owned(), md.as_ref().is_some_and(|m| m.is_dir()), md.map(|m| m.len()).unwrap_or(0))
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        v.truncate(500);
        self.tl.files = v;
        self.tl.files_for = self.tl.cwd.clone();
    }

    /// Dateien wie in der Web-Demo: Übersicht (Speicher, Schnellorte, Ordner, USB) und Ordneransicht
    pub(super) fn s_files(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        use super::app_modern::file_kind;
        use crate::ui::Ic;
        let cwd = self.tl.cwd.clone();
        let home = cwd == "/data";
        let title = if home { self.t("Files").to_string() } else { self.files_dname(&cwd) };
        let area = self.header(ui, area, &title);
        let can_write = !cwd.starts_with("/proc") && !cwd.starts_with("/sys");
        if std::mem::take(&mut self.hdr_tap) && can_write {
            let p = free_name(&cwd, self.t("New folder"));
            if let Err(e) = std::fs::create_dir(&p) {
                self.toast(e.to_string());
            }
            self.tl.files_for.clear();
            ui.dirty = true;
        }
        if self.tl.files_for != cwd {
            self.files_refresh();
            self.files_sort();
        }
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(16.0);
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if home {
            // Interner Speicher
            if let Some((total, free)) = sys::disk("/data").or_else(|| sys::disk("/")) {
                let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(66.0));
                ui.card(r);
                let inn = Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(14.0), r.w - ui.dp(28.0), ui.dp(20.0));
                ui.label(inn, self.t("Internal storage"), 14.0, c, false, Al::Start);
                let used = total.saturating_sub(free);
                ui.label(inn, &format!("{} / {}", sys::human(used), sys::human(total)), 14.0, cs, false, Al::End);
                let br = Rect::new(inn.x, inn.y + ui.dp(28.0), inn.w, ui.dp(6.0));
                ui.cv.rrect(br, ui.dp(3.0), ui.th.line);
                let f = if total > 0 { used as f32 / total as f32 } else { 0.0 };
                let fw = (br.w * f).max(ui.dp(6.0));
                let fr = if ui.rtl { Rect::new(br.right() - fw, br.y, fw, br.h) } else { Rect::new(br.x, br.y, fw, br.h) };
                ui.cv.rrect(fr, ui.dp(3.0), ui.th.accent);
                y += r.h + ui.dp(14.0);
            }
            // Schnellorte: Bilder, Dokumente, Downloads
            let quick: [(Ic, &str, &str); 3] = [(Ic::Image, "Pictures", "/data/Pictures"), (Ic::Doc, "Documents", "/data/Documents"), (Ic::Download, "Downloads", "/data/Download")];
            let gap = ui.dp(10.0);
            let cw = (ui.w - 2.0 * m - 2.0 * gap) / 3.0;
            for (k, (ic, key, path)) in quick.iter().enumerate() {
                let col = if ui.rtl { 2 - k } else { k };
                let r = Rect::new(m + col as f32 * (cw + gap), y, cw, ui.dp(78.0));
                let held = ui.held(r);
                ui.card_h(r, held);
                ui.icon(*ic, r.cx(), r.y + ui.dp(28.0), ui.dp(22.0), ui.th.accent);
                ui.label(Rect::new(r.x, r.y + ui.dp(48.0), r.w, ui.dp(18.0)), self.t(key), 13.0, c, false, Al::Center);
                if ui.tap(r) {
                    let _ = std::fs::create_dir_all(path);
                    self.tl.cwd = path.to_string();
                    *sc = 0.0;
                    ui.dirty = true;
                }
            }
            y += ui.dp(78.0) + ui.dp(14.0);
            // Ordner unter /data
            ui.kb.row_end.set(-1.0);
            for (name, is_dir, _) in self.tl.files.clone() {
                if !is_dir || name.starts_with('.') || name == "ultar" || name == "lost+found" {
                    continue;
                }
                let full = format!("/data/{name}");
                let n = sys::memo("files_counts", 3000, std::collections::HashMap::<String, usize>::new).get(&full).copied().unwrap_or_else(|| {
                    let n = std::fs::read_dir(&full).map(|rd| rd.take(1000).count()).unwrap_or(0);
                    let mut m = sys::memo("files_counts", 3000, std::collections::HashMap::<String, usize>::new);
                    m.insert(full.clone(), n);
                    sys::memo_set("files_counts", m);
                    n
                });
                let sub = format!("{n} {}", self.t("items"));
                let dn = self.files_dname(&full);
                if ui.row(&mut y, Some(Ic::Folder), &dn, Some(&sub), None, true) {
                    self.tl.cwd = full;
                    *sc = 0.0;
                    ui.dirty = true;
                }
            }
            let usb = sys::memo("usb_dir", 2000, || ["/media/usb", "/mnt/usb", "/run/media/usb"].iter().find(|p| std::path::Path::new(p).is_dir() && std::fs::read_dir(p).map(|mut d| d.next().is_some()).unwrap_or(false)).map(|s| s.to_string()));
            let sub = if usb.is_some() { self.t("connected") } else { self.t("not connected") };
            if ui.row(&mut y, Some(Ic::Usb), self.t("USB stick"), Some(sub), None, false) {
                if let Some(u) = usb {
                    self.tl.cwd = u;
                    *sc = 0.0;
                    ui.dirty = true;
                }
            }
            y += ui.dp(20.0);
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        // Ordneransicht: Einfügen-Karte, Hinweis, Liste
        if can_write {
            if let Some((src, mv)) = self.tl.clip.clone() {
                let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(56.0));
                ui.cv.rrect(r, ui.dp(20.0), ui.th.accent);
                ui.cv.rrect(r.inset(1.0), ui.dp(19.0), ui.th.card);
                let nm = std::path::Path::new(&src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let ix = if ui.rtl { r.right() - ui.dp(26.0) } else { r.x + ui.dp(26.0) };
                ui.icon(Ic::Copy, ix, r.cy(), ui.dp(18.0), c);
                let bw = ui.text_w(self.t("Paste here"), 13.5, true) + ui.dp(28.0);
                let br = Rect::new(if ui.rtl { r.x + ui.dp(10.0) } else { r.right() - ui.dp(10.0) - bw }, r.cy() - ui.dp(18.0), bw, ui.dp(36.0));
                let tx = Rect::new(if ui.rtl { br.right() + ui.dp(8.0) } else { r.x + ui.dp(48.0) }, r.y, r.w - bw - ui.dp(70.0), r.h);
                let nf = ui.fit(&nm, 13.0, false, tx.w);
                ui.label(tx, &nf, 13.0, c, false, Al::Start);
                if ui.button(br, self.t("Paste here"), Btn::Primary) {
                    match paste(&src, &cwd, mv) {
                        Ok(_) => {
                            if mv {
                                self.tl.clip = None;
                            }
                            self.toast(self.t("Done").into());
                        }
                        Err(e) => self.toast(e),
                    }
                    self.tl.files_for.clear();
                    self.gal.rescan();
                    ui.dirty = true;
                }
                y += r.h + ui.dp(10.0);
            }
        }
        let hint = self.t("Long-press to copy, move, rename, delete");
        let hf = ui.fit(hint, 12.0, false, ui.w - 2.0 * m - ui.dp(8.0));
        ui.label(Rect::new(m + ui.dp(4.0), y, ui.w - 2.0 * m - ui.dp(8.0), ui.dp(18.0)), &hf, 12.0, cs, false, Al::Start);
        y += ui.dp(26.0);
        ui.kb.row_end.set(-1.0);
        if self.tl.files.is_empty() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(100.0));
            ui.card(r);
            ui.label(r, self.t("Empty"), 14.0, cs, false, Al::Center);
            y += r.h;
        }
        for (name, is_dir, size) in self.tl.files.clone() {
            let full = if cwd == "/" { format!("/{name}") } else { format!("{cwd}/{name}") };
            let y0 = y;
            let (ic, _) = file_kind(&name, is_dir);
            let sub = if is_dir { self.t("Folder").to_string() } else { sys::human(size) };
            let r = Rect::new(m, y0, ui.w - 2.0 * m, ui.dp(62.0));
            let lp = ui.visible(r) && self.long_press(ui, r);
            let tapped = ui.row(&mut y, Some(ic), &name, Some(&sub), None, false);
            if lp {
                self.tl.menu = Some((full.clone(), is_dir));
                self.tl.del_armed = false;
                ui.dirty = true;
            } else if tapped {
                if is_dir {
                    self.tl.cwd = full;
                    *sc = 0.0;
                } else if installable(&full) {
                    self.install_file(&full);
                } else if is_image(&full) || crate::mjpeg::is_video(&full) {
                    let sib: Vec<String> = self.tl.files.iter().filter(|f| !f.1 && (is_image(&f.0) || crate::mjpeg::is_video(&f.0))).map(|f| if cwd == "/" { format!("/{}", f.0) } else { format!("{cwd}/{}", f.0) }).collect();
                    self.open_photo(full, sib);
                } else if let Some(s) = Self::office_screen_for(&full) {
                    // PDF, Word, Excel, Text/Code → eigene Büro-App
                    self.go(s);
                } else {
                    self.tl.del_armed = false;
                    self.go(Screen::Viewer(full));
                }
                ui.dirty = true;
            }
        }
        y += ui.dp(20.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// App-Datei installieren (alle Formate über uapp). Windows-Installer zeigen Fenster: sie laufen im
    /// Fenstersystem des Ubuntu-Bereichs, wenn es da ist; alles andere als Hintergrundauftrag mit Protokoll.
    pub(super) fn install_file(&mut self, path: &str) {
        // erst prüfen, ob es so etwas schon gibt: verknüpfen statt doppelt installieren (Entscheidung bleibt beim Nutzer)
        if !self.job_running() && super::app_zellen::empfehlung(path).is_some() {
            crate::sys::memo_clear();
            self.go(Screen::Empfehlung(path.to_string()));
            return;
        }
        self.install_file_direkt(path);
    }

    pub(super) fn install_file_direkt(&mut self, path: &str) {
        if self.job_running() {
            self.toast(self.t("Please wait").into());
            return;
        }
        let l = path.to_ascii_lowercase();
        let windows = l.ends_with(".exe") || l.ends_with(".msi");
        if windows && std::path::Path::new("/compat/usr/bin/weston").exists() && sys::has_drm() {
            self.launch = Some(format!("exec:uapp install '{}'", path.replace('\'', "'\\''")));
            self.extra.desk = None;
            return;
        }
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.job = Some(super::Job { title: format!("{}: {name}", self.t("Install")), lines: Vec::new(), done: None });
        self.bg.run("/usr/bin/uapp", vec!["install".into(), path.to_string()], sys::Msg::JobLine, sys::Msg::JobDone);
        self.extra.desk = None; // Startliste neu laden
        self.go(Screen::Job);
    }

    /// Anzeigename eines Ordners wie in der Demo (übersetzt für die Standardordner)
    pub(super) fn files_dname(&self, path: &str) -> String {
        let n = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        match n.as_str() {
            "Pictures" => self.t("Pictures").into(),
            "Documents" => self.t("Documents").into(),
            "Download" | "Downloads" => "Downloads".into(),
            "Music" => self.t("Music").into(),
            "DCIM" => self.t("Camera").into(),
            "home" => "Ubuntu (/home)".into(),
            _ => n,
        }
    }

    /// Ordner zuerst, dann nach gewählter Sortierung.
    fn files_sort(&mut self) {
        let cwd = self.tl.cwd.clone();
        let mtime = |n: &str| std::fs::metadata(format!("{cwd}/{n}")).and_then(|m| m.modified()).ok();
        match self.tl.fsort {
            1 => self.tl.files.sort_by(|a, b| b.1.cmp(&a.1).then(mtime(&b.0).cmp(&mtime(&a.0)))),
            2 => self.tl.files.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2))),
            _ => self.tl.files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))),
        }
    }

    pub(super) fn s_viewer(&mut self, ui: &mut Ui, area: Rect, path: &str, sc: &mut f32) {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let area = self.header(ui, area, &name);
        let bh = ui.dp(52.0);
        let body = Rect::new(ui.dp(12.0), area.y, ui.w - ui.dp(24.0), (area.h - bh - ui.dp(20.0)).max(0.0));
        if is_image(path) {
            if self.tl.view.as_ref().map(|v| v.0.as_str()) != Some(path) {
                let max = (ui.w.max(ui.h) as u32).max(480);
                self.tl.view = Some((path.to_string(), decode(path, max)));
            }
            match &self.tl.view.as_ref().unwrap().1 {
                Ok(p) => {
                    let k = (body.w / p.w as f32).min(body.h / p.h as f32);
                    let (w, h) = (p.w as f32 * k, p.h as f32 * k);
                    ui.cv.blit_rgba(&p.rgba, p.w, p.h, Rect::new(body.x + (body.w - w) / 2.0, body.y + (body.h - h) / 2.0, w, h));
                }
                Err(e) => {
                    let (e, c) = (e.clone(), ui.th.danger);
                    ui.label(body, &e, 15.0, c, false, Al::Center);
                }
            }
        } else {
            if self.tl.text.as_ref().map(|v| v.0.as_str()) != Some(path) {
                let lines = match std::fs::read(path) {
                    Ok(b) if b.len() > 256 * 1024 => vec![self.t("File too large to display").to_string()],
                    Ok(b) if b.iter().take(4096).any(|&x| x == 0) => vec![self.t("Not a text file").to_string()],
                    Ok(b) => String::from_utf8_lossy(&b).lines().take(2000).map(String::from).collect(),
                    Err(e) => vec![e.to_string()],
                };
                self.tl.text = Some((path.to_string(), lines));
            }
            ui.card(body);
            let inner = body.inset(ui.dp(12.0));
            ui.scroll_begin(inner, sc);
            let mut y = inner.y - *sc;
            let c = ui.th.text;
            for l in self.tl.text.as_ref().unwrap().1.clone() {
                y = ui.para(inner.x, y, inner.w, if l.is_empty() { " " } else { &l }, 13.0, c);
            }
            ui.scroll_end(inner, y + *sc - inner.y, sc);
            ui.set_clip(None);
        }
        // Löschen nur im Nutzerbereich, mit zweitem Tippen zur Bestätigung
        if path.starts_with("/data/") {
            let label = if self.tl.del_armed { self.t("Tap again to delete") } else { self.t("Delete") };
            if ui.button(Rect::new(ui.dp(12.0), area.bottom() - bh - ui.dp(8.0), ui.w - ui.dp(24.0), bh), label, Btn::Danger) {
                if self.tl.del_armed {
                    let msg = match std::fs::remove_file(path) {
                        Ok(()) => self.t("Deleted").to_string(),
                        Err(e) => e.to_string(),
                    };
                    self.tl.del_armed = false;
                    self.tl.files_for.clear();
                    self.gal.rescan();
                    self.back();
                    self.toast(msg);
                } else {
                    self.tl.del_armed = true;
                }
                ui.dirty = true;
            }
        }
    }

    /// Datenschutz-Zentrum wie in der Web-Demo: Hauptkarte mit Schalter, zwei Zahlen, Einzelschalter, Schutz
    pub(super) fn s_privacy(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        use crate::privacy::{self, Cap};
        use crate::ui::Ic;
        let area = self.header(ui, area, self.t("Privacy center"));
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let all = self.set.priv_all;
        // Hauptkarte
        let hr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(96.0));
        let tone = if all { ui.th.ok } else { ui.th.danger };
        ui.cv.rrect(hr, ui.dp(20.0), if all { ui.th.line } else { ui.th.danger });
        ui.cv.rrect(hr.inset(1.0), ui.dp(19.0), ui.th.card);
        let bx = if ui.rtl { hr.right() - ui.dp(18.0) - ui.dp(56.0) } else { hr.x + ui.dp(18.0) };
        let br = Rect::new(bx, hr.cy() - ui.dp(28.0), ui.dp(56.0), ui.dp(56.0));
        ui.cv.rrect(br, ui.dp(18.0), tone.alpha(0.18));
        ui.icon(Ic::Shield, br.cx(), br.cy(), ui.dp(28.0), tone);
        let (tw, th) = (ui.dp(44.0), ui.dp(26.0));
        let tx = if ui.rtl { hr.x + ui.dp(18.0) } else { hr.right() - ui.dp(18.0) - tw };
        let tr = Rect::new(tx, hr.cy() - th / 2.0, tw, th);
        ui.cv.rrect(tr, th / 2.0, if all { ui.th.accent } else { ui.th.line.mix(cs, 0.25) });
        let kx = if all != ui.rtl { tr.right() - th / 2.0 } else { tr.x + th / 2.0 };
        ui.cv.circle(kx, tr.cy(), th / 2.0 - ui.dp(3.0), if all { ui.th.on_accent } else { cs });
        let txt_x = if ui.rtl { tr.right() + ui.dp(12.0) } else { br.right() + ui.dp(14.0) };
        let txt_w = hr.w - ui.dp(56.0) - tw - ui.dp(36.0) - ui.dp(26.0);
        ui.label(Rect::new(txt_x, hr.y + ui.dp(16.0), txt_w, ui.dp(22.0)), self.t("Device active"), 16.0, c, true, Al::Start);
        let desc = if all { self.t("One tap turns off network, camera, mic, location and sensors.") } else { self.t("Everything blocked – nothing can send or record.") };
        ui.para(txt_x, hr.y + ui.dp(40.0), txt_w, desc, 12.5, cs);
        if ui.tap(hr) {
            self.set.priv_all = !all;
            self.save();
            privacy::apply(&self.set);
            ui.dirty = true;
        }
        y += hr.h + ui.dp(14.0);
        // Zwei Zahlen
        let gap = ui.dp(10.0);
        let cw = (ui.w - 2.0 * m - gap) / 2.0;
        let stats = [(self.t("Blocked servers"), self.tl.blocked.to_string()), (self.t("Links checked"), self.buddy_links_checked().to_string())];
        for (k, (lab, val)) in stats.iter().enumerate() {
            let col = if ui.rtl { 1 - k } else { k };
            let r = Rect::new(m + col as f32 * (cw + gap), y, cw, ui.dp(76.0));
            ui.card(r);
            ui.label(Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(12.0), r.w - ui.dp(28.0), ui.dp(18.0)), lab, 12.0, cs, false, Al::Start);
            let old = ui.rtl;
            ui.rtl = false;
            ui.label(Rect::new(r.x + ui.dp(14.0), r.y + ui.dp(34.0), r.w - ui.dp(28.0), ui.dp(32.0)), val, 26.0, c, true, if old { Al::End } else { Al::Start });
            ui.rtl = old;
        }
        y += ui.dp(76.0) + ui.dp(8.0);
        // Einzeln schalten
        ui.section(&mut y, self.t("Individual switches"));
        ui.kb.row_end.set(-1.0);
        let groups: [(Cap, Ic, &str, &str); 6] = [
            (Cap::Network, Ic::Globe, "Network", "Wi-Fi · mobile"),
            (Cap::Camera, Ic::Camera, "Camera", ""),
            (Cap::Microphone, Ic::Mic, "Microphone", ""),
            (Cap::Location, Ic::Globe, "Location", "GPS"),
            (Cap::Sensors, Ic::Sliders, "Sensors", "motion, light, proximity"),
            (Cap::Bluetooth, Ic::Bt, "Bluetooth", ""),
        ];
        for (cap, ic, title, sub) in groups {
            let mut on = match cap {
                Cap::Network => self.set.priv_net,
                Cap::Camera => self.set.priv_cam,
                Cap::Microphone => self.set.priv_mic,
                Cap::Location => self.set.priv_loc,
                Cap::Sensors => self.set.priv_sensors,
                Cap::Bluetooth => self.set.priv_bt,
            };
            let old = ui.enabled;
            ui.enabled = old && all;
            ui.kb.row_ic.set(Some(ic));
            ui.kb.row_bg.set(Some(rgb(if all && on { 0x173A24 } else { 0x4A1D1D })));
            let sub_t = if sub.is_empty() { None } else { Some(self.t(sub)) };
            if ui.toggle_row(&mut y, self.t(title), sub_t, &mut on) {
                match cap {
                    Cap::Network => self.set.priv_net = on,
                    Cap::Camera => self.set.priv_cam = on,
                    Cap::Microphone => self.set.priv_mic = on,
                    Cap::Location => self.set.priv_loc = on,
                    Cap::Sensors => self.set.priv_sensors = on,
                    Cap::Bluetooth => self.set.priv_bt = on,
                }
                self.save();
                privacy::apply(&self.set);
            }
            ui.enabled = old;
        }
        y += ui.dp(14.0);
        // Schutz
        ui.section(&mut y, self.t("Protection"));
        ui.kb.row_end.set(-1.0);
        let mut on = self.set.adblock;
        let entries = format!("{} {}", self.tl.blocked, self.t("entries"));
        ui.kb.row_ic.set(Some(Ic::Shield));
        ui.kb.row_bg.set(Some(rgb(0x2A2D35)));
        if ui.toggle_row(&mut y, self.t("System-wide ad & tracker block"), Some(&entries), &mut on) {
            self.set.adblock = on;
            self.tl.blocked = adblock_apply(on);
            self.save();
        }
        ui.kb.row_bg.set(Some(rgb(0x4A421C)));
        if ui.row(&mut y, Some(Ic::Search), self.t("Check a link now"), None, None, true) {
            self.buddy_open_linkcheck();
            ui.dirty = true;
        }
        let online = sys::online() && sys::compat_ready();
        ui.kb.row_bg.set(Some(rgb(0x2A2D35)));
        let st = if self.job_running() { self.t("Running…") } else if online { "" } else { self.t("offline") };
        if ui.row(&mut y, Some(Ic::Download), self.t("Update block list"), None, Some(st), false) && online && !self.job_running() {
            self.start_job(self.t("Block list").to_string(), &["run", "curl", "-fsSL", "--retry", "2", "-o", "/var/tmp/ultar-blocklist.hosts", BLOCKLIST_URL]);
            self.tl.blocked = 0;
            ui.dirty = true;
        }
        ui.kb.row_bg.set(Some(rgb(0x2A2D35)));
        ui.row(&mut y, Some(Ic::Box), self.t("Android cage"), Some(self.t("planned · off by default")), Some(self.t("Off")), false);
        y += ui.dp(20.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    pub(super) fn s_sound(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Sound"));
        let mut y = area.y;
        let mut on = self.set.sound;
        if ui.toggle_row(&mut y, self.t("Sounds"), Some(self.t("Alarm, timer and notification tones")), &mut on) {
            self.set.sound = on;
            self.save();
        }
        ui.section(&mut y, self.t("Volume"));
        let mut v = self.set.volume;
        let sr = Rect::new(ui.dp(24.0), y, ui.w - ui.dp(48.0), ui.dp(44.0));
        if ui.slider("volume", sr, &mut v) {
            self.set.volume = v.clamp(0.0, 1.0);
            ui.dirty = true;
        }
        // Erst beim Loslassen sichern und anspielen
        if ui.p.released && (self.set.volume - self.tl.vol_saved).abs() > 0.001 {
            self.tl.vol_saved = self.set.volume;
            self.save();
            if self.set.sound {
                crate::audio::notify(self.set.volume);
            }
        }
        y += ui.dp(60.0);
        let has = crate::audio::available();
        let old = ui.enabled;
        ui.enabled = old && has;
        if ui.button(Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)), self.t("Play test tone"), Btn::Primary) {
            crate::audio::alarm(self.set.volume);
        }
        ui.enabled = old;
        y += ui.dp(64.0);
        if !has {
            self.caption(ui, &mut y, self.t("No sound device found"));
        }
    }

    /// Startseite für Browser und Web-Apps: prüft, ob der Browser installiert ist, und startet ihn im Vollbild.
    /// Eingabe der Browser-Startseite öffnen (Adresse oder Suche)
    pub(super) fn web_go(&mut self) {
        let q = self.f("web_q").trim().to_string();
        if q.is_empty() {
            return;
        }
        let url = if q.contains("://") { q } else if q.contains('.') && !q.contains(' ') { format!("https://{q}") } else { format!("https://duckduckgo.com/?q={}", q.replace(' ', "+")) };
        self.fields.insert("web_q", String::new());
        self.focus = None;
        self.focus_reset = true;
        self.web_open(&url);
    }

    fn web_open(&mut self, url: &str) {
        let ready = std::path::Path::new("/compat/usr/bin/weston").exists() && std::path::Path::new("/compat/usr/bin/epiphany").exists();
        if ready {
            self.launch = Some(url.to_string());
        } else {
            self.go(Screen::Web(url.to_string()));
        }
    }

    /// Browser-Startseite wie in der Web-Demo: Adressleiste, Logo, Suchfeld, Schnellzugriffe, Tracker-Karte
    fn web_start(&mut self, ui: &mut Ui, area: Rect) {
        use crate::ui::Ic;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let m = ui.dp(16.0);
        // Adressleiste
        let bar = Rect::new(0.0, area.y, ui.w, ui.dp(56.0));
        let bx = if ui.rtl { ui.w - ui.dp(28.0) } else { ui.dp(28.0) };
        if ui.icon_btn(bx, bar.cy(), ui.dp(18.0), Ic::Back, ui.th.card.alpha(0.0), c) {
            self.back();
            ui.dirty = true;
            return;
        }
        let pw = ui.w - ui.dp(56.0) - ui.dp(84.0);
        let pr = Rect::new(if ui.rtl { ui.dp(84.0) } else { ui.dp(52.0) }, bar.cy() - ui.dp(20.0), pw, ui.dp(40.0));
        ui.cv.rrect(pr, ui.dp(20.0), ui.th.press);
        let six = if ui.rtl { pr.right() - ui.dp(18.0) } else { pr.x + ui.dp(18.0) };
        ui.icon(Ic::Search, six, pr.cy(), ui.dp(13.0), cs);
        ui.label(Rect::new(if ui.rtl { pr.x + ui.dp(40.0) } else { pr.x + ui.dp(34.0) }, pr.y, pr.w - ui.dp(74.0), pr.h), self.t("Search or address"), 13.5, c, false, Al::Start);
        let shx = if ui.rtl { pr.x + ui.dp(20.0) } else { pr.right() - ui.dp(20.0) };
        ui.icon(Ic::Shield, shx, pr.cy(), ui.dp(12.0), ui.th.accent);
        if ui.tap(pr) {
            self.focus = Some("web_q");
            self.focus_reset = true;
            ui.dirty = true;
        }
        let tx = if ui.rtl { ui.dp(56.0) } else { ui.w - ui.dp(56.0) };
        let tr = Rect::new(tx - ui.dp(11.0), bar.cy() - ui.dp(11.0), ui.dp(22.0), ui.dp(22.0));
        ui.cv.rrect(tr, ui.dp(6.0), c);
        ui.cv.rrect(tr.inset(ui.dp(1.6)), ui.dp(4.5), ui.th.bg);
        ui.label(tr, "1", 11.0, c, true, Al::Center);
        let mx = if ui.rtl { ui.dp(24.0) } else { ui.w - ui.dp(24.0) };
        for k in -1..=1 {
            ui.cv.circle(mx, bar.cy() + k as f32 * ui.dp(5.0), ui.dp(1.6), c);
        }
        let mut y = bar.bottom() + ui.dp(30.0);
        // Logo
        let lg = crate::brand::logo();
        let lw = ui.dp(150.0);
        let lh = lw * lg.h as f32 / lg.w.max(1) as f32;
        ui.cv.image(&lg, (ui.w - lw) / 2.0, y, lw, lh);
        y += lh + ui.dp(28.0);
        // Suchfeld
        let fr = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0));
        ui.field(fr, "web_q", &self.f("web_q"), self.t("Search or enter address"));
        y += ui.dp(66.0);
        // Schnellzugriffe
        let tiles: [(Ic, &str, &str); 8] = [
            (Ic::Globe, "code-craft", "https://code-craft.tech/"),
            (Ic::Doc, "wikipedia", "https://wikipedia.org/"),
            (Ic::Play, "youtube", "https://m.youtube.com/"),
            (Ic::Chat, "web.whatsapp", "https://web.whatsapp.com/"),
            (Ic::Globe, "maps.code-craft", "https://maps.code-craft.tech/"),
            (Ic::Note, "news", "https://news.google.com/"),
            (Ic::Box, "paket", "https://www.dhl.de/"),
            (Ic::Term, "github", "https://github.com/"),
        ];
        let colw = (ui.w - 2.0 * m) / 4.0;
        for (i, (ic, lab, u)) in tiles.iter().enumerate() {
            let col = if ui.rtl { 3 - i % 4 } else { i % 4 };
            let cx = m + col as f32 * colw + colw / 2.0;
            let cy = y + (i / 4) as f32 * ui.dp(84.0) + ui.dp(24.0);
            let r = Rect::new(cx - ui.dp(24.0), cy - ui.dp(24.0), ui.dp(48.0), ui.dp(48.0));
            let held = ui.held(r);
            ui.cv.circle(cx, cy, ui.dp(24.0), if held { ui.th.line } else { ui.th.press });
            ui.icon(*ic, cx, cy, ui.dp(18.0), c);
            let lf = ui.fit(lab, 11.5, false, colw - ui.dp(4.0));
            ui.label(Rect::new(cx - colw / 2.0, cy + ui.dp(30.0), colw, ui.dp(16.0)), &lf, 11.5, cs, false, Al::Center);
            if ui.tap(r) {
                self.web_open(u);
                ui.dirty = true;
            }
        }
        y += 2.0 * ui.dp(84.0) + ui.dp(10.0);
        // Tracker-Karte
        let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(64.0));
        ui.card(r);
        let ix = if ui.rtl { r.right() - ui.dp(28.0) } else { r.x + ui.dp(28.0) };
        ui.icon(Ic::Shield, ix, r.cy(), ui.dp(20.0), ui.th.accent);
        let tx2 = Rect::new(if ui.rtl { r.x + ui.dp(14.0) } else { r.x + ui.dp(54.0) }, r.y, r.w - ui.dp(68.0), r.h);
        let t1 = format!("{} {}", self.tl.blocked, self.t("tracker servers blocked"));
        ui.label(Rect::new(tx2.x, r.y + ui.dp(12.0), tx2.w, ui.dp(22.0)), &t1, 15.0, c, true, Al::Start);
        let t2 = ui.fit(self.t("No ads, no tracking – on every page."), 12.5, false, tx2.w);
        ui.label(Rect::new(tx2.x, r.y + ui.dp(35.0), tx2.w, ui.dp(18.0)), &t2, 12.5, cs, false, Al::Start);
    }

    pub(super) fn s_web(&mut self, ui: &mut Ui, area: Rect, url: &str) {
        if url == "about:blank" {
            self.web_start(ui, area);
            return;
        }
        let area = self.header(ui, area, self.t("Browser"));
        let mut y = area.y;
        let ready = std::path::Path::new("/compat/usr/bin/weston").exists() && std::path::Path::new("/compat/usr/bin/epiphany").exists();
        let r = Rect::new(ui.dp(16.0), y + ui.dp(8.0), ui.w - ui.dp(32.0), ui.dp(52.0));
        if ready {
            if url != "about:blank" {
                self.caption(ui, &mut y, url);
            }
            let r = Rect::new(ui.dp(16.0), y + ui.dp(8.0), ui.w - ui.dp(32.0), ui.dp(52.0));
            if ui.button(r, self.t("Open"), Btn::Primary) {
                self.launch = Some(url.to_string());
                ui.dirty = true;
            }
            y = r.bottom() + ui.dp(16.0);
            self.caption(ui, &mut y, self.t("The browser takes over the whole screen."));
            self.caption(ui, &mut y, self.t("Back to Ultar: press the power button."));
            self.caption(ui, &mut y, self.t("Typing needs a keyboard for now."));
        } else if !sys::compat_ready() {
            self.caption(ui, &mut y, self.t("Ubuntu area not available"));
        } else {
            self.caption(ui, &mut y, self.t("The browser is not installed yet."));
            let old = ui.enabled;
            ui.enabled = old && sys::online() && !self.job_running();
            let r = Rect::new(r.x, y + ui.dp(8.0), r.w, r.h);
            if ui.button(r, self.t("Install browser"), Btn::Primary) {
                self.start_job(self.t("Install browser").to_string(), &["apt", "install", "--no-install-recommends", "weston", "epiphany-browser", "udev", "dbus", "glib-networking", "adwaita-icon-theme", "fonts-noto-core", "libgl1-mesa-dri", "libegl1", "gstreamer1.0-plugins-good", "gstreamer1.0-libav", "gsettings-desktop-schemas", "dconf-gsettings-backend", "libglib2.0-bin", "librsvg2-common"]);
                ui.dirty = true;
            }
            ui.enabled = old;
            if !sys::online() {
                let mut y2 = r.bottom() + ui.dp(8.0);
                self.caption(ui, &mut y2, self.t("No internet connection"));
            }
        }
    }

    /// Kontextmenü eines Dateieintrags (nach langem Druck)
    pub(super) fn d_file_menu(&mut self, ui: &mut Ui) {
        let Some((path, is_dir)) = self.tl.menu.clone() else { return };
        ui.set_clip(None);
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0).alpha(0.55));
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let can_write = writable(&path);
        let mut items: Vec<(&'static str, Btn, u8)> = vec![("Open", Btn::Primary, 0)];
        if !is_dir {
            // dem Assistenten bringen (Foto ansehen, Dokument lesen …)
            items.push(("Give to assistant", Btn::Plain, 6));
        }
        items.push(("Copy", Btn::Plain, 1));
        if can_write {
            items.push(("Move", Btn::Plain, 2));
            items.push(("Rename", Btn::Plain, 3));
            items.push((if self.tl.del_armed { "Tap again to delete" } else { "Delete" }, Btn::Danger, 4));
        }
        items.push(("Cancel", Btn::Plain, 5));
        let bh = ui.dp(50.0);
        let w = (ui.w - ui.dp(48.0)).min(ui.dp(420.0));
        let h = ui.dp(64.0) + items.len() as f32 * (bh + ui.dp(8.0)) + ui.dp(8.0);
        let r = Rect::new((ui.w - w) / 2.0, ((ui.h - h) / 2.0).max(0.0), w, h);
        ui.card(r);
        let c = ui.th.text;
        ui.label(Rect::new(r.x + ui.dp(16.0), r.y + ui.dp(12.0), r.w - ui.dp(32.0), ui.dp(34.0)), &name, 18.0, c, true, Al::Center);
        let old = ui.enabled;
        ui.enabled = true;
        let mut y = r.y + ui.dp(56.0);
        let mut close = false;
        for (label, kind, id) in items {
            if ui.button(Rect::new(r.x + ui.dp(16.0), y, r.w - ui.dp(32.0), bh), self.t(label), kind) {
                match id {
                    0 => {
                        if is_dir {
                            self.tl.cwd = path.clone();
                        } else if is_image(&path) || crate::mjpeg::is_video(&path) {
                            self.open_photo(path.clone(), Vec::new());
                        } else if let Some(s) = Self::office_screen_for(&path) {
                            self.go(s);
                        } else {
                            self.go(Screen::Viewer(path.clone()));
                        }
                        close = true;
                    }
                    6 => {
                        self.jv.attach.push(path.clone());
                        self.go(Screen::Agent);
                        close = true;
                    }
                    1 | 2 => {
                        self.tl.clip = Some((path.clone(), id == 2));
                        self.toast(self.t("Now open the target folder and tap Paste").into());
                        close = true;
                    }
                    3 => {
                        self.fields.insert("fname", name.clone());
                        self.go(Screen::Rename(path.clone()));
                        close = true;
                    }
                    4 => {
                        if self.tl.del_armed {
                            let p = std::path::Path::new(&path);
                            let res = if is_dir { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                            self.toast(match res {
                                Ok(()) => self.t("Deleted").to_string(),
                                Err(e) => e.to_string(),
                            });
                            self.tl.files_for.clear();
                            self.gal.rescan();
                            close = true;
                        } else {
                            self.tl.del_armed = true;
                        }
                    }
                    _ => close = true,
                }
                ui.dirty = true;
            }
            y += bh + ui.dp(8.0);
        }
        ui.enabled = old;
        if close {
            self.tl.menu = None;
            self.tl.del_armed = false;
        }
    }

    pub(super) fn s_rename(&mut self, ui: &mut Ui, area: Rect, path: &str) {
        let area = self.header(ui, area, self.t("Rename"));
        let mut y = area.y;
        self.caption(ui, &mut y, path);
        ui.field(Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0)), "fname", &self.f("fname"), self.t("New name"));
        y += ui.dp(64.0);
        if ui.button(Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)), self.t("Apply"), Btn::Primary) {
            match rename(path, &self.f("fname")) {
                Ok(_) => {
                    self.tl.files_for.clear();
                    self.gal.rescan();
                    self.focus = None;
                    self.back();
                    self.back();
                    self.toast(self.t("Done").into());
                }
                Err(e) => self.toast(e),
            }
            ui.dirty = true;
        }
    }

    /// Langer Druck (≥ 0,55 s ohne Bewegung) auf eine Fläche. Der folgende Loslass-Tipp wird unterdrückt.
    pub(super) fn long_press(&mut self, ui: &mut Ui, r: Rect) -> bool {
        if !ui.enabled || !ui.p.down || ui.p.moved || self.lp_fired || !r.contains(ui.p.px, ui.p.py) || !ui.visible(r) {
            return false;
        }
        if self.press_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(550)) {
            self.lp_fired = true;
            ui.p.moved = true; // kein Tipp beim Loslassen
            return true;
        }
        false
    }

    /// App-Start vom Startbildschirm mitzählen (Grundlage für „Häufig genutzt“).
    pub(super) fn count_launch(&mut self, name: &str) {
        *self.tl.launches.count.entry(name.to_string()).or_default() += 1;
        let _ = sys::save("launches.toml", &self.tl.launches);
    }

    /// Die bis zu vier meistgenutzten Apps (ab drei Starts).
    pub(super) fn frequent(&self) -> Vec<String> {
        let mut v: Vec<(&String, &u32)> = self.tl.launches.count.iter().filter(|(_, c)| **c >= 3).collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        v.into_iter().take(4).map(|(n, _)| n.clone()).collect()
    }

    /// Nach einem Listen-Download die Sperre mit der neuen Liste setzen.
    pub(super) fn adblock_refresh(&mut self) {
        if self.set.adblock {
            self.tl.blocked = adblock_apply(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator() {
        assert_eq!(calc("1+2×3").unwrap(), 7.0);
        assert_eq!(calc("(1+2)×3").unwrap(), 9.0);
        assert_eq!(calc("−4+10÷4").unwrap(), -1.5);
        assert_eq!(calc("50%×200").unwrap(), 100.0);
        assert_eq!(calc("2×(3+(4−1))").unwrap(), 12.0);
        assert!(calc("1÷0").is_err());
        assert!(calc("2+").is_err());
        assert!(calc("(2+3").is_err());
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
        assert_eq!(fmt_num(12.0), "12");
    }

    #[test]
    fn file_operations() {
        let base = format!("/tmp/ultar-test-{}", std::process::id());
        // Schreibschutz außerhalb des Nutzerbereichs
        assert!(!writable("/etc/hosts") && !writable("/data/../etc/x") && writable("/data/a/b"));
        assert!(paste("/etc/hostname", &base, false).is_err());
        assert!(rename("/etc/hostname", "x").is_err());
        assert!(rename("/data/x", "a/b").is_err() && rename("/data/x", "..").is_err());
        // freie Namen
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(format!("{base}/a.txt"), "1").unwrap();
        assert_eq!(free_name(&base, "b.txt"), format!("{base}/b.txt"));
        assert_eq!(free_name(&base, "a.txt"), format!("{base}/a (2).txt"));
        // Baum kopieren
        std::fs::create_dir_all(format!("{base}/d/e")).unwrap();
        std::fs::write(format!("{base}/d/e/f.txt"), "inhalt").unwrap();
        let mut budget = 100;
        copy_tree(std::path::Path::new(&format!("{base}/d")), std::path::Path::new(&format!("{base}/kopie")), &mut budget).unwrap();
        assert_eq!(std::fs::read_to_string(format!("{base}/kopie/e/f.txt")).unwrap(), "inhalt");
        let mut none = 0;
        assert!(copy_tree(std::path::Path::new(&format!("{base}/d")), std::path::Path::new(&format!("{base}/k2")), &mut none).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn image_names() {
        assert!(is_image("/a/B.JPG") && is_image("x.png") && !is_image("x.txt"));
    }
}
