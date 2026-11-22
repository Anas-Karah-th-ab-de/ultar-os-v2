//! Zweite Runde Systemfunktionen (Untermodul von `app`): Werkzeuge (Kompass, Wasserwaage, Qibla, Gebetszeiten,
//! Rechner, Kennwörter, Zähler, Licht/SOS, Fokus, Aufgaben, Erinnerungen, Notfall), Übersicht der letzten Apps,
//! Anheften, Zwischenablage, Akku-Verlauf, Nutzungszeit, Gerätepflege, Speicher-Analyse, laufende Programme,
//! Modi (Schlafen, automatisches Design, geplanter Neustart …) und Sperrliste. Logik: `crate::sysy`.

use super::sensors::{self, Kind};
use super::{App, Screen};
use crate::gfx::{rgb, Color, Rect};
use crate::quick;
use crate::sys;
use crate::sysx;
use crate::sysy::{self, MoreCfg, Stats};
use crate::ui::{Al, Btn, Ic, Ui};
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Debug)]
pub enum Mx {
    Tools,
    Compass,
    Level,
    Qibla,
    Prayer,
    Password,
    Dice,
    Percent,
    Loan,
    DateCalc,
    Bases,
    Currency,
    Counter,
    BigText,
    Light,
    Sos,
    Focus,
    Countdown,
    Todo,
    Reminders,
    Emergency,
    Recents,
    Clipboard,
    BattHist,
    Usage,
    Care,
    StorageX,
    Tasks,
    Modes,
    Blocked,
    /// Hijri-Datum (wie in der Web-Demo)
    Hijri,
}

pub struct MxState {
    pub cfg: MoreCfg,
    pub stats: Stats,
    last_draw: Option<Instant>,
    cur_app: String,
    pub recents: Vec<(String, Screen)>,
    pub pinned: bool,
    pub to_top: bool,
    pub caffeine: Option<Instant>,
    sos: Option<Instant>,
    /// Fokus-Timer: (Ende, Arbeitsphase?)
    pub focus: Option<(Instant, bool)>,
    focus_prev_dnd: bool,
    shot_at: Option<Instant>,
    want_shot: bool,
    dice: Vec<i64>,
    pw: String,
    pw_opts: (usize, bool, bool, bool),
    big: Option<Vec<(String, u64, bool)>>,
    procs: Vec<(u32, String, u64)>,
    procs_at: Option<Instant>,
    last_min: i64,
    on_since: Instant,
    break_warned: bool,
    was_asleep: bool,
    was_locked: bool,
    charge_warned: bool,
    sens: (Option<Instant>, Option<[f32; 3]>, Option<[f32; 3]>),
    unsaved: u32,
    usage_tab: usize,
}

impl MxState {
    pub fn load() -> Self {
        Self {
            cfg: sys::load("more.toml"),
            stats: sys::load("stats.toml"),
            last_draw: None,
            cur_app: String::new(),
            recents: Vec::new(),
            pinned: false,
            to_top: false,
            caffeine: None,
            sos: None,
            focus: None,
            focus_prev_dnd: false,
            shot_at: None,
            want_shot: false,
            dice: Vec::new(),
            pw: String::new(),
            pw_opts: (16, true, true, true),
            big: None,
            procs: Vec::new(),
            procs_at: None,
            last_min: 0,
            on_since: Instant::now(),
            break_warned: false,
            was_asleep: false,
            was_locked: false,
            charge_warned: false,
            sens: (None, None, None),
            unsaved: 0,
            usage_tab: 0,
        }
    }
}

/// Werkzeuge der Übersicht: (Symbol, Name, Ziel)
const TOOLS: [(Ic, &str, Mx); 25] = [
    // Reihenfolge wie in der Web-Demo, danach die weiteren Werkzeuge
    (Ic::Svg("compass"), "Compass", Mx::Compass),
    (Ic::Svg("level"), "Spirit level", Mx::Level),
    (Ic::Svg("kaaba"), "Qibla", Mx::Qibla),
    (Ic::Svg("mosque"), "Prayer times", Mx::Prayer),
    (Ic::Svg("cal"), "Hijri date", Mx::Hijri),
    (Ic::Svg("sos2"), "SOS light", Mx::Sos),
    (Ic::Svg("focus"), "Focus timer", Mx::Focus),
    (Ic::Svg("list"), "Tasks", Mx::Todo),
    (Ic::Svg("torch"), "Torch", Mx::Light),
    (Ic::Svg("heart"), "Emergency info", Mx::Emergency),
    (Ic::Svg("pulse"), "Screen time", Mx::Usage),
    (Ic::Svg("layers"), "Modes", Mx::Modes),
    (Ic::Svg("key"), "Password", Mx::Password),
    (Ic::Grid, "Dice", Mx::Dice),
    (Ic::Calc, "Percent", Mx::Percent),
    (Ic::Calc, "Loan", Mx::Loan),
    (Ic::Svg("cal"), "Dates", Mx::DateCalc),
    (Ic::Term, "Hex/Bin", Mx::Bases),
    (Ic::Svg("data"), "Currency", Mx::Currency),
    (Ic::Plus, "Counter", Mx::Counter),
    (Ic::Svg("type"), "Big text", Mx::BigText),
    (Ic::Svg("timer"), "Countdown", Mx::Countdown),
    (Ic::Bell, "Reminders", Mx::Reminders),
    (Ic::Svg("paste"), "Clip", Mx::Clipboard),
    (Ic::Shield, "Blocked", Mx::Blocked),
];

fn day_key(d: &sys::DateTime) -> String {
    format!("{:04}-{:02}-{:02}", d.y, d.mo, d.d)
}

impl App {
    fn mx_save(&mut self) {
        let _ = sys::save("more.toml", &self.mx.cfg);
    }

    /// Übersetzter Name einer App aus ihrem gespeicherten Schlüssel
    fn app_label(&self, name: &str) -> String {
        match name {
            "Home" => self.t("Home screen").to_string(),
            "Tools" => self.t("Tools").to_string(),
            "Mobile network" => self.t("Mobile network").to_string(),
            n => self
                .all_apps()
                .iter()
                .find(|a| a.1 == n)
                .map(|a| self.t(a.1).to_string())
                .unwrap_or_else(|| self.t("Settings").to_string()),
        }
    }

    fn mx_dt(&self) -> sys::DateTime {
        sys::from_unix(sys::unix_now(), self.set.tz_min)
    }

    fn mx_num(&self, id: &str) -> Option<f64> {
        let s = self.f(id).replace(',', ".").replace(' ', "");
        s.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    /// In die Zwischenablage legen (mit Verlauf)
    pub(super) fn copy_text(&mut self, s: &str) {
        sysy::clip_push(&mut self.mx.cfg.clip, s);
        self.mx_save();
        self.toast(self.t("Copied").into());
    }

    pub(super) fn more_blocked(&self, number: &str) -> bool {
        self.mx
            .cfg
            .blocked
            .iter()
            .any(|b| super::app_modern::same_number(b, number))
    }

    pub(super) fn caffeine_on(&self) -> bool {
        self.mx.caffeine.is_some_and(|t| t > Instant::now())
    }

    pub fn take_shot(&mut self) -> bool {
        std::mem::take(&mut self.mx.want_shot)
    }

    /// Taschen-Schutz: Bildschirm bei einer Benachrichtigung nicht wecken, wenn der Näherungssensor verdeckt ist.
    pub fn pocket_block(&self) -> bool {
        self.mx.cfg.pocket
            && !self.call_active()
            && self.tl.ringing.is_none()
            && sys::proximity_near() == Some(true)
    }

    fn sensor(&mut self) -> (Option<[f32; 3]>, Option<[f32; 3]>) {
        if !(self.set.priv_all && self.set.priv_sensors) {
            return (None, None);
        }
        if self
            .mx
            .sens
            .0
            .is_none_or(|t| t.elapsed() > Duration::from_millis(120))
        {
            let all = sensors::read_all();
            let get = |k: Kind| {
                all.iter()
                    .find(|s| s.kind == k && s.values.len() >= 3)
                    .map(|s| [s.values[0], s.values[1], s.values[2]])
            };
            self.mx.sens = (Some(Instant::now()), get(Kind::Magnetic), get(Kind::Accel));
        }
        (self.mx.sens.1, self.mx.sens.2)
    }

    pub(super) fn more_open(&mut self, m: &Mx) {
        match m {
            Mx::Password => self.mx_new_password(),
            Mx::StorageX => self.mx.big = None,
            Mx::Tasks => self.mx.procs_at = None,
            Mx::Modes => {
                self.fields
                    .insert("mx_owner", self.mx.cfg.owner_info.clone());
            }
            Mx::Emergency => {
                self.fields
                    .insert("mx_enum", self.mx.cfg.emergency_number.clone());
                self.fields
                    .insert("mx_econ", self.mx.cfg.emergency_contact.clone());
                self.fields.insert("mx_med", self.mx.cfg.medical.clone());
            }
            Mx::Currency => {
                self.fields.insert("mx_cur_v", "1".into());
            }
            _ => {}
        }
    }

    /// Beim Öffnen eines Bildschirms: letzte Apps und Nutzungszeit führen.
    pub(super) fn more_on_go(&mut self, s: &Screen) {
        if let Screen::More(m) = s {
            self.more_open(m);
        }
        if self.stack.len() == 1 && !matches!(s, Screen::More(Mx::Recents)) {
            let name = self
                .all_apps()
                .into_iter()
                .find(|a| a.2.as_ref() == Some(s))
                .map(|a| a.1.to_string())
                .unwrap_or_else(|| match s {
                    Screen::More(_) => "Tools".into(),
                    Screen::Radio | Screen::Cells | Screen::Route => "Mobile network".into(),
                    _ => "Settings".into(),
                });
            self.mx.recents.retain(|r| r.0 != name);
            self.mx.recents.insert(0, (name.clone(), s.clone()));
            self.mx.recents.truncate(8);
            self.mx.cur_app = name;
        }
    }

    pub(super) fn more_needs_tick(&self) -> bool {
        self.mx.sos.is_some()
            || self.mx.shot_at.is_some()
            || self.mx.focus.is_some()
                && matches!(
                    self.stack.last().map(|s| &s.0),
                    Some(Screen::More(Mx::Focus))
                )
            || matches!(
                self.stack.last().map(|s| &s.0),
                Some(Screen::More(
                    Mx::Compass | Mx::Level | Mx::Qibla | Mx::Tasks
                ))
            )
    }

    pub(super) fn more_on_sleep(&mut self) {
        self.mx.was_asleep = true;
        self.mx.last_draw = None;
        if self.mx.sos.take().is_some() {
            quick::set_torch(false);
        }
        let _ = sys::save("stats.toml", &self.mx.stats);
        self.mx.unsaved = 0;
    }

    /// Bei jedem Bild: Nutzungszeit dem Vordergrund-Programm zuschreiben, Einschalten/Entsperren zählen.
    pub(super) fn more_account(&mut self) {
        let now = Instant::now();
        let d = self.mx_dt();
        let day = day_key(&d);
        if self.mx.was_asleep {
            self.mx.was_asleep = false;
            self.mx.on_since = now;
            self.mx.break_warned = false;
            self.mx.stats.wakes.entry(day.clone()).or_default().0 += 1;
        }
        if self.mx.was_locked && !self.sx.locked {
            self.mx.stats.wakes.entry(day.clone()).or_default().1 += 1;
        }
        self.mx.was_locked = self.sx.locked;
        if let Some(t) = self.mx.last_draw {
            let secs = now.duration_since(t).as_secs().min(65);
            if secs >= 1 && !self.sx.locked {
                let app = if self.stack.len() <= 1 || self.mx.cur_app.is_empty() {
                    "Home".to_string()
                } else {
                    self.mx.cur_app.clone()
                };
                self.mx.stats.add_usage(&day, &app, secs);
                self.mx.last_draw = Some(now);
                self.mx.unsaved += secs as u32;
            }
        } else {
            self.mx.last_draw = Some(now);
        }
        if self.mx.unsaved >= 300 {
            self.mx.unsaved = 0;
            let _ = sys::save("stats.toml", &self.mx.stats);
        }
    }

    /// Hintergrundarbeit; läuft auch bei ausgeschaltetem Bildschirm (einmal je Minute). true = neu zeichnen.
    pub(super) fn more_tick(&mut self) -> bool {
        let mut redraw = false;
        let now_i = Instant::now();
        // SOS-Lichtsignal: Taschenlampe im Morse-Takt
        if let Some(t0) = self.mx.sos {
            let pat = sysy::morse("SOS ");
            let total: u32 = pat.iter().map(|p| p.1).sum();
            let mut at = (t0.elapsed().as_millis() as u32) % total.max(1);
            let mut on = false;
            for (lit, ms) in pat {
                if at < ms {
                    on = lit;
                    break;
                }
                at -= ms;
            }
            if quick::torch_on() != on {
                quick::set_torch(on);
            }
            redraw = true;
        }
        if self.mx.shot_at.is_some_and(|t| t <= now_i) {
            self.mx.shot_at = None;
            self.mx.want_shot = true;
            redraw = true;
        }
        if self.mx.caffeine.is_some_and(|t| t <= now_i) {
            self.mx.caffeine = None;
            redraw = true;
        }
        if let Some((end, work)) = self.mx.focus {
            if end <= now_i {
                let (title, next) = if work {
                    ("Focus done – take a break", Duration::from_secs(5 * 60))
                } else {
                    (
                        "Break over – back to focus",
                        Duration::from_secs(self.mx.cfg.focus_min.max(1) as u64 * 60),
                    )
                };
                ultar_notify::post(
                    "system",
                    "focus",
                    self.t(title),
                    "",
                    ultar_notify::Urgency::High,
                );
                self.mx.focus = Some((now_i + next, !work));
                redraw = true;
            }
        }
        let now = sys::unix_now();
        if now / 60 == self.mx.last_min {
            return redraw;
        }
        self.mx.last_min = now / 60;
        let d = self.mx_dt();
        let minute = (d.h * 60 + d.mi) as u16;
        // Erinnerungen
        let due: Vec<String> = self
            .mx
            .cfg
            .reminders
            .iter()
            .filter(|r| r.0 <= now)
            .map(|r| r.1.clone())
            .collect();
        if !due.is_empty() {
            self.mx.cfg.reminders.retain(|r| r.0 > now);
            self.mx_save();
            for r in due {
                ultar_notify::post(
                    "system",
                    "reminder",
                    self.t("Reminder"),
                    &r,
                    ultar_notify::Urgency::High,
                );
            }
            redraw = true;
        }
        // Akkuverlauf alle 10 Minuten
        if let Some((pct, chg)) = sys::battery() {
            if self
                .mx
                .stats
                .batt
                .last()
                .is_none_or(|l| now - l.0 >= 600 || l.2 != chg)
            {
                self.mx.stats.add_batt(now, pct, chg);
                self.mx.unsaved += 60;
            }
            if !chg {
                self.mx.charge_warned = false;
            } else if self.mx.cfg.charge_alert && pct >= 80 && !self.mx.charge_warned {
                self.mx.charge_warned = true;
                ultar_notify::post(
                    "system",
                    "battery",
                    self.t("Battery at 80 % – unplug to protect it"),
                    "",
                    ultar_notify::Urgency::Normal,
                );
            }
        }
        // Automatisches Design
        if self.mx.cfg.auto_theme {
            let dark = !sysx::in_window(minute, self.mx.cfg.day_from, self.mx.cfg.day_to);
            if self.set.dark != dark {
                self.set.dark = dark;
                self.save();
                redraw = true;
            }
        }
        // Stündliches Zeitsignal (nicht bei „Nicht stören")
        if self.mx.cfg.hourly_chime && d.mi == 0 && !self.dnd_active() {
            sys::vibrate(80);
        }
        // Pausen-Erinnerung nach durchgehender Bildschirmzeit
        if self.mx.cfg.break_min > 0
            && !self.mx.was_asleep
            && !self.mx.break_warned
            && self.mx.on_since.elapsed().as_secs() >= self.mx.cfg.break_min as u64 * 60
        {
            self.mx.break_warned = true;
            ultar_notify::post(
                "system",
                "wellbeing",
                self.t("Time for a short break"),
                "",
                ultar_notify::Urgency::Normal,
            );
        }
        // Speicher fast voll (höchstens einmal täglich)
        if self.mx.cfg.storage_warn && now - self.mx.stats.last_storage_warn > 86_400 {
            if let Some((total, free)) = sys::disk("/data") {
                if total > 0 && (free * 100 / total < 5 || free < 500 * 1024 * 1024) {
                    self.mx.stats.last_storage_warn = now;
                    ultar_notify::post(
                        "system",
                        "storage",
                        self.t("Storage almost full"),
                        &format!("{} {}", sys::human(free), self.t("free")),
                        ultar_notify::Urgency::Normal,
                    );
                }
            }
        }
        // Sicherungs-Erinnerung (älter als 30 Tage, höchstens einmal je Woche)
        if self.mx.cfg.backup_remind
            && now - self.mx.stats.last_backup_hint > 7 * 86_400
            && d.h == 12
            && d.mi == 0
        {
            if self.backup_age_days().is_none_or(|a| a > 30) {
                self.mx.stats.last_backup_hint = now;
                ultar_notify::post(
                    "system",
                    "backup",
                    self.t("Time for a backup"),
                    self.t("Settings → Backup & reset"),
                    ultar_notify::Urgency::Low,
                );
            }
        }
        // Geplanter Neustart
        if self.mx.cfg.restart_on
            && minute == self.mx.cfg.restart_at
            && (self.mx.cfg.restart_day == 0 || self.mx.cfg.restart_day as usize == d.wd + 1)
            && !self.call_active()
            && sys::uptime_s() > 3600
        {
            self.mx.stats.last_restart = now;
            let _ = sys::save("stats.toml", &self.mx.stats);
            sys::ctl("reboot");
        }
        redraw
    }

    fn backup_age_days(&self) -> Option<i64> {
        let newest = std::fs::read_dir(sysx::backup_root())
            .ok()?
            .flatten()
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .max()?;
        Some(newest.elapsed().ok()?.as_secs() as i64 / 86_400)
    }

    /// Nächstes anstehendes Ereignis für den Startbildschirm: (Symbol, Text)
    pub(super) fn glance(&mut self) -> Option<(Ic, String)> {
        let d = self.mx_dt();
        let now_min = d.h * 60 + d.mi;
        if let Some((end, work)) = self.mx.focus {
            let left = end.saturating_duration_since(Instant::now()).as_secs() / 60 + 1;
            return Some((
                Ic::Clock,
                format!(
                    "{} · {left} min",
                    self.t(if work { "Focus" } else { "Break" })
                ),
            ));
        }
        if let Some(r) = self.mx.cfg.reminders.iter().min_by_key(|r| r.0) {
            let dt = sys::from_unix(r.0, self.set.tz_min);
            return Some((
                Ic::Bell,
                format!("{} · {}", self.clock_str(dt.h, dt.mi), r.1),
            ));
        }
        let next = self
            .tl
            .alarms
            .list
            .iter()
            .filter(|a| a.on)
            .map(|a| {
                (
                    ((a.h as i64 * 60 + a.m as i64) - now_min).rem_euclid(1440),
                    a,
                )
            })
            .min_by_key(|x| x.0);
        if let Some((_, a)) = next {
            return Some((
                Ic::Bell,
                format!(
                    "{} {}",
                    self.t("Alarm"),
                    self.clock_str(a.h as i64, a.m as i64)
                ),
            ));
        }
        let (hy, hm, hd) = sysy::hijri(d.y, d.mo, d.d);
        let mon = if self.set.lang == "ar" {
            sysy::HIJRI_MONTHS_AR
        } else {
            sysy::HIJRI_MONTHS
        }[(hm - 1).clamp(0, 11) as usize];
        Some((Ic::Clock, format!("{hd} {mon} {hy}")))
    }

    /// Zusatz auf dem Sperrbildschirm: Besitzerinfo
    pub(super) fn more_lock_extra(&mut self, ui: &mut Ui) {
        let info = self.mx.cfg.owner_info.clone();
        if info.trim().is_empty() {
            return;
        }
        let c = rgb(0xFFFFFF).alpha(0.75);
        let f = ui.fit(&info, 13.0, false, ui.w - ui.dp(48.0));
        ui.label(
            Rect::new(ui.dp(24.0), ui.h * 0.355, ui.w - ui.dp(48.0), ui.dp(22.0)),
            &f,
            13.0,
            c,
            false,
            Al::Center,
        );
    }

    /// Statussymbole (rechts in der Statusleiste): Wecker, Nicht stören, lautlos, Fokus, wach halten, angeheftet.
    pub(super) fn status_icons(&mut self, ui: &mut Ui, mut ix: f32, dir: f32, h: f32) {
        let step = ui.dp(19.0);
        let c = ui.th.text.alpha(0.85);
        let mut put = |ui: &mut Ui, ic: Ic, col: Color| {
            ui.icon(ic, ix, h / 2.0, ui.dp(12.0), col);
            ix += dir * step;
        };
        if self.tl.alarms.list.iter().any(|a| a.on) {
            put(ui, Ic::Bell, c);
        }
        if self.dnd_active() || self.mx.cfg.sleep_mode {
            put(ui, Ic::Moon, c);
        }
        if self.sx.cfg.profile > 0 || !self.set.sound {
            put(ui, Ic::Speaker, ui.th.sub.alpha(0.6));
        }
        if self.mx.focus.is_some() {
            put(ui, Ic::Clock, ui.th.accent);
        }
        if self.caffeine_on() {
            put(ui, Ic::Sun, ui.th.accent);
        }
        if self.mx.pinned {
            put(ui, Ic::Lock, ui.th.accent);
        }
    }

    /// Zweite Schalterreihe der Schnelleinstellungen. Liefert die belegte Höhe.
    /// Bildschirmfoto in 3 s (Schnelleinstellungen)
    pub(super) fn mx_shot_soon(&mut self) {
        self.mx.shot_at = Some(Instant::now() + Duration::from_secs(3));
    }

    pub(super) fn shade_row2(&mut self, ui: &mut Ui, m: f32, y: f32, gap: f32) -> f32 {
        let caf = self.caffeine_on();
        let items: [(Ic, &str, bool); 4] = [
            (Ic::Sun, self.t("Night light"), self.sx.cfg.night),
            (Ic::Moon, self.t("Do not disturb"), self.sx.cfg.dnd),
            (Ic::Clock, self.t("Keep awake"), caf),
            (Ic::Image, self.t("Screenshot"), self.mx.shot_at.is_some()),
        ];
        let sw = (ui.w - 2.0 * m - 3.0 * gap) / 4.0;
        let sh = ui.dp(58.0);
        for (i, (ic, name, on)) in items.into_iter().enumerate() {
            let x = m + i as f32 * (sw + gap);
            let r = Rect::new(if ui.rtl { ui.w - x - sw } else { x }, y, sw, sh);
            if self.small_tile(ui, r, ic, name, on) {
                match i {
                    0 => {
                        self.sx.cfg.night = !on;
                        let _ = sys::save("system.toml", &self.sx.cfg);
                    }
                    1 => {
                        self.sx.cfg.dnd = !on;
                        let _ = sys::save("system.toml", &self.sx.cfg);
                    }
                    2 => {
                        self.mx.caffeine = if on {
                            None
                        } else {
                            Some(Instant::now() + Duration::from_secs(30 * 60))
                        }
                    }
                    _ => {
                        // Leiste schließen, 3 s warten, dann aufnehmen
                        self.shade = false;
                        self.mx.shot_at = Some(Instant::now() + Duration::from_secs(3));
                        self.toast(self.t("Screenshot in 3 s").into());
                    }
                }
                ui.dirty = true;
            }
        }
        sh + ui.dp(8.0)
    }

    /// Kleine Schalter-Kachel (Schnelleinstellungen)
    pub(super) fn small_tile(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        ic: Ic,
        name: &str,
        on: bool,
    ) -> bool {
        let held = ui.held(r);
        let bg = if on {
            ui.th.accent
        } else if held {
            ui.th.press
        } else {
            ui.th.card
        };
        ui.cv
            .rrect(r, ui.dp(16.0), if on { ui.th.accent } else { ui.th.line });
        ui.cv.rrect(r.inset(1.0), ui.dp(16.0), bg);
        let fg = if on {
            ui.th.on_accent
        } else {
            ui.th.text.mix(ui.th.sub, 0.25)
        };
        ui.icon(ic, r.cx(), r.y + r.h * 0.36, ui.dp(19.0), fg);
        let mut fs = 10.5;
        while fs > 8.0 && ui.text_w(name, fs, on) > r.w - ui.dp(8.0) {
            fs -= 0.5;
        }
        let f = ui.fit(name, fs, on, r.w - ui.dp(6.0));
        ui.label(
            Rect::new(r.x, r.y + r.h * 0.6, r.w, r.h * 0.32),
            &f,
            fs,
            fg,
            on,
            Al::Center,
        );
        ui.tap(r)
    }

    /// Über der Tastatur: Einfügen / Kopieren für das aktive Feld
    // ================================================================ Bildschirme

    pub(super) fn s_more(&mut self, ui: &mut Ui, area: Rect, m: Mx, sc: &mut f32) {
        match m {
            Mx::Tools => self.s_tools(ui, area, sc),
            Mx::Compass => self.s_compass(ui, area, false),
            Mx::Qibla => self.s_compass(ui, area, true),
            Mx::Level => self.s_level(ui, area),
            Mx::Prayer => self.s_prayer(ui, area, sc),
            Mx::Password => self.s_password(ui, area),
            Mx::Dice => self.s_dice(ui, area),
            Mx::Percent => self.s_percent(ui, area, sc),
            Mx::Loan => self.s_loan(ui, area, sc),
            Mx::DateCalc => self.s_datecalc(ui, area, sc),
            Mx::Bases => self.s_bases(ui, area),
            Mx::Hijri => self.s_hijri(ui, area),
            Mx::Currency => self.s_currency(ui, area, sc),
            Mx::Counter => self.s_counter(ui, area),
            Mx::BigText => self.s_bigtext(ui, area),
            Mx::Light => self.s_light(ui),
            Mx::Sos => self.s_sos(ui, area),
            Mx::Focus => self.s_focus(ui, area),
            Mx::Countdown => self.s_countdown(ui, area, sc),
            Mx::Todo => self.s_todo(ui, area, sc),
            Mx::Reminders => self.s_reminders(ui, area, sc),
            Mx::Emergency => self.s_emergency(ui, area, sc),
            Mx::Recents => self.s_recents(ui, area, sc),
            Mx::Clipboard => self.s_clipboard(ui, area, sc),
            Mx::BattHist => self.s_batthist(ui, area),
            Mx::Usage => self.s_usage(ui, area, sc),
            Mx::Care => self.s_care(ui, area, sc),
            Mx::StorageX => self.s_storagex(ui, area, sc),
            Mx::Tasks => self.s_tasks(ui, area, sc),
            Mx::Modes => self.s_modes(ui, area, sc),
            Mx::Blocked => self.s_blocked(ui, area, sc),
        }
    }

    /// Hinweistext mit Zeilenumbruch
    fn note(&mut self, ui: &mut Ui, y: &mut f32, s: &str) {
        let c = ui.th.sub;
        let h = ui.para(ui.dp(22.0), *y + ui.dp(2.0), ui.w - ui.dp(44.0), s, 12.5, c);
        *y += h + ui.dp(12.0);
    }

    /// Schlichter Anteilsbalken in Akzentfarbe
    fn bar(&mut self, ui: &mut Ui, r: Rect, v: f32) {
        ui.cv.rrect(r, r.h / 2.0, ui.th.line);
        let w = (r.w * v.clamp(0.0, 1.0)).max(r.h);
        let fr = if ui.rtl {
            Rect::new(r.right() - w, r.y, w, r.h)
        } else {
            Rect::new(r.x, r.y, w, r.h)
        };
        ui.cv.rrect(fr, r.h / 2.0, ui.th.accent);
    }

    /// Eintragsfeld mit Beschriftung
    fn mx_field(
        &mut self,
        ui: &mut Ui,
        y: &mut f32,
        id: &'static str,
        label: &'static str,
        hint: &str,
    ) {
        self.caption(ui, y, self.t(label));
        let r = Rect::new(ui.dp(16.0), *y, ui.w - ui.dp(32.0), ui.dp(50.0));
        if ui.visible(r) {
            ui.field(r, id, &self.f(id), hint);
        }
        *y += ui.dp(58.0);
    }

    /// Ergebnis-Karte: große Zahl + Beschriftung, Antippen kopiert
    fn mx_result(&mut self, ui: &mut Ui, y: &mut f32, label: &str, value: &str) {
        let r = Rect::new(ui.dp(16.0), *y, ui.w - ui.dp(32.0), ui.dp(74.0));
        *y += r.h + ui.dp(10.0);
        if !ui.visible(r) {
            return;
        }
        ui.card(r);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let inn = r.inset(ui.dp(14.0));
        ui.label(
            Rect::new(inn.x, inn.y - ui.dp(4.0), inn.w, ui.dp(18.0)),
            label,
            12.0,
            cs,
            false,
            Al::Start,
        );
        let old = ui.rtl;
        let vf = ui.fit(value, 24.0, true, inn.w);
        ui.label(
            Rect::new(inn.x, inn.y + ui.dp(14.0), inn.w, ui.dp(34.0)),
            &vf,
            24.0,
            c,
            true,
            Al::Start,
        );
        ui.rtl = old;
        if ui.tap(r) {
            self.copy_text(value);
            ui.dirty = true;
        }
    }

    /// Werkzeuge wie in der Web-Demo: Karten in 3 Spalten, goldenes Symbol, Name darunter
    fn s_tools(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Tools"));
        ui.scroll_begin(area, sc);
        let m = ui.dp(16.0);
        let gap = ui.dp(10.0);
        let cols = (((ui.w - 2.0 * m) / ui.dp(118.0)) as usize).clamp(3, 8);
        let cw = (ui.w - 2.0 * m - gap * (cols - 1) as f32) / cols as f32;
        let ch = ui.dp(96.0);
        let y0 = area.y - *sc;
        let c = ui.th.text;
        for (i, (ic, name, target)) in TOOLS.iter().enumerate() {
            let col = if ui.rtl {
                cols - 1 - i % cols
            } else {
                i % cols
            };
            let r = Rect::new(
                m + col as f32 * (cw + gap),
                y0 + (i / cols) as f32 * (ch + gap),
                cw,
                ch,
            );
            if !ui.visible(r) {
                continue;
            }
            let held = ui.held(r);
            ui.card_h(r, held);
            ui.icon(*ic, r.cx(), r.y + ui.dp(34.0), ui.dp(26.0), ui.th.accent);
            let lab = self.t(name);
            let lf = ui.fit(lab, 12.5, false, r.w - ui.dp(8.0));
            ui.label(
                Rect::new(
                    r.x + ui.dp(4.0),
                    r.y + ui.dp(58.0),
                    r.w - ui.dp(8.0),
                    ui.dp(20.0),
                ),
                &lf,
                12.5,
                c,
                false,
                Al::Center,
            );
            if ui.tap(r) {
                self.go(Screen::More(target.clone()));
                ui.dirty = true;
            }
        }
        ui.scroll_end(
            area,
            TOOLS.len().div_ceil(cols) as f32 * (ch + gap) + ui.dp(16.0),
            sc,
        );
    }

    /// Kompass und Qibla: Zifferblatt dreht sich gegen die Geräterichtung.
    fn s_compass(&mut self, ui: &mut Ui, area: Rect, qibla: bool) {
        let area = self.header(ui, area, self.t(if qibla { "Qibla" } else { "Compass" }));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (mag, _) = self.sensor();
        let head = mag.map(|m| sysy::heading(m[0], m[1], self.mx.cfg.compass_offset));
        let (cx, cy) = (ui.w / 2.0, area.y + area.h * 0.40);
        let r = (ui.w * 0.36).min(area.h * 0.32);
        ui.cv.ring(cx, cy, r, ui.dp(2.0), ui.th.line);
        ui.cv.ring(cx, cy, r * 0.72, 1.0, ui.th.line);
        let rot = head.unwrap_or(0.0);
        let pt = |deg: f32, rr: f32| {
            let a = (deg - rot).to_radians();
            (cx + a.sin() * rr, cy - a.cos() * rr)
        };
        for k in 0..36 {
            let (x0, y0) = pt(k as f32 * 10.0, r);
            let (x1, y1) = pt(
                k as f32 * 10.0,
                r - ui.dp(if k % 9 == 0 { 14.0 } else { 7.0 }),
            );
            ui.cv.line(
                x0,
                y0,
                x1,
                y1,
                ui.dp(if k % 9 == 0 { 2.0 } else { 1.0 }),
                if k == 0 { rgb(0xF0566A) } else { cs },
            );
        }
        for (deg, l) in [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")] {
            let (x, y) = pt(deg, r - ui.dp(30.0));
            ui.label(
                Rect::new(x - ui.dp(14.0), y - ui.dp(12.0), ui.dp(28.0), ui.dp(24.0)),
                l,
                15.0,
                if deg == 0.0 { rgb(0xF0566A) } else { c },
                true,
                Al::Center,
            );
        }
        // Gerätezeiger (oben)
        ui.cv.line(
            cx,
            cy - r - ui.dp(14.0),
            cx,
            cy - r + ui.dp(4.0),
            ui.dp(3.0),
            ui.th.accent,
        );
        let (lat, lon) = self.mx_place();
        if qibla {
            let q = sysy::qibla(lat, lon) as f32;
            let (x, y) = pt(q, r * 0.86);
            ui.cv.line(cx, cy, x, y, ui.dp(3.0), ui.th.accent);
            ui.cv.circle(x, y, ui.dp(9.0), ui.th.accent);
            ui.cv.circle(cx, cy, ui.dp(5.0), c);
            let aligned =
                head.is_some_and(|h| ((h - q + 540.0).rem_euclid(360.0) - 180.0).abs() < 4.0);
            let txt = format!("{} {:.0}° {}", self.t("Qibla at"), q, sysy::cardinal(q));
            ui.label(
                Rect::new(0.0, cy + r + ui.dp(28.0), ui.w, ui.dp(30.0)),
                &txt,
                22.0,
                if aligned { rgb(0x34C759) } else { c },
                true,
                Al::Center,
            );
            let city = self.mx_city_name();
            ui.label(
                Rect::new(0.0, cy + r + ui.dp(60.0), ui.w, ui.dp(20.0)),
                &city,
                13.0,
                cs,
                false,
                Al::Center,
            );
        } else {
            ui.cv.circle(cx, cy, ui.dp(5.0), c);
            let txt = head
                .map(|h| format!("{:.0}° {}", h, sysy::cardinal(h)))
                .unwrap_or_else(|| "—".into());
            ui.label(
                Rect::new(0.0, cy + r + ui.dp(28.0), ui.w, ui.dp(40.0)),
                &txt,
                32.0,
                c,
                true,
                Al::Center,
            );
        }
        let mut y = area.bottom() - ui.dp(if qibla { 62.0 } else { 118.0 });
        if head.is_none() {
            ui.para(
                ui.dp(24.0),
                cy + r + ui.dp(84.0),
                ui.w - ui.dp(48.0),
                self.t("No magnetic sensor data (sensors off or missing)"),
                12.5,
                rgb(0xF08A4B),
            );
        }
        if qibla {
            if ui.row(
                &mut y,
                Some(Ic::Globe),
                self.t("Location"),
                None,
                Some(&self.mx_city_name()),
                true,
            ) {
                self.go(Screen::More(Mx::Prayer));
                ui.dirty = true;
            }
        } else {
            let v = format!("{:+.0}°", self.mx.cfg.compass_offset);
            let d = ui.stepper(&mut y, self.t("Correction"), &v);
            if d != 0 {
                self.mx.cfg.compass_offset =
                    (self.mx.cfg.compass_offset + d as f32 * 5.0).clamp(-180.0, 180.0);
                self.mx_save();
            }
            self.note(
                ui,
                &mut y,
                self.t("Hold the device flat. Move it in a figure eight to calibrate."),
            );
        }
        ui.dirty = true;
    }

    fn mx_place(&self) -> (f64, f64) {
        if self.mx.cfg.lat != 0.0 || self.mx.cfg.lon != 0.0 {
            (self.mx.cfg.lat, self.mx.cfg.lon)
        } else {
            let c = sysy::CITIES[self.mx.cfg.city.min(sysy::CITIES.len() - 1)];
            (c.2, c.3)
        }
    }

    fn mx_city_name(&self) -> String {
        if self.mx.cfg.lat != 0.0 || self.mx.cfg.lon != 0.0 {
            format!("{:.3}, {:.3}", self.mx.cfg.lat, self.mx.cfg.lon)
        } else {
            let c = sysy::CITIES[self.mx.cfg.city.min(sysy::CITIES.len() - 1)];
            if self.set.lang == "ar" { c.1 } else { c.0 }.to_string()
        }
    }

    fn s_level(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Spirit level"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (_, acc) = self.sensor();
        let (cx, cy) = (ui.w / 2.0, area.y + area.h * 0.42);
        let r = (ui.w * 0.36).min(area.h * 0.32);
        ui.cv.ring(cx, cy, r, ui.dp(2.0), ui.th.line);
        ui.cv.ring(cx, cy, r * 0.2, ui.dp(1.5), cs);
        ui.cv.line(cx - r, cy, cx + r, cy, 1.0, ui.th.line);
        ui.cv.line(cx, cy - r, cx, cy + r, 1.0, ui.th.line);
        match acc {
            Some(a) => {
                let (roll, pitch) = sysy::tilt(a[0], a[1], a[2]);
                let span = r - ui.dp(22.0);
                let lim = |v: f32| (v / 45.0).clamp(-1.0, 1.0) * span;
                let flat = roll.abs() < 1.0 && pitch.abs() < 1.0;
                let col = if flat { rgb(0x34C759) } else { ui.th.accent };
                ui.cv.circle(
                    cx - lim(roll),
                    cy + lim(pitch),
                    ui.dp(22.0),
                    col.alpha(0.25),
                );
                ui.cv
                    .circle(cx - lim(roll), cy + lim(pitch), ui.dp(16.0), col);
                ui.label(
                    Rect::new(0.0, cy + r + ui.dp(26.0), ui.w, ui.dp(40.0)),
                    &format!("{:.1}°  ·  {:.1}°", roll, pitch),
                    30.0,
                    if flat { col } else { c },
                    true,
                    Al::Center,
                );
                ui.label(
                    Rect::new(0.0, cy + r + ui.dp(68.0), ui.w, ui.dp(20.0)),
                    self.t("left/right · front/back"),
                    12.5,
                    cs,
                    false,
                    Al::Center,
                );
            }
            None => {
                ui.para(
                    ui.dp(24.0),
                    cy + r + ui.dp(30.0),
                    ui.w - ui.dp(48.0),
                    self.t("No accelerometer data (sensors off or missing)"),
                    12.5,
                    rgb(0xF08A4B),
                );
            }
        }
        ui.dirty = true;
    }

    fn s_prayer(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Prayer times"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let d = self.mx_dt();
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (hy, hm, hd) = sysy::hijri(d.y, d.mo, d.d);
        let ar = self.set.lang == "ar";
        let mon = if ar {
            sysy::HIJRI_MONTHS_AR
        } else {
            sysy::HIJRI_MONTHS
        }[(hm - 1).clamp(0, 11) as usize];
        self.mx_result(
            ui,
            &mut y,
            &format!("{:02}.{:02}.{} · {}", d.d, d.mo, d.y, self.mx_city_name()),
            &format!("{hd} {mon} {hy}"),
        );
        let (lat, lon) = self.mx_place();
        let times = sysy::prayer_times(
            lat,
            lon,
            self.set.tz_min,
            (d.y, d.mo, d.d),
            self.mx.cfg.method,
        );
        let now = (d.h * 60 + d.mi) as u16;
        let next = times
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .find(|(_, t)| t.is_some_and(|t| t > now))
            .map(|(i, _)| i);
        let names = ["Fajr", "Sunrise", "Dhuhr", "Asr", "Maghrib", "Isha"];
        // wie in der Demo: eine Liste, nächste Gebetszeit mit goldenem Wert und goldenem Symbolfeld
        ui.kb.row_end.set(-1.0);
        for (i, t) in times.iter().enumerate() {
            let on = next == Some(i);
            let v = t
                .map(|t| self.clock_str((t / 60) as i64, (t % 60) as i64))
                .unwrap_or_else(|| "—".into());
            ui.kb
                .row_bg
                .set(Some(rgb(if on { 0x4A421C } else { 0x2A2D35 })));
            ui.kb.row_gold.set(on);
            ui.row(
                &mut y,
                Some(if i == 1 { Ic::Sun } else { Ic::Clock }),
                self.t(names[i]),
                None,
                Some(&v),
                false,
            );
        }
        let _ = (c, cs);
        y += ui.dp(10.0);
        if let Some(i) = next {
            if let Some(t) = times[i] {
                let left = t - now;
                self.note(
                    ui,
                    &mut y,
                    &format!(
                        "{} {} {} h {:02} min",
                        self.t(names[i]),
                        self.t("in"),
                        left / 60,
                        left % 60
                    ),
                );
            }
        }
        ui.section(&mut y, self.t("Location"));
        let cities: Vec<&str> = sysy::CITIES
            .iter()
            .map(|c| if ar { c.1 } else { c.0 })
            .collect();
        for chunk in 0..cities.len().div_ceil(4) {
            let part = &cities[chunk * 4..(chunk * 4 + 4).min(cities.len())];
            let sel = if self.mx.cfg.lat == 0.0
                && self.mx.cfg.lon == 0.0
                && self.mx.cfg.city / 4 == chunk
            {
                self.mx.cfg.city % 4
            } else {
                usize::MAX
            };
            if let Some(i) = ui.chips(&mut y, part, sel) {
                self.mx.cfg.city = chunk * 4 + i;
                self.mx.cfg.lat = 0.0;
                self.mx.cfg.lon = 0.0;
                self.mx_save();
            }
        }
        ui.section(&mut y, self.t("Calculation method"));
        let ms: Vec<&str> = sysy::METHODS.iter().map(|m| m.0).collect();
        if let Some(i) = ui.chips(&mut y, &ms, self.mx.cfg.method) {
            self.mx.cfg.method = i;
            self.mx_save();
        }
        self.note(
            ui,
            &mut y,
            self.t("Calculated times; the local announcement may differ by a few minutes."),
        );
        if ui.row(&mut y, Some(Ic::Globe), self.t("Qibla"), None, None, true) {
            self.go(Screen::More(Mx::Qibla));
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn mx_new_password(&mut self) {
        let (len, up, di, sy) = self.mx.pw_opts;
        self.mx.pw = sysy::password(&sysy::random_bytes(len * 8), len, up, di, sy);
    }

    fn s_password(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Password generator"));
        let mut y = area.y;
        let pw = self.mx.pw.clone();
        self.mx_result(ui, &mut y, self.t("Tap to copy"), &pw);
        let d = ui.stepper(&mut y, self.t("Length"), &self.mx.pw_opts.0.to_string());
        let mut ch = d != 0;
        self.mx.pw_opts.0 = (self.mx.pw_opts.0 as i32 + d * 2).clamp(8, 40) as usize;
        let (mut up, mut di, mut sy) = (self.mx.pw_opts.1, self.mx.pw_opts.2, self.mx.pw_opts.3);
        ch |= ui.toggle_row(&mut y, self.t("Capital letters"), None, &mut up);
        ch |= ui.toggle_row(&mut y, self.t("Digits"), None, &mut di);
        ch |= ui.toggle_row(&mut y, self.t("Symbols"), None, &mut sy);
        self.mx.pw_opts = (self.mx.pw_opts.0, up, di, sy);
        if ui.button(
            Rect::new(ui.dp(16.0), y + ui.dp(4.0), ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("New password"),
            Btn::Primary,
        ) || ch
        {
            self.mx_new_password();
            ui.dirty = true;
        }
        y += ui.dp(62.0);
        self.note(
            ui,
            &mut y,
            self.t("Created on this device from the system's random source. Nothing is stored."),
        );
    }

    fn s_dice(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Dice & coin"));
        let c = ui.th.text;
        let txt = if self.mx.dice.is_empty() {
            "—".to_string()
        } else {
            self.mx
                .dice
                .iter()
                .map(|v| match v {
                    -1 => self.t("Heads").to_string(),
                    -2 => self.t("Tails").to_string(),
                    n => n.to_string(),
                })
                .collect::<Vec<_>>()
                .join("  ·  ")
        };
        let f = ui.fit(&txt, 44.0, true, ui.w - ui.dp(40.0));
        ui.label(
            Rect::new(
                ui.dp(20.0),
                area.y + ui.dp(30.0),
                ui.w - ui.dp(40.0),
                ui.dp(90.0),
            ),
            &f,
            44.0,
            c,
            true,
            Al::Center,
        );
        if self.mx.dice.len() > 1 && self.mx.dice.iter().all(|v| *v > 0) {
            let sum: i64 = self.mx.dice.iter().sum();
            ui.label(
                Rect::new(0.0, area.y + ui.dp(118.0), ui.w, ui.dp(22.0)),
                &format!("Σ {sum}"),
                15.0,
                ui.th.sub,
                false,
                Al::Center,
            );
        }
        let mut y = area.y + ui.dp(170.0);
        let w = ui.w - ui.dp(24.0);
        let acts: [(&str, u8); 4] = [
            (self.t("Roll one die"), 0),
            (self.t("Roll two dice"), 1),
            (self.t("Flip a coin"), 2),
            (self.t("Number 1–100"), 3),
        ];
        for (lab, a) in acts {
            if ui.button(
                Rect::new(ui.dp(12.0), y, w, ui.dp(46.0)),
                lab,
                if a == 0 { Btn::Primary } else { Btn::Plain },
            ) {
                self.mx.dice = match a {
                    0 => vec![sysy::random_range(1, 6)],
                    1 => vec![sysy::random_range(1, 6), sysy::random_range(1, 6)],
                    2 => vec![-sysy::random_range(1, 2)],
                    _ => vec![sysy::random_range(1, 100)],
                };
                if self.sx.cfg.haptic {
                    sys::vibrate(25);
                }
                ui.dirty = true;
            }
            y += ui.dp(58.0);
        }
    }

    fn s_percent(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Percent & tip"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        self.mx_field(ui, &mut y, "mx_p_amount", "Amount", "100");
        self.mx_field(ui, &mut y, "mx_p_pct", "Percent", "10");
        self.mx_field(ui, &mut y, "mx_p_people", "People (split)", "1");
        if let (Some(a), Some(p)) = (self.mx_num("mx_p_amount"), self.mx_num("mx_p_pct")) {
            let part = a * p / 100.0;
            let n = self.mx_num("mx_p_people").unwrap_or(1.0).max(1.0);
            self.mx_result(
                ui,
                &mut y,
                &format!("{p} % {} {a}", self.t("of")),
                &sysx::fmt_num(part),
            );
            self.mx_result(
                ui,
                &mut y,
                self.t("Amount plus percent"),
                &sysx::fmt_num(a + part),
            );
            self.mx_result(
                ui,
                &mut y,
                self.t("Amount minus percent"),
                &sysx::fmt_num(a - part),
            );
            if n > 1.0 {
                self.mx_result(
                    ui,
                    &mut y,
                    self.t("Per person (with tip)"),
                    &sysx::fmt_num((a + part) / n),
                );
            }
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_loan(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Loan calculator"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        self.mx_field(ui, &mut y, "mx_l_amount", "Amount", "10000");
        self.mx_field(ui, &mut y, "mx_l_rate", "Interest per year (%)", "6");
        self.mx_field(ui, &mut y, "mx_l_months", "Months", "24");
        if let (Some(a), Some(r), Some(n)) = (
            self.mx_num("mx_l_amount"),
            self.mx_num("mx_l_rate"),
            self.mx_num("mx_l_months"),
        ) {
            if n >= 1.0 && n <= 600.0 && a > 0.0 && r >= 0.0 {
                let pay = sysy::loan_payment(a, r, n as u32);
                self.mx_result(ui, &mut y, self.t("Monthly payment"), &format!("{pay:.2}"));
                self.mx_result(ui, &mut y, self.t("Total paid"), &format!("{:.2}", pay * n));
                self.mx_result(
                    ui,
                    &mut y,
                    self.t("Interest total"),
                    &format!("{:.2}", pay * n - a),
                );
            }
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_datecalc(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Date calculator"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let d = self.mx_dt();
        let today = format!("{:02}.{:02}.{}", d.d, d.mo, d.y);
        self.mx_field(ui, &mut y, "mx_d_a", "From (DD.MM.YYYY)", &today);
        self.mx_field(ui, &mut y, "mx_d_b", "To (DD.MM.YYYY)", &today);
        self.mx_field(ui, &mut y, "mx_d_n", "Add days", "30");
        let a = sysy::parse_date(&self.f("mx_d_a")).or(if self.f("mx_d_a").is_empty() {
            Some((d.y, d.mo, d.d))
        } else {
            None
        });
        let days = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        if let Some(a) = a {
            if let Some(b) = sysy::parse_date(&self.f("mx_d_b")) {
                let n = sysy::days_between(a, b);
                self.mx_result(
                    ui,
                    &mut y,
                    self.t("Days between"),
                    &format!(
                        "{n} ({} {} {} {})",
                        n.abs() / 7,
                        self.t("weeks"),
                        n.abs() % 7,
                        self.t("days")
                    ),
                );
                self.mx_result(
                    ui,
                    &mut y,
                    self.t("Weekday of the target date"),
                    self.t(days[sysy::weekday(b.0, b.1, b.2)]),
                );
            }
            if let Some(n) = self.mx_num("mx_d_n") {
                let r = sysy::add_days(a, n as i64);
                self.mx_result(
                    ui,
                    &mut y,
                    self.t("Date after adding days"),
                    &format!(
                        "{:02}.{:02}.{} · {}",
                        r.2,
                        r.1,
                        r.0,
                        self.t(days[sysy::weekday(r.0, r.1, r.2)])
                    ),
                );
            }
        } else {
            self.note(ui, &mut y, self.t("Invalid date"));
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    pub(super) fn s_bases(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Number systems"));
        let mut y = area.y;
        self.mx_field(ui, &mut y, "mx_b_dec", "Decimal", "255");
        self.mx_field(ui, &mut y, "mx_b_hex", "Hexadecimal", "FF");
        self.mx_field(ui, &mut y, "mx_b_bin", "Binary", "11111111");
        // das zuletzt bearbeitete (fokussierte) Feld gibt den Wert vor
        let src = match self.focus {
            Some("mx_b_hex") => sysy::parse_base(&self.f("mx_b_hex"), 16),
            Some("mx_b_bin") => sysy::parse_base(&self.f("mx_b_bin"), 2),
            _ => sysy::parse_base(&self.f("mx_b_dec"), 10),
        };
        if let Some(v) = src {
            for (id, base) in [("mx_b_dec", 10), ("mx_b_hex", 16), ("mx_b_bin", 2)] {
                if self.focus != Some(id) {
                    self.fields.insert(id, sysy::to_base(v, base));
                }
            }
            self.mx_result(ui, &mut y, self.t("Octal"), &sysy::to_base(v, 8));
        }
    }

    fn s_currency(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Currency"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let rates = self.mx.cfg.rates.clone();
        let names: Vec<&str> = rates.iter().map(|r| r.0.as_str()).collect();
        self.mx_field(ui, &mut y, "mx_cur_v", "Amount", "1");
        self.note(ui, &mut y, self.t("From"));
        let fsel = self
            .fields
            .get("mx_cur_from")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
            .min(names.len().saturating_sub(1));
        if let Some(i) = ui.chips(&mut y, &names, fsel) {
            self.fields.insert("mx_cur_from", i.to_string());
        }
        let v = self.mx_num("mx_cur_v").unwrap_or(0.0);
        ui.section(&mut y, self.t("Result"));
        for (i, r) in rates.iter().enumerate() {
            if i != fsel {
                self.mx_result(
                    ui,
                    &mut y,
                    &r.0,
                    &sysx::fmt_num(sysy::convert_rate(&rates, fsel, i, v)),
                );
            }
        }
        ui.section(
            &mut y,
            self.t("Your rates (units per 1 of the first currency)"),
        );
        for (i, r) in rates.iter().enumerate().skip(1) {
            let d = ui.stepper(&mut y, &r.0, &sysx::fmt_num(r.1));
            if d != 0 {
                // Schrittweite ~1 % des Werts, sinnvoll gerundet
                let step = (r.1 * 0.01).max(0.001);
                self.mx.cfg.rates[i].1 = ((r.1 + d as f64 * step) * 1000.0).round() / 1000.0;
                self.mx_save();
            }
        }
        self.note(
            ui,
            &mut y,
            self.t("Rates are entered by you – no internet needed, nothing is sent."),
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_counter(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Counter"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let n = self.mx.cfg.counter;
        let goal = self.mx.cfg.counter_goal.max(1);
        let (cx, cy) = (ui.w / 2.0, area.y + area.h * 0.36);
        let r = (ui.w * 0.34).min(area.h * 0.28);
        let big = Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
        let held = ui.held(big);
        ui.cv.circle(
            cx,
            cy,
            r,
            if held {
                ui.th.accent.alpha(0.3)
            } else {
                ui.th.accent.alpha(0.14)
            },
        );
        ui.cv.ring(cx, cy, r, ui.dp(3.0), ui.th.accent);
        ui.label(
            Rect::new(cx - r, cy - ui.dp(40.0), 2.0 * r, ui.dp(70.0)),
            &n.to_string(),
            60.0,
            c,
            true,
            Al::Center,
        );
        ui.label(
            Rect::new(cx - r, cy + ui.dp(30.0), 2.0 * r, ui.dp(22.0)),
            &format!("{} / {goal} · {}×", n % goal, n / goal),
            14.0,
            cs,
            false,
            Al::Center,
        );
        if ui.tap(big) {
            self.mx.cfg.counter += 1;
            // am Ziel deutlich, sonst kurz vibrieren
            sys::vibrate(if self.mx.cfg.counter % goal == 0 {
                250
            } else {
                15
            });
            if self.mx.cfg.counter % 10 == 0 {
                self.mx_save();
            }
            ui.dirty = true;
        }
        let mut y = cy + r + ui.dp(30.0);
        let goals = [33u32, 99, 100, 1000];
        let gs: Vec<String> = goals.iter().map(|g| g.to_string()).collect();
        let gr: Vec<&str> = gs.iter().map(|s| s.as_str()).collect();
        if let Some(i) = ui.chips(
            &mut y,
            &gr,
            goals.iter().position(|g| *g == goal).unwrap_or(usize::MAX),
        ) {
            self.mx.cfg.counter_goal = goals[i];
            self.mx_save();
        }
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0)),
            self.t("Reset"),
            Btn::Plain,
        ) {
            self.mx.cfg.counter = 0;
            self.mx_save();
            ui.dirty = true;
        }
    }

    fn s_bigtext(&mut self, ui: &mut Ui, area: Rect) {
        let txt = self.f("mx_big");
        let mut y = area.y + ui.dp(8.0);
        ui.field(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0)),
            "mx_big",
            &txt,
            self.t("Text to show large"),
        );
        y += ui.dp(64.0);
        let c = ui.th.text;
        if txt.trim().is_empty() {
            ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                self.t("Type a message – it is shown as large as possible."),
                13.0,
                ui.th.sub,
            );
            return;
        }
        // größte Schrift, bei der der umbrochene Text in die Fläche passt
        let (w, h) = (
            ui.w - ui.dp(32.0),
            (area.bottom() - y - ui.dp(12.0)).max(ui.dp(60.0)),
        );
        let mut fs = 120.0f32;
        while fs > 16.0 {
            let (tw, th) = ui.tx.measure(&txt, ui.dp(fs), true, w);
            if th <= h && tw <= w + 1.0 {
                break;
            }
            fs -= 6.0;
        }
        ui.para(ui.dp(16.0), y, w, &txt, fs, c);
    }

    fn s_light(&mut self, ui: &mut Ui) {
        // ganzer Bildschirm weiß, volle Helligkeit; Verlassen stellt die Helligkeit wieder her
        ui.cv
            .rect(Rect::new(0.0, 0.0, ui.w, ui.h - ui.dp(52.0)), rgb(0xFFFFFF));
        sys::set_backlight(1.0);
        ui.label(
            Rect::new(0.0, ui.h * 0.45, ui.w, ui.dp(24.0)),
            self.t("Screen light – back to leave"),
            14.0,
            rgb(0x999999),
            false,
            Al::Center,
        );
    }

    fn s_sos(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("SOS signal"));
        let on = self.mx.sos.is_some();
        let lit = on && quick::torch_on();
        let (cx, cy) = (ui.w / 2.0, area.y + area.h * 0.34);
        let r = ui.w * 0.26;
        let red = rgb(0xF0566A);
        ui.cv.circle(
            cx,
            cy,
            r + ui.dp(14.0),
            red.alpha(if lit { 0.35 } else { 0.1 }),
        );
        ui.cv.circle(cx, cy, r, if on { red } else { ui.th.card });
        ui.label(
            Rect::new(cx - r, cy - ui.dp(24.0), 2.0 * r, ui.dp(48.0)),
            "SOS",
            38.0,
            if on { rgb(0xFFFFFF) } else { ui.th.text },
            true,
            Al::Center,
        );
        if ui.tap(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r)) {
            if on {
                self.mx.sos = None;
                quick::set_torch(false);
            } else {
                self.mx.sos = Some(Instant::now());
            }
            ui.dirty = true;
        }
        let mut y = cy + r + ui.dp(40.0);
        self.note(
            ui,
            &mut y,
            self.t(if on {
                "Flashing · · · – – – · · · with the camera light. Tap to stop."
            } else {
                "Tap to flash SOS in Morse code with the camera light."
            }),
        );
        if ui.row(
            &mut y,
            Some(Ic::Shield),
            self.t("Emergency"),
            Some(self.t("Emergency call and your information")),
            None,
            true,
        ) {
            self.go(Screen::More(Mx::Emergency));
            ui.dirty = true;
        }
    }

    fn s_focus(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Focus timer"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (cx, cy) = (ui.w / 2.0, area.y + area.h * 0.32);
        let r = ui.w * 0.3;
        ui.cv.ring(cx, cy, r, ui.dp(6.0), ui.th.line);
        let txt = match self.mx.focus {
            Some((end, work)) => {
                let left = end.saturating_duration_since(Instant::now()).as_secs();
                let total = if work {
                    self.mx.cfg.focus_min.max(1) as f32 * 60.0
                } else {
                    300.0
                };
                let frac = 1.0 - (left as f32 / total).clamp(0.0, 1.0);
                // Fortschritt als Punkte auf dem Ring
                let col = if work { ui.th.accent } else { rgb(0x34C759) };
                let n = (frac * 120.0) as i32;
                for k in 0..n {
                    let a = (k as f32 * 3.0).to_radians();
                    ui.cv
                        .circle(cx + a.sin() * r, cy - a.cos() * r, ui.dp(3.0), col);
                }
                ui.label(
                    Rect::new(0.0, cy + ui.dp(26.0), ui.w, ui.dp(22.0)),
                    self.t(if work { "Focus" } else { "Break" }),
                    14.0,
                    cs,
                    false,
                    Al::Center,
                );
                format!("{:02}:{:02}", left / 60, left % 60)
            }
            None => format!("{:02}:00", self.mx.cfg.focus_min),
        };
        ui.label(
            Rect::new(0.0, cy - ui.dp(30.0), ui.w, ui.dp(56.0)),
            &txt,
            46.0,
            c,
            true,
            Al::Center,
        );
        let mut y = cy + r + ui.dp(34.0);
        if self.mx.focus.is_none() {
            let d = ui.stepper(
                &mut y,
                self.t("Focus length"),
                &format!("{} min", self.mx.cfg.focus_min),
            );
            if d != 0 {
                self.mx.cfg.focus_min = (self.mx.cfg.focus_min as i32 + d * 5).clamp(5, 120) as u32;
                self.mx_save();
            }
        }
        let run = self.mx.focus.is_some();
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t(if run { "Stop" } else { "Start focus" }),
            if run { Btn::Plain } else { Btn::Primary },
        ) {
            if run {
                self.mx.focus = None;
                self.sx.cfg.dnd = self.mx.focus_prev_dnd;
            } else {
                self.mx.focus = Some((
                    Instant::now() + Duration::from_secs(self.mx.cfg.focus_min as u64 * 60),
                    true,
                ));
                self.mx.focus_prev_dnd = self.sx.cfg.dnd;
                self.sx.cfg.dnd = true;
            }
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        self.note(
            ui,
            &mut y,
            self.t("Do not disturb is on while you focus. After each round: 5 minutes break."),
        );
        if run {
            ui.dirty = true;
        }
    }

    fn s_countdown(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Countdown"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let d = self.mx_dt();
        let (c, cs) = (ui.th.text, ui.th.sub);
        let list = self.mx.cfg.countdowns.clone();
        for (i, (name, cy_, cm, cd)) in list.iter().enumerate() {
            let n = sysy::days_between((d.y, d.mo, d.d), (*cy_, *cm, *cd));
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(66.0));
            y += r.h + ui.dp(6.0);
            ui.card(r);
            let inn = r.inset(ui.dp(14.0));
            let nf = ui.fit(name, 16.0, true, inn.w - ui.dp(150.0));
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(10.0), inn.w, ui.dp(22.0)),
                &nf,
                16.0,
                c,
                true,
                Al::Start,
            );
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(34.0), inn.w, ui.dp(20.0)),
                &format!("{:02}.{:02}.{}", cd, cm, cy_),
                12.5,
                cs,
                false,
                Al::Start,
            );
            let v = if n == 0 {
                self.t("Today").to_string()
            } else if n > 0 {
                format!("{n} {}", self.t("days"))
            } else {
                format!("{} {} {}", self.t("ago"), -n, self.t("days"))
            };
            let vw = ui.text_w(&v, 18.0, true);
            let dx = if ui.rtl {
                inn.x + ui.dp(44.0)
            } else {
                inn.right() - ui.dp(44.0) - vw
            };
            ui.label(
                Rect::new(dx, r.y, vw + 2.0, r.h),
                &v,
                18.0,
                if n >= 0 { ui.th.accent } else { cs },
                true,
                Al::Start,
            );
            let bx = if ui.rtl {
                inn.x + ui.dp(14.0)
            } else {
                inn.right() - ui.dp(14.0)
            };
            if ui.icon_btn(bx, r.cy(), ui.dp(15.0), Ic::Trash, rgb(0).alpha(0.0), cs) {
                self.mx.cfg.countdowns.remove(i);
                self.mx_save();
                ui.dirty = true;
                break;
            }
        }
        ui.section(&mut y, self.t("New countdown"));
        self.mx_field(
            ui,
            &mut y,
            "mx_cd_name",
            "Title",
            self.t("Trip, exam, birthday…"),
        );
        self.mx_field(ui, &mut y, "mx_cd_date", "Date (DD.MM.YYYY)", "01.01.2027");
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Add"),
            Btn::Primary,
        ) {
            match (
                sysy::parse_date(&self.f("mx_cd_date")),
                self.f("mx_cd_name").trim().to_string(),
            ) {
                (Some(dt), n) if !n.is_empty() => {
                    self.mx.cfg.countdowns.push((n, dt.0, dt.1, dt.2));
                    self.mx
                        .cfg
                        .countdowns
                        .sort_by_key(|c| sysy::jdn(c.1, c.2, c.3));
                    self.mx_save();
                    self.fields.remove("mx_cd_name");
                    self.fields.remove("mx_cd_date");
                    self.focus = None;
                    self.focus_reset = true;
                }
                _ => self.toast(self.t("Enter a title and a valid date").into()),
            }
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_todo(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Tasks"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let mut y0 = area.y;
        let fw = ui.w - ui.dp(24.0) - ui.dp(58.0);
        let fx = if ui.rtl {
            ui.dp(12.0) + ui.dp(58.0)
        } else {
            ui.dp(12.0)
        };
        ui.field(
            Rect::new(fx, y0, fw, ui.dp(48.0)),
            "mx_todo",
            &self.f("mx_todo"),
            self.t("New task"),
        );
        let bx = if ui.rtl {
            ui.dp(36.0)
        } else {
            ui.w - ui.dp(36.0)
        };
        let (a, oa) = (ui.th.accent, ui.th.on_accent);
        if ui.icon_btn(bx, y0 + ui.dp(25.0), ui.dp(24.0), Ic::Plus, a, oa) {
            let t = self.f("mx_todo").trim().to_string();
            if !t.is_empty() {
                self.mx.cfg.todos.insert(0, (t, false));
                self.mx_save();
                self.fields.remove("mx_todo");
            }
            ui.dirty = true;
        }
        y0 += ui.dp(60.0);
        let body = Rect::new(area.x, y0, area.w, area.bottom() - y0);
        ui.scroll_begin(body, sc);
        let mut y = body.y - *sc;
        let open = self.mx.cfg.todos.iter().filter(|t| !t.1).count();
        self.note(
            ui,
            &mut y,
            &format!(
                "{open} {} · {} {}",
                self.t("open"),
                self.mx.cfg.todos.len() - open,
                self.t("done")
            ),
        );
        let list = self.mx.cfg.todos.clone();
        for (i, (text, done)) in list.iter().enumerate() {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(56.0));
            y += r.h + ui.dp(6.0);
            if !ui.visible(r) {
                continue;
            }
            let held = ui.held(r);
            ui.card_h(r, held);
            let kx = if ui.rtl {
                r.right() - ui.dp(28.0)
            } else {
                r.x + ui.dp(28.0)
            };
            if *done {
                ui.cv.circle(kx, r.cy(), ui.dp(12.0), ui.th.accent);
                ui.icon(Ic::Check, kx, r.cy(), ui.dp(13.0), ui.th.on_accent);
            } else {
                ui.cv.ring(kx, r.cy(), ui.dp(12.0), ui.dp(2.0), cs);
            }
            let tx = Rect::new(
                if ui.rtl {
                    r.x + ui.dp(48.0)
                } else {
                    r.x + ui.dp(54.0)
                },
                r.y,
                r.w - ui.dp(102.0),
                r.h,
            );
            let tf = ui.fit(text, 15.0, false, tx.w);
            ui.label(tx, &tf, 15.0, if *done { cs } else { c }, false, Al::Start);
            if *done {
                let tw = ui.text_w(&tf, 15.0, false);
                let lx = if ui.rtl { tx.right() - tw } else { tx.x };
                ui.cv.line(lx, r.cy(), lx + tw, r.cy(), 1.0, cs);
            }
            let dx = if ui.rtl {
                r.x + ui.dp(24.0)
            } else {
                r.right() - ui.dp(24.0)
            };
            if ui.icon_btn(
                dx,
                r.cy(),
                ui.dp(15.0),
                Ic::Trash,
                rgb(0).alpha(0.0),
                cs.alpha(0.7),
            ) {
                self.mx.cfg.todos.remove(i);
                self.mx_save();
                ui.dirty = true;
                break;
            } else if ui.tap(r) {
                self.mx.cfg.todos[i].1 = !done;
                // erledigte ans Ende
                self.mx.cfg.todos.sort_by_key(|t| t.1);
                self.mx_save();
                ui.dirty = true;
                break;
            }
        }
        ui.scroll_end(body, y + *sc - body.y, sc);
    }

    fn s_reminders(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Reminders"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let now = sys::unix_now();
        let list = self.mx.cfg.reminders.clone();
        if list.is_empty() {
            self.note(ui, &mut y, self.t("No reminders"));
        }
        for (i, (at, text)) in list.iter().enumerate() {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(60.0));
            y += r.h + ui.dp(6.0);
            ui.card(r);
            let inn = r.inset(ui.dp(14.0));
            let tf = ui.fit(text, 15.0, true, inn.w - ui.dp(40.0));
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(9.0), inn.w - ui.dp(40.0), ui.dp(22.0)),
                &tf,
                15.0,
                c,
                true,
                Al::Start,
            );
            let dt = sys::from_unix(*at, self.set.tz_min);
            let left = (at - now).max(0) / 60;
            let sub = format!(
                "{:02}.{:02}. {} · {} {} min",
                dt.d,
                dt.mo,
                self.clock_str(dt.h, dt.mi),
                self.t("in"),
                left
            );
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(32.0), inn.w - ui.dp(40.0), ui.dp(18.0)),
                &sub,
                12.0,
                cs,
                false,
                Al::Start,
            );
            let bx = if ui.rtl {
                r.x + ui.dp(26.0)
            } else {
                r.right() - ui.dp(26.0)
            };
            if ui.icon_btn(bx, r.cy(), ui.dp(15.0), Ic::Trash, rgb(0).alpha(0.0), cs) {
                self.mx.cfg.reminders.remove(i);
                self.mx_save();
                ui.dirty = true;
                break;
            }
        }
        ui.section(&mut y, self.t("New reminder"));
        self.mx_field(
            ui,
            &mut y,
            "mx_rem_text",
            "Remind me of",
            self.t("Call back, take medicine…"),
        );
        self.note(ui, &mut y, self.t("In"));
        let opts = ["10 min", "30 min", "1 h", "3 h", "24 h"];
        let mins = [10i64, 30, 60, 180, 1440];
        if let Some(i) = ui.chips(&mut y, &opts, usize::MAX) {
            let t = self.f("mx_rem_text").trim().to_string();
            if t.is_empty() {
                self.toast(self.t("Enter a text first").into());
            } else {
                self.mx.cfg.reminders.push((now + mins[i] * 60, t));
                self.mx.cfg.reminders.sort_by_key(|r| r.0);
                self.mx_save();
                self.fields.remove("mx_rem_text");
                self.focus = None;
                self.focus_reset = true;
            }
            ui.dirty = true;
        }
        self.note(
            ui,
            &mut y,
            self.t("Reminders also ring when the screen is off."),
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_emergency(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Emergency"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let red = rgb(0xF0566A);
        let num = self.mx.cfg.emergency_number.clone();
        let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(72.0));
        let held = ui.held(r);
        ui.cv.rrect(
            r,
            ui.dp(20.0),
            if held { red.mix(rgb(0), 0.2) } else { red },
        );
        ui.icon(
            Ic::Phone,
            if ui.rtl {
                r.right() - ui.dp(36.0)
            } else {
                r.x + ui.dp(36.0)
            },
            r.cy(),
            ui.dp(26.0),
            rgb(0xFFFFFF),
        );
        ui.label(
            r,
            &format!("{} {num}", self.t("Emergency call")),
            20.0,
            rgb(0xFFFFFF),
            true,
            Al::Center,
        );
        if ui.tap(r) && !num.is_empty() {
            self.phone.number = num.clone();
            self.phone.tel.dial(&num);
            self.go(Screen::Phone);
            ui.dirty = true;
        }
        y += r.h + ui.dp(10.0);
        let con = self.mx.cfg.emergency_contact.clone();
        if !con.is_empty()
            && ui.row(
                &mut y,
                Some(Ic::Person),
                self.t("Call emergency contact"),
                Some(&con),
                None,
                true,
            )
        {
            self.phone.number = con.clone();
            self.phone.tel.dial(&con);
            self.go(Screen::Phone);
            ui.dirty = true;
        }
        if ui.row(
            &mut y,
            Some(Ic::Torch),
            self.t("SOS signal"),
            Some(self.t("Flash SOS with the camera light")),
            None,
            true,
        ) {
            self.go(Screen::More(Mx::Sos));
            ui.dirty = true;
        }
        if !self.mx.cfg.medical.trim().is_empty() {
            let med = self.mx.cfg.medical.clone();
            self.mx_result(ui, &mut y, self.t("Medical information"), &med);
        }
        ui.section(&mut y, self.t("Your emergency data"));
        self.mx_field(ui, &mut y, "mx_enum", "Emergency number", "112");
        self.mx_field(ui, &mut y, "mx_econ", "Emergency contact (number)", "+49 …");
        self.mx_field(
            ui,
            &mut y,
            "mx_med",
            "Medical information",
            self.t("Blood group, allergies, medicines"),
        );
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Save"),
            Btn::Primary,
        ) {
            self.mx.cfg.emergency_number = self.f("mx_enum").trim().to_string();
            self.mx.cfg.emergency_contact = self.f("mx_econ").trim().to_string();
            self.mx.cfg.medical = self.f("mx_med").trim().to_string();
            self.mx_save();
            self.focus = None;
            self.focus_reset = true;
            self.toast(self.t("Saved").into());
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        self.note(
            ui,
            &mut y,
            self.t("Stays on this device. Shown here without unlocking other apps."),
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// Zuletzt verwendet wie in der Web-Demo: dunkle Fläche, Karten nebeneinander (wischen), ✕ schließt,
    /// langer Druck heftet an
    fn s_recents(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let (c, cs) = (rgb(0xF2F0EA), rgb(0xA6A6AE));
        ui.cv.rect(area, rgb(0x060709));
        let m = ui.dp(22.0);
        let list = self.mx.recents.clone();
        let ty = area.y + ui.dp(10.0);
        ui.label(
            Rect::new(m, ty, ui.w - 2.0 * m, ui.dp(32.0)),
            self.t("Recent apps"),
            17.0,
            c,
            true,
            Al::Start,
        );
        if !list.is_empty() {
            let lab = self.t("Close all");
            let lw = ui.text_w(lab, 13.0, false) + ui.dp(26.0);
            let px = if ui.rtl { m } else { ui.w - m - lw };
            if ui.pill(px, ty, lab, false).1 {
                self.mx.recents.clear();
                ui.dirty = true;
                return;
            }
        }
        let hint_h = ui.dp(40.0);
        let ly = ty + ui.dp(42.0);
        let lh = area.bottom() - hint_h - ly;
        if list.is_empty() {
            ui.label(
                Rect::new(0.0, ly, ui.w, lh),
                self.t("No open apps"),
                14.0,
                cs,
                false,
                Al::Center,
            );
            return;
        }
        let cw = ui.dp(230.0).min(ui.w * 0.62);
        let ch = (lh - ui.dp(60.0)).min(ui.dp(470.0)).max(ui.dp(200.0));
        let gap = ui.dp(16.0);
        let total = list.len() as f32 * (cw + gap) - gap;
        let max = (total - (ui.w - 2.0 * ui.dp(40.0))).max(0.0);
        // waagerecht wischen
        if ui.p.down && ui.p.moved && ui.p.capture.is_none() && ui.p.py > ly && ui.p.py < ly + lh {
            *sc = (*sc - ui.p.dx).clamp(0.0, max);
            ui.p.dx = 0.0;
            ui.dirty = true;
        }
        let cy = ly + (lh - ch) / 2.0;
        let x0 = if total < ui.w - 2.0 * ui.dp(40.0) {
            (ui.w - total) / 2.0
        } else {
            ui.dp(40.0) - *sc
        };
        let all = self.all_apps();
        for (i, (name, scr)) in list.iter().enumerate() {
            let k = if ui.rtl { list.len() - 1 - i } else { i };
            let r = Rect::new(x0 + k as f32 * (cw + gap), cy, cw, ch);
            if r.right() < 0.0 || r.x > ui.w {
                continue;
            }
            ui.cv.rrect(r, ui.dp(26.0), ui.th.line);
            ui.cv.rrect(r.inset(1.0), ui.dp(25.0), rgb(0x14161B));
            let ic = all
                .iter()
                .find(|a| a.1 == name.as_str())
                .map(|a| a.0)
                .unwrap_or(Ic::Gear);
            let lab = self.app_label(name);
            let hx = if ui.rtl {
                r.right() - ui.dp(22.0)
            } else {
                r.x + ui.dp(22.0)
            };
            ui.icon(ic, hx, r.y + ui.dp(25.0), ui.dp(16.0), ui.th.accent);
            let tx = Rect::new(
                if ui.rtl {
                    r.x + ui.dp(44.0)
                } else {
                    r.x + ui.dp(38.0)
                },
                r.y + ui.dp(12.0),
                r.w - ui.dp(82.0),
                ui.dp(26.0),
            );
            let lf = ui.fit(&lab, 13.0, true, tx.w);
            ui.label(tx, &lf, 13.0, c, true, Al::Start);
            let xx = if ui.rtl {
                r.x + ui.dp(24.0)
            } else {
                r.right() - ui.dp(24.0)
            };
            let closed = ui.icon_btn(
                xx,
                r.y + ui.dp(25.0),
                ui.dp(13.0),
                Ic::Close,
                rgb(0).alpha(0.0),
                c,
            );
            let body = Rect::new(
                r.x + ui.dp(8.0),
                r.y + ui.dp(48.0),
                r.w - ui.dp(16.0),
                r.h - ui.dp(56.0),
            );
            ui.cv.rrect(body, ui.dp(18.0), rgb(0x0A0B0E));
            ui.icon(ic, body.cx(), body.cy(), ui.dp(54.0), ui.th.accent);
            if closed {
                self.mx.recents.remove(i);
                ui.dirty = true;
                return;
            }
            if self.long_press(ui, r) {
                self.home();
                self.go(scr.clone());
                self.mx.pinned = true;
                self.toast(self.t("Pinned – hold Home to release").into());
                ui.dirty = true;
                return;
            }
            if ui.tap(r) {
                self.home();
                self.go(scr.clone());
                ui.dirty = true;
                return;
            }
        }
        ui.label(
            Rect::new(0.0, area.bottom() - hint_h, ui.w, hint_h),
            self.t("Tap to switch · × to close"),
            12.0,
            rgb(0x8E8E97),
            false,
            Al::Center,
        );
    }

    fn s_clipboard(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Clipboard"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let list = self.mx.cfg.clip.clone();
        if list.is_empty() {
            self.note(
                ui,
                &mut y,
                self.t("Nothing copied yet. Use “Copy” above the keyboard or tap a result."),
            );
        }
        for (i, t) in list.iter().enumerate() {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(56.0));
            y += r.h + ui.dp(6.0);
            if !ui.visible(r) {
                continue;
            }
            let held = ui.held(r);
            ui.card_h(r, held);
            let one: String = t.replace('\n', " ");
            let tw = r.w - ui.dp(70.0);
            let tf = ui.fit(&one, 14.5, i == 0, tw);
            let tx = if ui.rtl {
                r.x + ui.dp(56.0)
            } else {
                r.x + ui.dp(14.0)
            };
            ui.label(
                Rect::new(tx, r.y, tw, r.h),
                &tf,
                14.5,
                if i == 0 { c } else { cs.mix(c, 0.5) },
                i == 0,
                Al::Start,
            );
            let bx = if ui.rtl {
                r.x + ui.dp(26.0)
            } else {
                r.right() - ui.dp(26.0)
            };
            if ui.icon_btn(bx, r.cy(), ui.dp(15.0), Ic::Trash, rgb(0).alpha(0.0), cs) {
                self.mx.cfg.clip.remove(i);
                self.mx_save();
                ui.dirty = true;
                break;
            } else if ui.tap(r) && i > 0 {
                // nach vorn holen = wird als Nächstes eingefügt
                let t = self.mx.cfg.clip.remove(i);
                self.mx.cfg.clip.insert(0, t);
                self.mx_save();
                ui.dirty = true;
                break;
            }
        }
        if !list.is_empty() {
            self.note(
                ui,
                &mut y,
                self.t("The first entry is pasted. Tap another one to move it to the front."),
            );
            if ui.button(
                Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0)),
                self.t("Clear all"),
                Btn::Plain,
            ) {
                self.mx.cfg.clip.clear();
                self.mx_save();
                ui.dirty = true;
            }
            y += ui.dp(58.0);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_batthist(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Battery history"));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let mut y = area.y;
        let hist = self.mx.stats.batt.clone();
        let est = sysy::battery_estimate(&hist);
        let now_pct = sys::battery()
            .map(|b| format!("{} %", b.0))
            .unwrap_or_else(|| "—".into());
        let etxt = match est {
            Some((true, m)) => format!(
                "{} {}",
                self.t("Full in about"),
                sysy::fmt_dur(m as u64 * 60)
            ),
            Some((false, m)) => {
                format!("{} {}", self.t("Lasts about"), sysy::fmt_dur(m as u64 * 60))
            }
            None => self.t("Estimate after about 15 minutes of use").to_string(),
        };
        self.mx_result(ui, &mut y, &etxt, &now_pct);
        // Verlauf der letzten 24 h
        let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(220.0));
        ui.card(r);
        let g = Rect::new(
            r.x + ui.dp(38.0),
            r.y + ui.dp(16.0),
            r.w - ui.dp(54.0),
            r.h - ui.dp(46.0),
        );
        for p in [0, 50, 100] {
            let yy = g.bottom() - g.h * p as f32 / 100.0;
            ui.cv.line(g.x, yy, g.right(), yy, 1.0, ui.th.line);
            ui.label(
                Rect::new(r.x + ui.dp(4.0), yy - ui.dp(8.0), ui.dp(30.0), ui.dp(16.0)),
                &format!("{p}"),
                10.5,
                cs,
                false,
                Al::End,
            );
        }
        let now = sys::unix_now();
        let span = 24.0 * 3600.0;
        let px = |t: i64| g.right() - g.w * ((now - t) as f32 / span).clamp(0.0, 1.0);
        let mut prev: Option<(f32, f32)> = None;
        for (t, pct, chg) in hist.iter().filter(|s| now - s.0 <= span as i64) {
            let (x, yy) = (px(*t), g.bottom() - g.h * *pct as f32 / 100.0);
            if let Some((qx, qy)) = prev {
                ui.cv.line(
                    qx,
                    qy,
                    x,
                    yy,
                    ui.dp(2.5),
                    if *chg { rgb(0x34C759) } else { ui.th.accent },
                );
            }
            prev = Some((x, yy));
        }
        for (k, l) in [(24, "-24 h"), (12, "-12 h"), (0, "")] {
            let x = g.right() - g.w * k as f32 / 24.0;
            ui.label(
                Rect::new(
                    x - ui.dp(30.0),
                    g.bottom() + ui.dp(6.0),
                    ui.dp(60.0),
                    ui.dp(16.0),
                ),
                if k == 0 { self.t("right now") } else { l },
                10.5,
                cs,
                false,
                Al::Center,
            );
        }
        if prev.is_none() {
            ui.label(
                g,
                self.t("Collecting data – one point every 10 minutes"),
                12.5,
                cs,
                false,
                Al::Center,
            );
        }
        y += r.h + ui.dp(10.0);
        ui.label(
            Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(20.0)),
            &format!(
                "{} · {}",
                self.t("gold = on battery"),
                self.t("green = charging")
            ),
            12.0,
            cs,
            false,
            Al::Start,
        );
        let _ = c;
    }

    fn s_usage(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Screen time"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let d = self.mx_dt();
        let tab = self.mx.usage_tab.min(1);
        if let Some(i) = ui.chips(&mut y, &[self.t("Today"), self.t("7 days")], tab) {
            self.mx.usage_tab = i;
        }
        let today = (d.y, d.mo, d.d);
        let days: Vec<String> = (0..if tab == 0 { 1 } else { 7 })
            .map(|k| {
                let x = sysy::add_days(today, -k);
                format!("{:04}-{:02}-{:02}", x.0, x.1, x.2)
            })
            .collect();
        let mut sum: std::collections::BTreeMap<String, u64> = Default::default();
        let (mut wakes, mut unlocks) = (0u32, 0u32);
        for k in &days {
            for (a, s) in self.mx.stats.day_usage(k) {
                *sum.entry(a).or_default() += s;
            }
            if let Some(w) = self.mx.stats.wakes.get(k) {
                wakes += w.0;
                unlocks += w.1;
            }
        }
        let total: u64 = sum.values().sum();
        self.mx_result(ui, &mut y, self.t("Screen on"), &sysy::fmt_dur(total));
        let _ = ui.row(
            &mut y,
            Some(Ic::Sun),
            self.t("Screen switched on"),
            None,
            Some(&format!("{wakes}×")),
            false,
        );
        let _ = ui.row(
            &mut y,
            Some(Ic::Lock),
            self.t("Unlocked"),
            None,
            Some(&format!("{unlocks}×")),
            false,
        );
        ui.section(&mut y, self.t("By app"));
        let mut v: Vec<(String, u64)> = sum.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        let max = v.first().map(|x| x.1).unwrap_or(1).max(1);
        for (name, secs) in v.iter().take(12) {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(52.0));
            y += r.h + ui.dp(6.0);
            ui.card(r);
            let inn = r.inset(ui.dp(14.0));
            let lab = self.app_label(name);
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(6.0), inn.w, ui.dp(20.0)),
                &lab,
                14.0,
                c,
                true,
                Al::Start,
            );
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(6.0), inn.w, ui.dp(20.0)),
                &sysy::fmt_dur(*secs),
                12.5,
                cs,
                false,
                Al::End,
            );
            self.bar(
                ui,
                Rect::new(inn.x, r.y + ui.dp(34.0), inn.w, ui.dp(6.0)),
                *secs as f32 / max as f32,
            );
        }
        if v.is_empty() {
            self.note(ui, &mut y, self.t("No data yet"));
        }
        self.note(
            ui,
            &mut y,
            self.t("Counted on this device only. Nothing is sent."),
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_care(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Device care"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (dt, df) = sys::disk("/data").unwrap_or((1, 1));
        let (mt, ma) = sys::meminfo();
        let b = sys::battery_info();
        let h = sysy::Health {
            storage_free_pct: df as f32 * 100.0 / dt.max(1) as f32,
            ram_free_pct: ma as f32 * 100.0 / mt.max(1) as f32,
            batt_health: None,
            temp_c: sysx::hottest_zone().map(|z| z.1),
            backup_age_days: self.backup_age_days(),
            uptime_days: sys::uptime_s() as f32 / 86_400.0,
            has_pin: !self.sx.cfg.pin_hash.is_empty(),
        };
        let (score, tips) = sysy::health_score(&h);
        let col = if score >= 80 {
            rgb(0x34C759)
        } else if score >= 50 {
            ui.th.accent
        } else {
            rgb(0xF0566A)
        };
        let (cx, cy, r) = (ui.w / 2.0, y + ui.dp(84.0), ui.dp(64.0));
        ui.cv.ring(cx, cy, r, ui.dp(8.0), ui.th.line);
        for k in 0..(score as i32 * 120 / 100) {
            let a = (k as f32 * 3.0).to_radians();
            ui.cv
                .circle(cx + a.sin() * r, cy - a.cos() * r, ui.dp(4.0), col);
        }
        ui.label(
            Rect::new(cx - r, cy - ui.dp(26.0), 2.0 * r, ui.dp(44.0)),
            &score.to_string(),
            38.0,
            c,
            true,
            Al::Center,
        );
        ui.label(
            Rect::new(cx - r, cy + ui.dp(16.0), 2.0 * r, ui.dp(18.0)),
            self.t(if score >= 80 {
                "Good"
            } else if score >= 50 {
                "Fair"
            } else {
                "Needs care"
            }),
            12.5,
            col,
            true,
            Al::Center,
        );
        y += ui.dp(176.0);
        if tips.is_empty() {
            self.note(ui, &mut y, self.t("Everything looks good."));
        }
        for t in tips {
            let target = if t.contains("backup") || t.contains("Backup") {
                Some(Screen::Sys(super::app_sys::Sx::Backup))
            } else if t.contains("Storage") {
                Some(Screen::More(Mx::StorageX))
            } else if t.contains("Memory") {
                Some(Screen::More(Mx::Tasks))
            } else if t.contains("PIN") {
                Some(Screen::Sys(super::app_sys::Sx::LockSet))
            } else {
                None
            };
            if ui.row(
                &mut y,
                Some(Ic::Info),
                self.t(t),
                None,
                None,
                target.is_some(),
            ) {
                if let Some(s) = target {
                    self.go(s);
                    ui.dirty = true;
                }
            }
        }
        ui.section(&mut y, self.t("Details"));
        let rows: [(Ic, &str, String, Option<Mx>); 5] = [
            (
                Ic::Disk,
                "Storage",
                format!("{} {}", sys::human(df), self.t("free")),
                Some(Mx::StorageX),
            ),
            (
                Ic::Chip,
                "Memory",
                format!("{} {}", sys::human(ma), self.t("free")),
                Some(Mx::Tasks),
            ),
            (
                Ic::Chip,
                "Battery",
                b.map(|b| format!("{} %", b.percent))
                    .unwrap_or_else(|| "—".into()),
                Some(Mx::BattHist),
            ),
            (
                Ic::Sun,
                "Temperature",
                h.temp_c
                    .map(|t| format!("{t:.0} °C"))
                    .unwrap_or_else(|| "—".into()),
                None,
            ),
            (
                Ic::Clock,
                "Running since",
                sysy::fmt_dur(sys::uptime_s()),
                None,
            ),
        ];
        for (ic, name, val, target) in rows {
            if ui.row(
                &mut y,
                Some(ic),
                self.t(name),
                None,
                Some(&val),
                target.is_some(),
            ) {
                if let Some(t) = target {
                    self.go(Screen::More(t));
                    ui.dirty = true;
                }
            }
        }
        if ui.row(
            &mut y,
            Some(Ic::Clock),
            self.t("Screen time"),
            None,
            None,
            true,
        ) {
            self.go(Screen::More(Mx::Usage));
            ui.dirty = true;
        }
        let _ = cs;
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_storagex(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Storage analysis"));
        if self.mx.big.is_none() {
            self.mx.big = Some(sysy::biggest("/data", 25));
        }
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        if let Some((total, free)) = sys::disk("/data") {
            self.mx_result(
                ui,
                &mut y,
                &format!("{} {}", self.t("free of"), sys::human(total)),
                &sys::human(free),
            );
        }
        self.note(ui, &mut y, self.t("Largest folders and files in your data"));
        let list = self.mx.big.clone().unwrap_or_default();
        let max = list.first().map(|x| x.1).unwrap_or(1).max(1);
        for (name, size, is_dir) in list.iter().filter(|x| x.1 > 0) {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(56.0));
            y += r.h + ui.dp(6.0);
            if !ui.visible(r) {
                continue;
            }
            let held = ui.held(r);
            ui.card_h(r, held);
            let inn = r.inset(ui.dp(14.0));
            let (ic, hue) = super::app_modern::file_kind(name, *is_dir);
            ui.icon(
                ic,
                if ui.rtl {
                    inn.right() - ui.dp(10.0)
                } else {
                    inn.x + ui.dp(10.0)
                },
                r.y + ui.dp(20.0),
                ui.dp(17.0),
                rgb(hue),
            );
            let tx = Rect::new(
                if ui.rtl { inn.x } else { inn.x + ui.dp(30.0) },
                r.y + ui.dp(8.0),
                inn.w - ui.dp(30.0),
                ui.dp(22.0),
            );
            let nf = ui.fit(name, 14.5, true, tx.w - ui.dp(90.0));
            ui.label(tx, &nf, 14.5, c, true, Al::Start);
            ui.label(tx, &sys::human(*size), 12.5, cs, false, Al::End);
            self.bar(
                ui,
                Rect::new(inn.x, r.y + ui.dp(38.0), inn.w, ui.dp(6.0)),
                *size as f32 / max as f32,
            );
            if *is_dir && ui.tap(r) {
                self.tl.cwd = format!("/data/{name}");
                self.go(Screen::Files);
                ui.dirty = true;
            }
        }
        if ui.row(
            &mut y,
            Some(Ic::Trash),
            self.t("Clean up"),
            Some(self.t("Caches and old program versions")),
            None,
            true,
        ) {
            self.go(Screen::Sys(super::app_sys::Sx::Backup));
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_tasks(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Running programs"));
        if self
            .mx
            .procs_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.mx.procs = sysy::processes(30);
            self.mx.procs_at = Some(Instant::now());
        }
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let (c, cs) = (ui.th.text, ui.th.sub);
        let (mt, ma) = sys::meminfo();
        self.mx_result(
            ui,
            &mut y,
            &format!("{} {}", self.t("free of"), sys::human(mt)),
            &sys::human(ma),
        );
        let list = self.mx.procs.clone();
        for (pid, name, rss) in list {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(54.0));
            y += r.h + ui.dp(6.0);
            if !ui.visible(r) {
                continue;
            }
            ui.card(r);
            let inn = r.inset(ui.dp(14.0));
            let prot = pid <= 1 || pid == std::process::id() || sysy::protected_process(&name);
            let nf = ui.fit(&name, 14.5, true, inn.w - ui.dp(150.0));
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(7.0), inn.w, ui.dp(20.0)),
                &nf,
                14.5,
                c,
                true,
                Al::Start,
            );
            ui.label(
                Rect::new(inn.x, r.y + ui.dp(28.0), inn.w, ui.dp(18.0)),
                &format!("{} · PID {pid}", sys::human(rss)),
                12.0,
                cs,
                false,
                Al::Start,
            );
            if prot {
                let w = ui.dp(70.0);
                ui.label(
                    Rect::new(if ui.rtl { inn.x } else { inn.right() - w }, r.y, w, r.h),
                    self.t("System"),
                    11.5,
                    cs,
                    false,
                    Al::Center,
                );
            } else {
                let w = ui.dp(84.0);
                let br = Rect::new(
                    if ui.rtl { inn.x } else { inn.right() - w },
                    r.cy() - ui.dp(18.0),
                    w,
                    ui.dp(36.0),
                );
                if ui.button(br, self.t("Stop"), Btn::Plain) {
                    // erst höflich (SIGTERM); wer nicht reagiert, taucht wieder auf und kann erneut beendet werden
                    unsafe {
                        nix::libc::kill(pid as i32, nix::libc::SIGTERM);
                    }
                    self.mx.procs_at = None;
                    self.toast(format!("{name}: {}", self.t("stop requested")));
                    ui.dirty = true;
                }
            }
        }
        self.note(
            ui,
            &mut y,
            self.t("System programs cannot be stopped here."),
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_modes(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Modes & automation"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut ch = false;
        ui.section(&mut y, self.t("Modes"));
        let mut sleep = self.mx.cfg.sleep_mode;
        if ui.toggle_row(
            &mut y,
            self.t("Sleep mode"),
            Some(self.t("Do not disturb, night light and dim screen with one switch")),
            &mut sleep,
        ) {
            if sleep {
                self.mx.cfg.sleep_prev = (self.set.brightness, self.sx.cfg.night, self.sx.cfg.dnd);
                self.set.brightness = self.set.brightness.min(0.2);
                self.sx.cfg.night = true;
                self.sx.cfg.dnd = true;
            } else {
                let p = self.mx.cfg.sleep_prev;
                self.set.brightness = p.0;
                self.sx.cfg.night = p.1;
                self.sx.cfg.dnd = p.2;
            }
            self.mx.cfg.sleep_mode = sleep;
            self.apply_brightness();
            self.save();
            let _ = sys::save("system.toml", &self.sx.cfg);
            ch = true;
        }
        let mut b = self.mx.cfg.auto_theme;
        if ui.toggle_row(
            &mut y,
            self.t("Automatic theme"),
            Some(self.t("Light by day, dark at night")),
            &mut b,
        ) {
            self.mx.cfg.auto_theme = b;
            self.mx.last_min = 0;
            ch = true;
        }
        if self.mx.cfg.auto_theme {
            let d = ui.stepper(
                &mut y,
                self.t("Day starts"),
                &sysy::fmt_hm(self.mx.cfg.day_from),
            );
            if d != 0 {
                self.mx.cfg.day_from =
                    (self.mx.cfg.day_from as i32 + d * 30).rem_euclid(1440) as u16;
                ch = true;
            }
            let d = ui.stepper(
                &mut y,
                self.t("Night starts"),
                &sysy::fmt_hm(self.mx.cfg.day_to),
            );
            if d != 0 {
                self.mx.cfg.day_to = (self.mx.cfg.day_to as i32 + d * 30).rem_euclid(1440) as u16;
                ch = true;
            }
        }
        ui.section(&mut y, self.t("Automation"));
        let mut b = self.mx.cfg.restart_on;
        if ui.toggle_row(
            &mut y,
            self.t("Scheduled restart"),
            Some(self.t("Keeps the system fresh; never during a call")),
            &mut b,
        ) {
            self.mx.cfg.restart_on = b;
            ch = true;
        }
        if self.mx.cfg.restart_on {
            let d = ui.stepper(
                &mut y,
                self.t("Time"),
                &sysy::fmt_hm(self.mx.cfg.restart_at),
            );
            if d != 0 {
                self.mx.cfg.restart_at =
                    (self.mx.cfg.restart_at as i32 + d * 30).rem_euclid(1440) as u16;
                ch = true;
            }
            let days = [
                self.t("Daily"),
                self.t("Mo"),
                self.t("Tu"),
                self.t("We"),
                self.t("Th"),
                self.t("Fr"),
                self.t("Sa"),
                self.t("Su"),
            ];
            if let Some(i) = ui.chips(
                &mut y,
                &days[..4],
                if self.mx.cfg.restart_day < 4 {
                    self.mx.cfg.restart_day as usize
                } else {
                    usize::MAX
                },
            ) {
                self.mx.cfg.restart_day = i as u8;
                ch = true;
            }
            if let Some(i) = ui.chips(
                &mut y,
                &days[4..],
                if self.mx.cfg.restart_day >= 4 {
                    self.mx.cfg.restart_day as usize - 4
                } else {
                    usize::MAX
                },
            ) {
                self.mx.cfg.restart_day = i as u8 + 4;
                ch = true;
            }
        }
        let toggles: [(&str, &str, u8); 6] = [
            (
                "Hourly signal",
                "Short vibration on the hour (not during do not disturb)",
                0,
            ),
            (
                "Charge alert at 80 %",
                "Unplugging early protects the battery",
                1,
            ),
            (
                "Pocket protection",
                "Do not wake the screen while the proximity sensor is covered",
                2,
            ),
            (
                "Low storage warning",
                "Notify when less than 5 % is free",
                3,
            ),
            (
                "Backup reminder",
                "When the last backup is older than 30 days",
                4,
            ),
            ("Break reminder", "After a long time at the screen", 5),
        ];
        for (name, sub, k) in toggles {
            let c = &self.mx.cfg;
            let mut v = [
                c.hourly_chime,
                c.charge_alert,
                c.pocket,
                c.storage_warn,
                c.backup_remind,
                c.break_min > 0,
            ][k as usize];
            if ui.toggle_row(&mut y, self.t(name), Some(self.t(sub)), &mut v) {
                let c = &mut self.mx.cfg;
                match k {
                    0 => c.hourly_chime = v,
                    1 => c.charge_alert = v,
                    2 => c.pocket = v,
                    3 => c.storage_warn = v,
                    4 => c.backup_remind = v,
                    _ => c.break_min = if v { 45 } else { 0 },
                }
                ch = true;
            }
        }
        if self.mx.cfg.break_min > 0 {
            let d = ui.stepper(
                &mut y,
                self.t("Remind after"),
                &format!("{} min", self.mx.cfg.break_min),
            );
            if d != 0 {
                self.mx.cfg.break_min =
                    (self.mx.cfg.break_min as i32 + d * 15).clamp(15, 240) as u32;
                ch = true;
            }
        }
        ui.section(&mut y, self.t("Lock screen"));
        self.mx_field(
            ui,
            &mut y,
            "mx_owner",
            "Owner info on the lock screen",
            self.t("Name, phone number if found"),
        );
        if self.f("mx_owner") != self.mx.cfg.owner_info && self.focus != Some("mx_owner") {
            self.mx.cfg.owner_info = self.f("mx_owner");
            ch = true;
        }
        ui.section(&mut y, self.t("More"));
        let links: [(Ic, &str, Mx); 5] = [
            (Ic::Phone, "Blocked numbers", Mx::Blocked),
            (Ic::Shield, "Emergency", Mx::Emergency),
            (Ic::Clock, "Screen time", Mx::Usage),
            (Ic::Chip, "Battery history", Mx::BattHist),
            (Ic::List, "Clipboard", Mx::Clipboard),
        ];
        for (ic, name, t) in links {
            if ui.row(&mut y, Some(ic), self.t(name), None, None, true) {
                self.go(Screen::More(t));
                ui.dirty = true;
            }
        }
        if ch {
            self.mx_save();
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_blocked(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Blocked numbers"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let cs = ui.th.sub;
        self.mx_field(ui, &mut y, "mx_block", "Number", "+49 …");
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Block"),
            Btn::Primary,
        ) {
            let n = self.f("mx_block").trim().to_string();
            if n.chars().filter(|c| c.is_ascii_digit()).count() >= 3 && !self.more_blocked(&n) {
                self.mx.cfg.blocked.push(n);
                self.mx_save();
                self.fields.remove("mx_block");
                self.focus = None;
                self.focus_reset = true;
            }
            ui.dirty = true;
        }
        y += ui.dp(62.0);
        let list = self.mx.cfg.blocked.clone();
        for (i, n) in list.iter().enumerate() {
            let name = self.contact_name(n);
            if ui.row(
                &mut y,
                Some(Ic::Phone),
                name.as_deref().unwrap_or(n),
                if name.is_some() { Some(n) } else { None },
                Some(self.t("Unblock")),
                false,
            ) {
                self.mx.cfg.blocked.remove(i);
                self.mx_save();
                ui.dirty = true;
                break;
            }
        }
        if list.is_empty() {
            self.note(ui, &mut y, self.t("No blocked numbers"));
        }
        let hh = ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("Calls from blocked numbers are rejected silently and noted in the notification history."), 12.5, cs);
        y += hh + ui.dp(10.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::Bg;

    fn app() -> App {
        // gleiches Datenverzeichnis wie app::flow_tests (Umgebungsvariablen gelten für alle Tests des Prozesses)
        let root = std::env::temp_dir().join(format!("ultar-flow-{}", std::process::id()));
        std::fs::create_dir_all(root.join("ultar")).unwrap();
        std::env::set_var("ULTAR_DRY", "1");
        std::env::set_var("ULTAR_MEDIA", &root);
        std::env::set_var("ULTAR_DATA", root.join("ultar"));
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_rfd, wfd) =
            nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC)
                .unwrap();
        let mut a = App::new(
            Bg {
                tx,
                wake: std::sync::Arc::new(wfd),
            },
            (720, 1520, "test".into()),
        );
        a.debug_open("home", "de");
        a
    }

    #[test]
    fn recents_pinning_block_and_reminders() {
        let mut a = app();
        // zuletzt verwendet: neueste vorn, ohne Doppelte
        a.go(Screen::Phone);
        a.home();
        a.go(Screen::Files);
        a.home();
        a.go(Screen::Phone);
        let names: Vec<&str> = a.mx.recents.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(names, vec!["Phone", "Files"]);
        // anheften: Start und Zurück bleiben in der App
        a.mx.pinned = true;
        a.home();
        assert_eq!(a.stack.len(), 2);
        a.back();
        assert_eq!(a.stack.len(), 2);
        a.mx.pinned = false;
        a.home();
        assert_eq!(a.stack.len(), 1);
        // Sperrliste: Anruf wird abgelehnt, klingelt nicht
        a.mx.cfg.blocked = vec!["+49 151 0000000".into()];
        a.phone.tel.simulate_incoming("0151 0000000");
        a.poll_notifications(false);
        assert!(matches!(
            a.phone.tel.state(),
            super::super::app_phone::CallState::Idle
        ));
        assert!(a.phone.ringing.is_none());
        assert_eq!(a.stack.len(), 1, "kein Anrufbildschirm");
        // nicht gesperrte Nummer klingelt und öffnet den Anrufbildschirm
        a.phone.tel.simulate_incoming("0170 1234567");
        a.poll_notifications(false);
        assert!(matches!(a.stack.last().map(|s| &s.0), Some(Screen::Phone)));
        a.phone.tel.hangup();
        a.poll_notifications(false);
        a.home();
        // fällige Erinnerung wird gemeldet und entfernt, künftige bleibt
        let now = sys::unix_now();
        a.mx.cfg.reminders = vec![(now - 5, "alt".into()), (now + 3600, "neu".into())];
        a.mx.last_min = 0;
        a.more_tick();
        assert_eq!(a.mx.cfg.reminders.len(), 1);
        assert_eq!(a.mx.cfg.reminders[0].1, "neu");
        // wach halten wirkt auf den Bildschirm-Timeout
        assert!(!a.stay_awake());
        a.mx.caffeine = Some(Instant::now() + Duration::from_secs(60));
        assert!(a.stay_awake());
        // Zwischenablage
        a.copy_text("abc");
        a.copy_text("def");
        a.copy_text("abc");
        assert_eq!(&a.mx.cfg.clip[..2], &["abc".to_string(), "def".to_string()]);
    }
}
