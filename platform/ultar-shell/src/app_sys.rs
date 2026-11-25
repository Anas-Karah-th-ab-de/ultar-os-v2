//! Systemfunktionen in der Oberfläche (Untermodul von `app`): Sperrbildschirm mit PIN, Zeitzonen/
//! Zeitabgleich, Energie, Nachtlicht/Auto-Helligkeit, Tonprofile/Nicht stören, Benachrichtigungsverlauf,
//! Netzwerk-Werkzeuge, Protokolle, Sicherung/Werkseinstellungen, Entwickleroptionen sowie die Apps
//! Kalender, Weltuhr, Einheitenrechner und die globale Suche. Logik: `crate::sysx`, Dienst: `crate::sysd`.

use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::sys;
use crate::sysd;
use crate::sysx::{self, SysCfg};
use crate::ui::{Al, Btn, Ic, Ui};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Debug)]
pub enum Sx {
    LockSet,
    PinSet,
    Time,
    Battery,
    DisplayX,
    SoundX,
    NotifHist,
    NetTools,
    HiddenWifi,
    Logs,
    Backup,
    Dev,
    Calendar,
    EventEdit(Option<usize>),
    World,
    Units,
    Search,
    CalcHist,
}

pub struct SxState {
    pub cfg: SysCfg,
    pub locked: bool,
    pin: String,
    /// Sperrbildschirm zeigt das PIN-Feld (nach Wisch nach oben / Tippen auf den Fingerabdruck)
    pin_mode: bool,
    pin_new: Option<String>,
    fails: u32,
    wait_until: Option<Instant>,
    pub vol_shown: Option<Instant>,
    last_auto: Option<Instant>,
    last_zone_check: Option<Instant>,
    frames: Vec<Instant>,
    diag: Arc<Mutex<Vec<String>>>,
    diag_busy: Arc<Mutex<bool>>,
    log_tab: usize,
    logs: Vec<String>,
    cal_ym: (i64, i64),
    cal_day: i64,
    unit: (usize, usize, usize),
    msg: String,
    confirm: u8,
    cleanup: Vec<(String, u64)>,
    #[allow(dead_code)] // written but never read; kept, see docs/OFFEN-WARNUNGEN.md
    hist_tab: usize,
    snooze: Option<(Instant, String)>,
    pub rssi: Option<i32>,
    last_rssi: Option<Instant>,
    // Fingerabdruck: Verbindung zum Dienst, Modus (0 aus, 1 prüfen, 2 anlernen, 3 abfragen)
    fp: Option<crate::fp::FpConn>,
    fp_mode: u8,
    fp_info: String,
    fp_count: Option<u32>,
    fp_retry: Option<Instant>,
    // Gesicht: laufendes Programm, Lauf-Nummer, Modus (0 aus, 1 prüfen, 2 anlernen), Versuch seit Aufwachen
    face: Option<crate::face::FaceRun>,
    face_gen: u32,
    face_mode: u8,
    face_info: String,
    face_tried: bool,
    /// Entwickler-Zugang (SSH): zuletzt gelesener Stand (`ultar-sshd status`)
    ssh: Option<(Instant, EntwicklerSsh)>,
}

/// Stand des Entwickler-Zugangs (SSH) für die Einstellungen
#[derive(Clone, Default)]
pub struct EntwicklerSsh {
    pub vorhanden: bool,
    pub an: bool,
    pub port: String,
    pub fingerabdruck: String,
    pub schluessel: String,
    pub adresse: String,
}

impl EntwicklerSsh {
    fn lesen() -> Self {
        let mut e = EntwicklerSsh::default();
        let Ok(o) = std::process::Command::new("ultar-sshd")
            .arg("status")
            .output()
        else {
            return e;
        };
        e.vorhanden = true;
        for z in String::from_utf8_lossy(&o.stdout).lines() {
            let (k, v) = z.split_once(':').unwrap_or((z, ""));
            let v = v.trim().to_string();
            match k {
                "entwickler-ssh" => e.an = v == "an",
                "port" => e.port = v,
                "wirtsschlüssel" => e.fingerabdruck = v,
                "erlaubte schlüssel" => e.schluessel = v,
                _ => {}
            }
        }
        // eigene Adresse: Weg ins Netz erfragen (UDP-connect sendet nichts)
        e.adresse = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|u| u.connect("1.1.1.1:53").and_then(|_| u.local_addr()))
            .map(|a| a.ip().to_string())
            .unwrap_or_default();
        e
    }
}

impl SxState {
    pub fn load() -> Self {
        let cfg: SysCfg = sys::load("system.toml");
        let locked = cfg.lock;
        Self {
            cfg,
            locked,
            pin: String::new(),
            pin_new: None,
            pin_mode: false,
            fails: 0,
            wait_until: None,
            vol_shown: None,
            last_auto: None,
            last_zone_check: None,
            frames: Vec::new(),
            diag: Arc::new(Mutex::new(Vec::new())),
            diag_busy: Arc::new(Mutex::new(false)),
            log_tab: 0,
            logs: Vec::new(),
            cal_ym: (0, 0),
            cal_day: 0,
            unit: (0, 2, 3),
            msg: String::new(),
            confirm: 0,
            cleanup: Vec::new(),
            hist_tab: 0,
            snooze: None,
            rssi: None,
            last_rssi: None,
            fp: None,
            fp_mode: 0,
            fp_info: String::new(),
            fp_count: None,
            fp_retry: None,
            face: None,
            face_gen: 0,
            face_mode: 0,
            face_info: String::new(),
            face_tried: false,
            ssh: None,
        }
    }
}

#[allow(dead_code)] // not used by anything yet; kept, see docs/OFFEN-WARNUNGEN.md
const DAYS_SHORT: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
pub(super) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl App {
    fn sx_save(&mut self) {
        let _ = sys::save("system.toml", &self.sx.cfg);
    }

    /// Im Kalender gewählter Monat und Tag (für das Blatt „Neuer Termin")
    pub(super) fn sx_cal_ym(&self) -> (i64, i64) {
        self.sx.cal_ym
    }
    pub(super) fn sx_cal_day(&self) -> i64 {
        self.sx.cal_day
    }

    fn now_dt(&self) -> sys::DateTime {
        sys::from_unix(sys::unix_now(), self.set.tz_min)
    }

    /// Uhrzeit nach Einstellung (12/24 h).
    pub(super) fn clock_str(&self, h: i64, mi: i64) -> String {
        sysx::fmt_clock(h, mi, self.sx.cfg.h12)
    }

    /// Ruhemodus „Nicht stören" jetzt aktiv?
    pub(super) fn dnd_active(&self) -> bool {
        let c = &self.sx.cfg;
        if c.dnd {
            return true;
        }
        if c.dnd_sched {
            let d = self.now_dt();
            return sysx::in_window((d.h * 60 + d.mi) as u16, c.dnd_from, c.dnd_to);
        }
        false
    }

    pub(super) fn night_active(&self) -> bool {
        let c = &self.sx.cfg;
        if c.night {
            return true;
        }
        if c.night_sched {
            let d = self.now_dt();
            return sysx::in_window((d.h * 60 + d.mi) as u16, c.night_from, c.night_to);
        }
        false
    }

    /// Bildschirm bleibt beim Laden an?
    pub fn stay_awake(&self) -> bool {
        self.caffeine_on() || self.sx.cfg.stay_awake_charging && sys::battery().is_some_and(|b| b.1)
    }

    /// Bildschirm geht aus: bei eingeschalteter Sperre sperren.
    pub(super) fn sys_on_sleep(&mut self) {
        if self.sx.cfg.lock {
            self.sx.locked = true;
            self.sx.pin.clear();
        }
        if self.sx.fp_mode == 1 {
            self.fp_stop();
        }
        if self.sx.face_mode == 1 {
            self.face_stop();
        }
        self.sx.face_tried = false;
    }

    /// Helligkeit: Energiesparen begrenzt auf 50 %.
    pub(super) fn sys_bright(&self, v: f32) -> f32 {
        if self.sx.cfg.saver {
            v.min(0.5)
        } else {
            v
        }
    }

    /// Alarm-Regeln nach Tonprofil und „Nicht stören": (Ton, Vibration in ms, Kopf-Hinweis)
    pub(super) fn sys_alert(&self, high: bool, sound: bool, vib: u32) -> (bool, u32, bool) {
        let quiet = self.dnd_active() && !high;
        let p = self.sx.cfg.profile;
        (
            sound && p == 0 && !quiet,
            if p == 2 || quiet {
                0
            } else {
                vib.max(if p == 1 && !high { 200 } else { 0 })
            },
            !quiet,
        )
    }

    pub(super) fn sys_msg(&mut self, m: sysd::SysMsg) {
        match m {
            sysd::SysMsg::Saver(on) => {
                self.sx.cfg.saver = on;
                self.sx_save();
                self.apply_saver();
            }
            sysd::SysMsg::TimeSynced(d) => {
                self.toast(format!("{} ({d:+} s)", self.t("Time synchronized")))
            }
            sysd::SysMsg::Fp(ev) => self.fp_event(ev),
            sysd::SysMsg::Face(g, ev) => {
                if g == self.sx.face_gen {
                    self.face_event(ev)
                }
            }
        }
    }

    // ------------------------------------------------------------ Gesicht

    fn face_start(&mut self, mode: u8, args: &[&str]) -> bool {
        self.sx.face = None;
        self.sx.face_gen = self.sx.face_gen.wrapping_add(1);
        let (bg, g) = (self.bg.clone(), self.sx.face_gen);
        self.sx.face = crate::face::FaceRun::start(args, move |ev| {
            bg.send(sys::Msg::Sys(sysd::SysMsg::Face(g, ev)))
        });
        self.sx.face_mode = if self.sx.face.is_some() { mode } else { 0 };
        self.sx.face.is_some()
    }

    fn face_stop(&mut self) {
        self.sx.face = None;
        self.sx.face_mode = 0;
        self.sx.face_gen = self.sx.face_gen.wrapping_add(1);
    }

    fn face_event(&mut self, ev: crate::face::FaceEv) {
        use crate::face::FaceEv as E;
        let learn = self.sx.face_mode == 2;
        match ev {
            E::Camera if learn => self.sx.face_info = self.t("Look at the screen").into(),
            E::NoFace if learn => {
                self.sx.face_info = self
                    .t("No face seen – hold the phone in front of you")
                    .into()
            }
            E::Dark => self.sx.face_info = self.t("Too dark").into(),
            E::Closer => self.sx.face_info = self.t("Move closer").into(),
            E::Progress(n, m) => {
                self.sx.face_info = format!("{} {n}/{m}", self.t("Recording face"))
            }
            E::Restart => self.sx.face_info = self.t("Hold still – starting again").into(),
            E::Done => {
                self.face_stop();
                self.sx.face_info.clear();
                self.toast(self.t("Face set up").into());
            }
            E::Match if self.sx.face_mode == 1 => {
                self.face_stop();
                if self.sx.locked {
                    self.sx.locked = false;
                    self.sx.fails = 0;
                    self.sx.pin.clear();
                    self.sx.msg.clear();
                    if self.sx.fp_mode == 1 {
                        self.fp_stop();
                    }
                }
            }
            E::Timeout => {
                if self.sx.face_mode == 1 {
                    self.sx.msg = self.t("Face not recognized").into();
                } else if learn {
                    self.sx.face_info = self.t("Time is up").into();
                }
                self.face_stop();
            }
            E::Fail(s) => {
                if learn {
                    self.sx.face_info = format!("{}: {s}", self.t("Face error"));
                }
                self.face_stop();
            }
            E::Exit(_) => {
                self.sx.face = None;
                self.sx.face_mode = 0;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ Fingerabdruck

    /// Verbindung zum Fingerabdruck-Dienst öffnen und einen Befehl schicken.
    fn fp_start(&mut self, mode: u8, cmd: &str) -> bool {
        self.sx.fp = None;
        let bg = self.bg.clone();
        let Some(mut c) =
            crate::fp::FpConn::open(move |ev| bg.send(sys::Msg::Sys(sysd::SysMsg::Fp(ev))))
        else {
            self.sx.fp_mode = 0;
            self.sx.fp_retry = Some(Instant::now() + Duration::from_secs(5));
            return false;
        };
        let ok = c.send(cmd);
        self.sx.fp = Some(c);
        self.sx.fp_mode = if ok { mode } else { 0 };
        ok
    }

    fn fp_stop(&mut self) {
        self.sx.fp = None;
        self.sx.fp_mode = 0;
    }

    fn fp_event(&mut self, ev: crate::fp::FpEv) {
        use crate::fp::FpEv as E;
        let mode = self.sx.fp_mode;
        match ev {
            E::Match(_) if mode == 1 => {
                self.fp_stop();
                if self.sx.locked {
                    self.sx.locked = false;
                    self.sx.fails = 0;
                    self.sx.pin.clear();
                    self.sx.msg.clear();
                }
            }
            E::NoMatch if mode == 1 => {
                self.sx.msg = self.t("Fingerprint not recognized").into();
                sys::vibrate(80);
            }
            E::Started => self.sx.fp_info = self.t("Place your finger on the sensor").into(),
            E::Rest(n) if mode == 2 => {
                self.sx.fp_info = format!("{} – {n}", self.t("Lift and touch again"))
            }
            E::Acquired(k) if mode == 2 && k != 0 => {
                self.sx.fp_info = self.t("Not clear – touch again").into()
            }
            E::Learned(_) if mode == 2 => {
                self.sx.fp_info.clear();
                self.toast(self.t("Fingerprint added").into());
                self.fp_start(3, "count");
            }
            E::Count(n) => {
                self.sx.fp_count = Some(n);
                if mode == 3 {
                    self.fp_stop();
                }
            }
            E::Removed => {
                self.sx.fp_count = Some(0);
                self.sx.cfg.fp_unlock = false;
                self.sx_save();
                self.toast(self.t("Fingerprints deleted").into());
                self.fp_stop();
            }
            E::Error(5, _) => {}
            E::Error(code, _) => {
                if mode == 2 {
                    self.sx.fp_info = if code == 3 {
                        self.t("Time is up").into()
                    } else {
                        format!("{} ({code})", self.t("Fingerprint error"))
                    };
                } else if mode == 1 && code == 7 {
                    self.sx.msg = self.t("Too many attempts – use the PIN").into();
                }
                self.fp_stop();
                self.sx.fp_retry =
                    Some(Instant::now() + Duration::from_secs(if code == 7 { 30 } else { 3 }));
            }
            E::Fail(s) => {
                if mode == 2 {
                    self.sx.fp_info = format!("{}: {s}", self.t("Fingerprint error"));
                }
                self.fp_stop();
                self.sx.fp_retry = Some(Instant::now() + Duration::from_secs(5));
            }
            E::Closed => {
                if self.sx.fp_mode != 0 {
                    self.sx.fp = None;
                    self.sx.fp_mode = 0;
                    self.sx.fp_retry = Some(Instant::now() + Duration::from_secs(3));
                }
            }
            _ => {}
        }
    }

    /// Schnelleinstellungen: Nicht stören, Nachtlicht, Energiesparen
    pub(super) fn sx_dnd(&self) -> bool {
        self.sx.cfg.dnd
    }
    pub(super) fn sx_night(&self) -> bool {
        self.sx.cfg.night
    }
    pub(super) fn sx_saver(&self) -> bool {
        self.sx.cfg.saver
    }
    pub(super) fn sx_set_dnd(&mut self, on: bool) {
        self.sx.cfg.dnd = on;
        let _ = sys::save("system.toml", &self.sx.cfg);
    }
    pub(super) fn sx_set_night(&mut self, on: bool) {
        self.sx.cfg.night = on;
        let _ = sys::save("system.toml", &self.sx.cfg);
    }
    pub(super) fn sx_set_saver(&mut self, on: bool) {
        self.sx.cfg.saver = on;
        let _ = sys::save("system.toml", &self.sx.cfg);
        self.apply_saver();
    }

    pub(super) fn apply_saver(&mut self) {
        let (av, _) = sysx::governors();
        let prof = if self.sx.cfg.saver {
            "power".to_string()
        } else if self.sx.cfg.perf.is_empty() {
            "balanced".into()
        } else {
            self.sx.cfg.perf.clone()
        };
        if let Some(g) = sysx::governor_for(&prof, &av) {
            sysx::set_governor(&g);
        }
        self.apply_brightness();
    }

    /// Wecker schlummern lassen: in `min` Minuten erneut klingeln.
    pub(super) fn sys_snooze(&mut self, msg: String, min: u64) {
        self.sx.snooze = Some((Instant::now() + Duration::from_secs(min * 60), msg));
    }

    /// Rechnung im Verlauf ablegen (höchstens 30).
    pub(super) fn sys_calc_history(&mut self, expr: &str, res: &str) {
        let mut h: CalcHist = sys::load("calc-history.toml");
        h.list.insert(0, (expr.to_string(), res.to_string()));
        h.list.truncate(30);
        let _ = sys::save("calc-history.toml", &h);
    }

    fn s_calchist(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("History"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut h: CalcHist = sys::load("calc-history.toml");
        if h.list.is_empty() {
            self.caption(ui, &mut y, self.t("No calculations yet"));
        }
        let mut pick = None;
        let old = ui.rtl;
        ui.rtl = false;
        for (e, r) in &h.list {
            if ui.row(&mut y, None, &format!("= {r}"), Some(e), None, false) {
                pick = Some(r.clone());
            }
        }
        ui.rtl = old;
        if !h.list.is_empty()
            && ui.button(
                Rect::new(ui.dp(16.0), y + ui.dp(6.0), ui.w - ui.dp(32.0), ui.dp(46.0)),
                self.t("Clear history"),
                Btn::Plain,
            )
        {
            h.list.clear();
            let _ = sys::save("calc-history.toml", &h);
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
        if let Some(r) = pick {
            // Ergebnis übernehmen und zurück zum Rechner
            self.tl.expr = r.clone();
            self.tl.result = r;
            self.back();
            ui.dirty = true;
        }
    }

    /// Regelmäßig (aus tick): Zeitzone/Sommerzeit, Auto-Helligkeit. true = neu zeichnen.
    pub(super) fn sys_tick(&mut self) -> bool {
        let mut r = false;
        if self
            .sx
            .snooze
            .as_ref()
            .is_some_and(|s| Instant::now() >= s.0)
        {
            let (_, msg) = self.sx.snooze.take().unwrap();
            ultar_notify::post("system", "clock", &msg, "", ultar_notify::Urgency::High);
            self.tl.ringing = Some(msg);
            r = true;
        }
        if self
            .sx
            .last_zone_check
            .map_or(true, |t| t.elapsed() > Duration::from_secs(30))
        {
            self.sx.last_zone_check = Some(Instant::now());
            if !self.sx.cfg.zone.is_empty() {
                if let Some(off) = sysx::offset_at(&self.sx.cfg.zone, sys::unix_now()) {
                    if off != self.set.tz_min {
                        self.set.tz_min = off;
                        self.save();
                        r = true;
                    }
                }
            }
        }
        if self
            .sx
            .last_rssi
            .map_or(true, |t| t.elapsed() > Duration::from_secs(10))
        {
            self.sx.last_rssi = Some(Instant::now());
            let v = sys::wifi_iface()
                .and_then(|i| crate::wifi::signal(&i))
                .map(|s| s.0)
                .filter(|r| *r < 0);
            if v != self.sx.rssi {
                self.sx.rssi = v;
                r = true;
            }
        }
        if self.sx.cfg.auto_bright
            && self
                .sx
                .last_auto
                .map_or(true, |t| t.elapsed() > Duration::from_secs(3))
        {
            self.sx.last_auto = Some(Instant::now());
            if let Some(lux) = light_lux() {
                let target = sysx::lux_to_brightness(lux);
                if (target - self.set.brightness).abs() > 0.04 {
                    // sanft annähern
                    self.set.brightness += (target - self.set.brightness) * 0.5;
                    self.apply_brightness();
                }
            }
        }
        if self
            .sx
            .vol_shown
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1700))
        {
            r = true;
        } else if self.sx.vol_shown.is_some() {
            self.sx.vol_shown = None;
            r = true;
        }
        if self.sx.cfg.show_fps || *self.sx.diag_busy.lock().unwrap() {
            r = true;
        }
        if matches!(
            self.stack.last().map(|s| &s.0),
            Some(Screen::Sys(Sx::World | Sx::Battery))
        ) {
            r = true;
        }
        r
    }

    pub(super) fn sys_needs_tick(&self) -> bool {
        self.sx.vol_shown.is_some()
            || self.sx.cfg.show_fps
            || *self.sx.diag_busy.lock().unwrap()
            || self.sx.snooze.is_some()
            || self.sx.cfg.auto_bright
            || matches!(
                self.stack.last().map(|s| &s.0),
                Some(Screen::Sys(Sx::World | Sx::Battery))
            )
            || (self.sx.locked && self.sx.wait_until.is_some_and(|t| t > Instant::now()))
    }

    /// Liste für die Einstellungen
    pub(super) fn sys_settings_items(&self) -> Vec<(Ic, &'static str, String, Screen)> {
        let c = &self.sx.cfg;
        let on = |b: bool| -> String {
            if b {
                self.t("On").into()
            } else {
                String::new()
            }
        };
        vec![
            (
                Ic::Lock,
                "Screen lock",
                if !c.pin_hash.is_empty() {
                    "PIN".into()
                } else {
                    on(c.lock)
                },
                Screen::Sys(Sx::LockSet),
            ),
            (
                Ic::Clock,
                "Time zone & sync",
                sysx::zone(&c.zone)
                    .map(|z| z.1.to_string())
                    .unwrap_or_default(),
                Screen::Sys(Sx::Time),
            ),
            (
                Ic::Chip,
                "Battery & performance",
                on(c.saver),
                Screen::Sys(Sx::Battery),
            ),
            (
                Ic::Sun,
                "Night light & brightness",
                on(self.night_active()),
                Screen::Sys(Sx::DisplayX),
            ),
            (
                Ic::Speaker,
                "Sound profiles",
                self.t(["Normal", "Vibrate", "Silent"][c.profile.min(2) as usize])
                    .into(),
                Screen::Sys(Sx::SoundX),
            ),
            (
                Ic::List,
                "Notification history",
                String::new(),
                Screen::Sys(Sx::NotifHist),
            ),
            (
                Ic::UpDown,
                "Network tools",
                String::new(),
                Screen::Sys(Sx::NetTools),
            ),
            (
                Ic::Disk,
                "Backup & reset",
                String::new(),
                Screen::Sys(Sx::Backup),
            ),
            (
                Ic::Term,
                "System logs",
                String::new(),
                Screen::Sys(Sx::Logs),
            ),
            (
                Ic::Sliders,
                "Developer options",
                String::new(),
                Screen::Sys(Sx::Dev),
            ),
        ]
    }

    pub(super) fn sys_open(&mut self, s: &Sx) {
        match s {
            Sx::Calendar => {
                let d = self.now_dt();
                if self.sx.cal_ym.0 == 0 {
                    self.sx.cal_ym = (d.y, d.mo);
                    self.sx.cal_day = d.d;
                }
            }
            Sx::EventEdit(i) => {
                let cal: sysx::Calendar = sys::load("calendar.toml");
                let e = i
                    .and_then(|i| cal.events.get(i).cloned())
                    .unwrap_or_default();
                self.fields.insert("sx_ev_title", e.title);
                self.fields.insert(
                    "sx_ev_time",
                    if e.at >= 0 && i.is_some() {
                        format!("{:02}:{:02}", e.at / 60, e.at % 60)
                    } else {
                        String::new()
                    },
                );
                self.fields.insert(
                    "sx_ev_rem",
                    if i.is_some() {
                        e.remind.to_string()
                    } else {
                        "15".into()
                    },
                );
            }
            Sx::Logs => self.sx_load_logs(),
            Sx::NetTools => {
                self.fields.insert("sx_dns", self.sx.cfg.dns.clone());
                self.fields.insert(
                    "sx_lim",
                    if self.sx.cfg.data_limit_mb > 0 {
                        self.sx.cfg.data_limit_mb.to_string()
                    } else {
                        String::new()
                    },
                );
            }
            Sx::Time => {
                self.fields.insert("sx_ntp", self.sx.cfg.ntp_server.clone());
            }
            Sx::Backup => {
                self.sx.cleanup = sysx::cleanup_candidates();
                self.sx.confirm = 0;
            }
            Sx::PinSet => {
                self.sx.pin.clear();
                self.sx.pin_new = None;
            }
            Sx::LockSet => {
                self.sx.fp_info.clear();
                self.sx.face_info.clear();
                if crate::fp::available() && self.sx.fp_mode == 0 {
                    self.fp_start(3, "count");
                }
            }
            Sx::Search => {
                self.fields.insert("sx_q", String::new());
                self.focus = Some("sx_q");
            }
            Sx::HiddenWifi => {
                self.fields.insert("sx_ssid", String::new());
                self.fields.insert("sx_wpw", String::new());
            }
            _ => {}
        }
        self.sx.msg.clear();
    }

    pub(super) fn s_sys(&mut self, ui: &mut Ui, area: Rect, s: Sx, sc: &mut f32) {
        match s {
            Sx::LockSet => self.s_lockset(ui, area, sc),
            Sx::PinSet => self.s_pinset(ui, area),
            Sx::Time => self.s_tzone(ui, area, sc),
            Sx::Battery => self.s_battery(ui, area, sc),
            Sx::DisplayX => self.s_displayx(ui, area, sc),
            Sx::SoundX => self.s_soundx(ui, area, sc),
            Sx::NotifHist => self.s_hist(ui, area, sc),
            Sx::NetTools => self.s_nettools(ui, area, sc),
            Sx::HiddenWifi => self.s_hidden(ui, area),
            Sx::Logs => self.s_logs(ui, area, sc),
            Sx::Backup => self.s_backup(ui, area, sc),
            Sx::Dev => self.s_dev(ui, area, sc),
            Sx::Calendar => self.s_calendar(ui, area, sc),
            Sx::EventEdit(i) => self.s_event(ui, area, i),
            Sx::World => self.s_world(ui, area, sc),
            Sx::Units => self.s_units(ui, area, sc),
            Sx::Search => self.s_search(ui, area, sc),
            Sx::CalcHist => self.s_calchist(ui, area, sc),
        }
    }

    fn sx_msg_line(&mut self, ui: &mut Ui, y: &mut f32) {
        if !self.sx.msg.is_empty() {
            let m = self.sx.msg.clone();
            let c = ui.th.accent;
            *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), &m, 14.0, c) + ui.dp(12.0);
        }
    }

    // ------------------------------------------------------------ Sperre

    fn s_lockset(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Screen lock"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut lock = self.sx.cfg.lock;
        if ui.toggle_row(
            &mut y,
            self.t("Lock screen"),
            Some(self.t("Locks when the screen turns off")),
            &mut lock,
        ) {
            self.sx.cfg.lock = lock;
            if !lock {
                self.sx.cfg.pin_hash.clear();
                self.sx.cfg.pin_salt.clear();
            }
            self.sx_save();
        }
        ui.section(&mut y, "PIN");
        let has = !self.sx.cfg.pin_hash.is_empty();
        let cs = ui.th.sub;
        y += ui.para(
            ui.dp(22.0),
            y,
            ui.w - ui.dp(44.0),
            self.t(if has {
                "A PIN protects this device."
            } else {
                "Without a PIN, swiping up unlocks the device."
            }),
            14.0,
            cs,
        ) + ui.dp(12.0);
        let bw = ui.w - ui.dp(24.0);
        if ui.button(
            Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
            self.t(if has { "Change PIN" } else { "Set PIN" }),
            Btn::Primary,
        ) {
            self.go(Screen::Sys(Sx::PinSet));
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        if has
            && ui.button(
                Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                self.t("Remove PIN"),
                Btn::Plain,
            )
        {
            self.sx.cfg.pin_hash.clear();
            self.sx.cfg.pin_salt.clear();
            self.sx_save();
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        if ui.button(
            Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
            self.t("Lock now"),
            Btn::Plain,
        ) {
            self.sx.locked = true;
            self.sx.pin.clear();
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        if crate::fp::available() {
            ui.section(&mut y, self.t("Fingerprint"));
            let n = self.sx.fp_count.unwrap_or(0);
            let cnt = self
                .sx
                .fp_count
                .map(|n| n.to_string())
                .unwrap_or("…".into());
            ui.row(
                &mut y,
                None,
                self.t("Saved fingerprints"),
                None,
                Some(&cnt),
                false,
            );
            if has {
                let mut on = self.sx.cfg.fp_unlock && n > 0;
                let old = ui.enabled;
                ui.enabled = old && n > 0;
                if ui.toggle_row(&mut y, self.t("Unlock with fingerprint"), None, &mut on) {
                    self.sx.cfg.fp_unlock = on;
                    self.sx_save();
                }
                ui.enabled = old;
            } else {
                self.caption(ui, &mut y, self.t("Set a PIN first"));
            }
            if !self.sx.fp_info.is_empty() {
                let m = self.sx.fp_info.clone();
                let c = ui.th.accent;
                y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &m, 15.0, c) + ui.dp(10.0);
            }
            let old = ui.enabled;
            ui.enabled = old && has;
            if self.sx.fp_mode == 2 {
                if ui.button(
                    Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                    self.t("Cancel"),
                    Btn::Plain,
                ) {
                    self.fp_stop();
                    self.sx.fp_info.clear();
                    ui.dirty = true;
                }
            } else if ui.button(
                Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                self.t("Add fingerprint"),
                Btn::Primary,
            ) {
                self.sx.fp_info = self.t("Place your finger on the sensor").into();
                if !self.fp_start(2, "learn") {
                    self.sx.fp_info = self.t("Fingerprint service not ready").into();
                }
                ui.dirty = true;
            }
            y += ui.dp(60.0);
            if n > 0
                && self.sx.fp_mode != 2
                && ui.button(
                    Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                    self.t("Delete all fingerprints"),
                    Btn::Plain,
                )
            {
                self.fp_start(4, "remove 0");
                ui.dirty = true;
            }
            ui.enabled = old;
            y += ui.dp(60.0);
        }
        if crate::face::available() {
            ui.section(&mut y, self.t("Face unlock"));
            let enrolled = crate::face::enrolled();
            let cs = ui.th.sub;
            y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("Less secure than a PIN or fingerprint: someone who looks like you or a photo may unlock the device."), 13.5, cs) + ui.dp(10.0);
            let st = if enrolled {
                self.t("Set up")
            } else {
                self.t("Not set up")
            };
            ui.row(&mut y, None, self.t("Face"), None, Some(st), false);
            if has {
                let mut on = self.sx.cfg.face_unlock && enrolled;
                let old = ui.enabled;
                ui.enabled = old && enrolled;
                if ui.toggle_row(&mut y, self.t("Unlock with face"), None, &mut on) {
                    self.sx.cfg.face_unlock = on;
                    self.sx_save();
                }
                ui.enabled = old;
            } else {
                self.caption(ui, &mut y, self.t("Set a PIN first"));
            }
            if !self.sx.face_info.is_empty() {
                let m = self.sx.face_info.clone();
                let c = ui.th.accent;
                y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &m, 15.0, c) + ui.dp(10.0);
            }
            let old = ui.enabled;
            ui.enabled = old && has;
            if self.sx.face_mode == 2 {
                if ui.button(
                    Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                    self.t("Cancel"),
                    Btn::Plain,
                ) {
                    self.face_stop();
                    self.sx.face_info.clear();
                    ui.dirty = true;
                }
            } else if ui.button(
                Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                self.t(if enrolled {
                    "Set up face again"
                } else {
                    "Set up face"
                }),
                Btn::Primary,
            ) {
                self.sx.face_info = self.t("Starting camera…").into();
                if !self.face_start(2, &["anlernen", "45"]) {
                    self.sx.face_info = self.t("Face error").into();
                }
                ui.dirty = true;
            }
            y += ui.dp(60.0);
            if enrolled
                && self.sx.face_mode != 2
                && ui.button(
                    Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
                    self.t("Delete face data"),
                    Btn::Plain,
                )
            {
                crate::face::delete();
                self.sx.cfg.face_unlock = false;
                self.sx_save();
                self.toast(self.t("Face data deleted").into());
                ui.dirty = true;
            }
            ui.enabled = old;
            y += ui.dp(60.0);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// Ziffernblock; liefert eine gedrückte Taste ('0'..'9', '<' löschen, 'k' OK).
    fn keypad(&mut self, ui: &mut Ui, top: f32, light: bool) -> Option<char> {
        let keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '<', '0', 'k'];
        let kw = ui.dp(76.0).min((ui.w - ui.dp(48.0)) / 3.0);
        let x0 = (ui.w - 3.0 * kw) / 2.0;
        let mut out = None;
        let old = ui.enabled;
        ui.enabled = true;
        for (i, k) in keys.iter().enumerate() {
            let r = Rect::new(x0 + (i % 3) as f32 * kw, top + (i / 3) as f32 * kw, kw, kw)
                .inset(ui.dp(7.0));
            let held = ui.held(r);
            let fg = if light { rgb(0xFFFFFF) } else { ui.th.text };
            let bgc = if held { fg.alpha(0.28) } else { fg.alpha(0.10) };
            ui.cv.circle(r.cx(), r.cy(), r.w / 2.0, bgc);
            match k {
                '<' => ui.icon(Ic::Bksp, r.cx(), r.cy(), ui.dp(24.0), fg),
                'k' => ui.label(r, "OK", 20.0, fg, true, Al::Center),
                d => ui.label(r, &d.to_string(), 28.0, fg, false, Al::Center),
            }
            if ui.tap(r) {
                out = Some(*k);
                ui.dirty = true;
            }
        }
        ui.enabled = old;
        out
    }

    fn pin_dots(&self, ui: &mut Ui, y: f32, n: usize, c: crate::gfx::Color) {
        let n = n.max(1).min(8);
        let gap = ui.dp(22.0);
        let x0 = ui.w / 2.0 - gap * (n as f32 - 1.0) / 2.0;
        for i in 0..n {
            if i < self.sx.pin.len() {
                ui.cv.circle(x0 + i as f32 * gap, y, ui.dp(7.0), c);
            } else {
                ui.cv
                    .ring(x0 + i as f32 * gap, y, ui.dp(7.0), ui.dp(1.5), c);
            }
        }
    }

    fn s_pinset(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Set PIN"));
        let c = ui.th.text;
        let title = if self.sx.pin_new.is_none() {
            self.t("Enter new PIN (4–8 digits)")
        } else {
            self.t("Confirm PIN")
        };
        ui.label(
            Rect::new(0.0, area.y + ui.dp(24.0), ui.w, ui.dp(30.0)),
            title,
            17.0,
            c,
            true,
            Al::Center,
        );
        self.pin_dots(ui, area.y + ui.dp(80.0), self.sx.pin.len().max(4), c);
        if !self.sx.msg.is_empty() {
            let m = self.sx.msg.clone();
            let d = ui.th.danger;
            ui.label(
                Rect::new(0.0, area.y + ui.dp(102.0), ui.w, ui.dp(24.0)),
                &m,
                14.0,
                d,
                false,
                Al::Center,
            );
        }
        let top = area.y + ui.dp(140.0);
        match self.keypad(ui, top, false) {
            Some('<') => {
                self.sx.pin.pop();
            }
            Some('k') => {
                if !sysx::pin_valid(&self.sx.pin) {
                    self.sx.msg = self.t("4–8 digits").into();
                } else if let Some(first) = self.sx.pin_new.take() {
                    if first == self.sx.pin {
                        let salt = sysx::new_salt();
                        self.sx.cfg.pin_hash = sysx::pin_hash(&first, &salt);
                        self.sx.cfg.pin_salt = salt;
                        self.sx.cfg.lock = true;
                        self.sx_save();
                        self.sx.pin.clear();
                        self.toast(self.t("PIN saved").into());
                        self.back();
                    } else {
                        self.sx.msg = self.t("PINs do not match").into();
                    }
                    self.sx.pin.clear();
                } else {
                    self.sx.pin_new = Some(std::mem::take(&mut self.sx.pin));
                    self.sx.msg.clear();
                }
            }
            Some(d) if self.sx.pin.len() < 8 => self.sx.pin.push(d),
            _ => {}
        }
    }

    /// Sperrbildschirm über allem. Liefert true, solange gesperrt (dann nichts anderes zeichnen).
    pub(super) fn sys_lock_draw(&mut self, ui: &mut Ui) -> bool {
        if !self.sx.locked || self.call_active() {
            self.sx.pin_mode = false;
            return false;
        }
        let w = rgb(0xF2F0EA);
        // Hintergrund wie in der Demo: warmes Leuchten oben, darunter Ultar-Schwarz
        ui.cv.rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0x0A0B0E));
        ui.cv.vgrad(
            Rect::new(0.0, 0.0, ui.w, ui.h * 0.55),
            rgb(0x1C1A10),
            rgb(0x0A0B0E),
        );
        let dt = self.now_dt();
        let has_pin = !self.sx.cfg.pin_hash.is_empty();
        if let Some((cap, chg)) = sys::battery() {
            let s = if chg {
                format!("{} · {cap} %", self.t("Charging"))
            } else {
                format!("{cap} %")
            };
            ui.label(
                Rect::new(0.0, ui.dp(8.0), ui.w - ui.dp(16.0), ui.dp(20.0)),
                &s,
                13.0,
                w.alpha(0.8),
                false,
                Al::End,
            );
        }
        let old = ui.enabled;
        ui.enabled = true;
        let top = ui.dp(40.0);
        if !self.sx.pin_mode || !has_pin {
            // ---- Hauptansicht ----
            let mut y = top + ui.dp(40.0);
            let date = if self.set.lang == "ar" {
                format!(
                    "{}، {} {}",
                    self.t(crate::i18n::DAYS[dt.wd]),
                    dt.d,
                    self.t(MONTHS[(dt.mo as usize).clamp(1, 12) - 1])
                )
            } else {
                format!(
                    "{}, {}. {}",
                    self.t(crate::i18n::DAYS[dt.wd]),
                    dt.d,
                    self.t(MONTHS[(dt.mo as usize).clamp(1, 12) - 1])
                )
            };
            ui.label(
                Rect::new(0.0, y, ui.w, ui.dp(22.0)),
                &date,
                15.0,
                w.alpha(0.75),
                false,
                Al::Center,
            );
            y += ui.dp(26.0);
            let clock = self.clock_str(dt.h, dt.mi);
            let mut csz = 104.0;
            while csz > 40.0 && ui.text_w(&clock, csz, true) > ui.w - ui.dp(32.0) {
                csz -= 2.0;
            }
            let cw = ui.text_w(&clock, csz, true) + ui.dp(4.0);
            let cr = Rect::new((ui.w - cw) / 2.0, y, cw, ui.dp(112.0));
            ui.label(cr, &clock, csz, w, true, Al::Center);
            let (hh, top_c) = (ui.h, rgb(0x1C1A10));
            ui.cv.dot_screen(cr, ui.dp(5.0).max(3.0), |yy| {
                top_c.mix(rgb(0x0A0B0E), (yy / (hh * 0.55)).clamp(0.0, 1.0))
            });
            y += ui.dp(116.0);
            let hs = super::app_demo::hijri_str(&self.set.lang, dt.y, dt.mo, dt.d);
            ui.label(
                Rect::new(0.0, y, ui.w, ui.dp(20.0)),
                &hs,
                13.0,
                w.alpha(0.6),
                false,
                Al::Center,
            );
            y += ui.dp(30.0);
            // Pillen: nächster Wecker, VPN
            let mut pills: Vec<(Ic, String)> = Vec::new();
            if let Some(a) = self.tl.alarms.list.iter().find(|a| a.on) {
                pills.push((Ic::Clock, format!("{:02}:{:02}", a.h, a.m)));
            }
            if crate::sys::vpn_status().up {
                pills.push((Ic::Lock, "VPN".into()));
            }
            if !pills.is_empty() {
                let ws: Vec<f32> = pills
                    .iter()
                    .map(|p| ui.text_w(&p.1, 13.0, false) + ui.dp(44.0))
                    .collect();
                let tot: f32 = ws.iter().sum::<f32>() + ui.dp(8.0) * (pills.len() as f32 - 1.0);
                let mut x = (ui.w - tot) / 2.0;
                for (i, (ic, t)) in pills.iter().enumerate() {
                    let r = Rect::new(x, y, ws[i], ui.dp(32.0));
                    ui.cv.rrect(r, ui.dp(16.0), w.alpha(0.12));
                    ui.cv.rrect(r.inset(1.0), ui.dp(15.0), rgb(0x0A0B0E));
                    ui.icon(*ic, r.x + ui.dp(18.0), r.cy(), ui.dp(13.0), w);
                    ui.label(
                        Rect::new(r.x + ui.dp(30.0), r.y, r.w - ui.dp(36.0), r.h),
                        t,
                        13.0,
                        w,
                        false,
                        Al::Start,
                    );
                    x += ws[i] + ui.dp(8.0);
                }
                y += ui.dp(32.0);
            }
            y += ui.dp(26.0);
            // Benachrichtigungen: Absender sichtbar, Inhalt bis zum Entsperren ausgeblendet
            let mut notes: Vec<ultar_notify::Note> = Vec::new();
            notes.extend(sys::notes_all());
            notes.sort_by_key(|n| std::cmp::Reverse(n.ts));
            let now = sys::unix_now();
            let bottom_ui = ui.h - ui.dp(52.0) - ui.dp(190.0);
            for n in notes.iter().take(3) {
                let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(66.0));
                if r.bottom() > bottom_ui {
                    break;
                }
                ui.cv.rrect(r, ui.dp(20.0), rgb(0x1C1F26).alpha(0.78));
                let ix = if ui.rtl {
                    r.right() - ui.dp(30.0)
                } else {
                    r.x + ui.dp(30.0)
                };
                ui.cv.rrect(
                    Rect::new(
                        ix - ui.dp(16.0),
                        r.cy() - ui.dp(16.0),
                        ui.dp(32.0),
                        ui.dp(32.0),
                    ),
                    ui.dp(10.0),
                    rgb(0x2A2D35),
                );
                let nic = match n.kind.as_str() {
                    "call" => Ic::Phone,
                    "sms" | "message" => Ic::Chat,
                    "store" => Ic::Box,
                    _ => Ic::Bell,
                };
                ui.icon(nic, ix, r.cy(), ui.dp(16.0), ui.th.accent);
                let tx = Rect::new(
                    if ui.rtl {
                        r.x + ui.dp(14.0)
                    } else {
                        r.x + ui.dp(58.0)
                    },
                    r.y,
                    r.w - ui.dp(72.0),
                    r.h,
                );
                let raw = if n.app.is_empty() {
                    n.kind.clone()
                } else {
                    n.app.clone()
                };
                let cap: String = raw
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + &raw[c.len_utf8()..])
                    .unwrap_or_default();
                let app = self.t_dyn(&cap);
                ui.label(
                    Rect::new(tx.x, r.y + ui.dp(8.0), tx.w, ui.dp(16.0)),
                    &app,
                    12.0,
                    w.alpha(0.7),
                    false,
                    Al::Start,
                );
                if n.ts > 0 {
                    let mins = ((now - n.ts).max(0) / 60) as i64;
                    let (v, u) = if mins < 60 { (mins, 0) } else { (mins / 60, 1) };
                    let ago = match self.set.lang.as_str() {
                        "de" => format!("vor {v} {}", if u == 0 { "Min." } else { "Std." }),
                        "ar" => format!("منذ {v} {}", if u == 0 { "د" } else { "س" }),
                        _ => format!("{v} {} ago", if u == 0 { "min" } else { "h" }),
                    };
                    ui.label(
                        Rect::new(tx.x, r.y + ui.dp(8.0), tx.w, ui.dp(16.0)),
                        ago.trim(),
                        12.0,
                        w.alpha(0.7),
                        false,
                        Al::End,
                    );
                }
                let tf = ui.fit(&n.title, 14.0, true, tx.w);
                ui.label(
                    Rect::new(tx.x, r.y + ui.dp(25.0), tx.w, ui.dp(20.0)),
                    &tf,
                    14.0,
                    w,
                    true,
                    Al::Start,
                );
                ui.label(
                    Rect::new(tx.x, r.y + ui.dp(44.0), tx.w, ui.dp(16.0)),
                    self.t("Content hidden until unlocked"),
                    12.5,
                    w.alpha(0.7),
                    false,
                    Al::Start,
                );
                y += ui.dp(74.0);
            }
            // Unten: Taschenlampe, Fingerabdruck, Kamera
            let fp_on = self.sx.cfg.fp_unlock && crate::fp::available();
            if has_pin
                && fp_on
                && self.sx.fp_mode == 0
                && self.sx.fp_retry.map_or(true, |t| Instant::now() >= t)
            {
                self.fp_start(1, "auth");
            }
            let by = ui.h - ui.dp(52.0) - ui.dp(150.0);
            let side = ui.dp(26.0);
            let (lx, rx) = (ui.dp(24.0) + side, ui.w - ui.dp(24.0) - side);
            let torch = crate::quick::torch_on();
            if ui.icon_btn(
                lx,
                by,
                side,
                Ic::Torch,
                if torch { ui.th.accent } else { w.alpha(0.08) },
                if torch { ui.th.on_accent } else { w },
            ) {
                crate::quick::set_torch(!torch);
                ui.dirty = true;
            }
            let cam = ui.icon_btn(rx, by, side, Ic::Camera, w.alpha(0.08), w);
            let fr = ui.dp(34.0);
            let gold = rgb(0xD8C870);
            ui.cv
                .circle(ui.w / 2.0, by, fr + ui.dp(10.0), gold.alpha(0.06));
            ui.cv.ring(ui.w / 2.0, by, fr, ui.dp(1.5), gold.alpha(0.6));
            ui.icon(Ic::Finger, ui.w / 2.0, by, ui.dp(26.0), rgb(0xF0E8A0));
            let fp_tap = ui.tap(Rect::new(ui.w / 2.0 - fr, by - fr, 2.0 * fr, 2.0 * fr));
            let hint = if self.sx.fp_mode == 1 {
                self.t("Touch sensor or tap for PIN")
            } else {
                self.t("Tap for PIN")
            };
            ui.label(
                Rect::new(0.0, by + fr + ui.dp(10.0), ui.w, ui.dp(18.0)),
                if has_pin {
                    hint
                } else {
                    self.t("Tap to unlock")
                },
                12.5,
                w.alpha(0.7),
                false,
                Al::Center,
            );
            let sy = by + fr + ui.dp(40.0);
            let (ax, ay, aw) = (ui.w / 2.0, sy, ui.dp(7.0));
            ui.cv.line(
                ax - aw,
                ay + aw * 0.5,
                ax,
                ay - aw * 0.5,
                ui.dp(2.0),
                w.alpha(0.85),
            );
            ui.cv.line(
                ax,
                ay - aw * 0.5,
                ax + aw,
                ay + aw * 0.5,
                ui.dp(2.0),
                w.alpha(0.85),
            );
            ui.label(
                Rect::new(0.0, sy + ui.dp(10.0), ui.w, ui.dp(20.0)),
                self.t("Swipe up"),
                13.0,
                w.alpha(0.85),
                false,
                Al::Center,
            );
            let swiped = ui.p.released && ui.p.moved && ui.p.y - ui.p.py < -ui.dp(60.0);
            let up_tap = ui.tap(Rect::new(
                ui.w / 2.0 - ui.dp(80.0),
                sy - ui.dp(14.0),
                ui.dp(160.0),
                ui.dp(48.0),
            ));
            if swiped || fp_tap || up_tap || cam {
                if has_pin {
                    self.sx.pin_mode = true;
                    self.sx.pin.clear();
                } else {
                    self.sx.locked = false;
                    if cam {
                        self.go(Screen::Camera);
                    }
                }
                ui.dirty = true;
            }
        } else {
            // ---- PIN-Ansicht ----
            ui.icon(Ic::Lock, ui.w / 2.0, top + ui.dp(52.0), ui.dp(26.0), w);
            ui.label(
                Rect::new(0.0, top + ui.dp(74.0), ui.w, ui.dp(24.0)),
                self.t("Enter PIN"),
                17.0,
                w,
                false,
                Al::Center,
            );
            let top = top + ui.dp(30.0) - ui.h * 0.08;
            let wait = self
                .sx
                .wait_until
                .map(|t| t.saturating_duration_since(Instant::now()).as_secs())
                .unwrap_or(0);
            let dots_y = top + ui.h * 0.08 + ui.dp(110.0);
            // Fingerabdruck: Prüfen läuft, solange der Sperrbildschirm sichtbar ist
            let fp_on = self.sx.cfg.fp_unlock && crate::fp::available();
            if fp_on
                && self.sx.fp_mode == 0
                && self.sx.fp_retry.map_or(true, |t| Instant::now() >= t)
            {
                self.fp_start(1, "auth");
            }
            // Gesicht: einmal je Aufwachen automatisch, danach per Tippen erneut
            let face_on =
                self.sx.cfg.face_unlock && crate::face::available() && crate::face::enrolled();
            if face_on && wait == 0 && !self.sx.face_tried && self.sx.face_mode == 0 {
                self.sx.face_tried = true;
                self.face_start(1, &["pruefen", "8"]);
            }
            if wait > 0 {
                let s = format!("{} {wait} s", self.t("Try again in"));
                ui.label(
                    Rect::new(0.0, dots_y - ui.dp(12.0), ui.w, ui.dp(24.0)),
                    &s,
                    15.0,
                    ui.th.danger,
                    true,
                    Al::Center,
                );
            } else {
                self.pin_dots(ui, dots_y, self.sx.pin.len().max(4), w);
                if !self.sx.msg.is_empty() {
                    let m = self.sx.msg.clone();
                    ui.label(
                        Rect::new(0.0, dots_y + ui.dp(16.0), ui.w, ui.dp(22.0)),
                        &m,
                        13.5,
                        rgb(0xF87171),
                        false,
                        Al::Center,
                    );
                } else if self.sx.face_mode == 1 {
                    let m = self.t("Looking for your face…");
                    ui.label(
                        Rect::new(0.0, dots_y + ui.dp(16.0), ui.w, ui.dp(22.0)),
                        m,
                        13.5,
                        w.alpha(0.65),
                        false,
                        Al::Center,
                    );
                } else if self.sx.fp_mode == 1 {
                    let m = self.t("Touch the fingerprint sensor");
                    ui.label(
                        Rect::new(0.0, dots_y + ui.dp(16.0), ui.w, ui.dp(22.0)),
                        m,
                        13.5,
                        w.alpha(0.65),
                        false,
                        Al::Center,
                    );
                }
                if face_on && self.sx.face_tried && self.sx.face_mode == 0 {
                    // kleines Gesichtssymbol oben: erneut versuchen
                    let r = Rect::new(
                        ui.w / 2.0 - ui.dp(70.0),
                        dots_y - ui.dp(58.0),
                        ui.dp(140.0),
                        ui.dp(34.0),
                    );
                    ui.label(
                        r,
                        self.t("Try face again"),
                        13.5,
                        ui.th.accent,
                        true,
                        Al::Center,
                    );
                    if ui.tap(r) {
                        self.sx.msg.clear();
                        self.face_start(1, &["pruefen", "8"]);
                        ui.dirty = true;
                    }
                }
            }
            let kw = ui.dp(76.0).min((ui.w - ui.dp(48.0)) / 3.0);
            let pad_top = (ui.h - ui.dp(40.0) - 4.0 * kw).max(dots_y + ui.dp(48.0));
            let key = self.keypad(ui, pad_top, true);
            if wait == 0 {
                match key {
                    Some('<') => {
                        self.sx.pin.pop();
                    }
                    Some('k') => self.try_unlock(),
                    Some(d) if self.sx.pin.len() < 8 => {
                        self.sx.pin.push(d);
                        // Bei passender Länge sofort prüfen (PIN-Länge ergibt sich aus dem Versuch, nicht gespeichert)
                        if self.sx.pin.len() >= 4
                            && sysx::pin_hash(&self.sx.pin, &self.sx.cfg.pin_salt)
                                == self.sx.cfg.pin_hash
                        {
                            self.try_unlock();
                        }
                    }
                    _ => {}
                }
            }
            if wait > 0 {
                ui.dirty = false;
            }
            let cr = Rect::new(
                ui.w / 2.0 - ui.dp(70.0),
                ui.h - ui.dp(52.0) - ui.dp(44.0),
                ui.dp(140.0),
                ui.dp(40.0),
            );
            ui.label(cr, self.t("Cancel"), 14.0, w.alpha(0.7), false, Al::Center);
            if ui.tap(cr) {
                self.sx.pin_mode = false;
                self.sx.pin.clear();
                ui.dirty = true;
            }
        }
        ui.enabled = old;
        self.sys_overlay(ui);
        true
    }

    fn try_unlock(&mut self) {
        if sysx::pin_hash(&self.sx.pin, &self.sx.cfg.pin_salt) == self.sx.cfg.pin_hash {
            self.sx.locked = false;
            self.sx.fails = 0;
            self.sx.msg.clear();
            if self.sx.fp_mode == 1 {
                self.fp_stop();
            }
            if self.sx.face_mode == 1 {
                self.face_stop();
            }
        } else {
            self.sx.fails += 1;
            let w = sysx::lockout_secs(self.sx.fails);
            if w > 0 {
                self.sx.wait_until = Some(Instant::now() + Duration::from_secs(w));
            }
            self.sx.msg = self.t("Wrong PIN").into();
            sys::vibrate(120);
        }
        self.sx.pin.clear();
    }

    // ------------------------------------------------------------ Zeit

    fn s_tzone(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Time zone & sync"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut at = self.sx.cfg.auto_time;
        if ui.toggle_row(
            &mut y,
            self.t("Automatic time"),
            Some(self.t("Synchronize with a time server")),
            &mut at,
        ) {
            self.sx.cfg.auto_time = at;
            self.sx_save();
        }
        let mut h12 = self.sx.cfg.h12;
        if ui.toggle_row(&mut y, self.t("12-hour format"), None, &mut h12) {
            self.sx.cfg.h12 = h12;
            self.sx_save();
        }
        let st: sysd::State = sys::load(sysd::STATE);
        let last = if st.last_sync == 0 {
            self.t("never").to_string()
        } else {
            let d = sys::from_unix(st.last_sync, self.set.tz_min);
            format!(
                "{:02}.{:02}. {}{}",
                d.d,
                d.mo,
                self.clock_str(d.h, d.mi),
                if st.last_sync_ok {
                    String::new()
                } else {
                    format!(" – {}", st.last_sync_msg)
                }
            )
        };
        ui.row(
            &mut y,
            Some(Ic::Clock),
            self.t("Last sync"),
            None,
            Some(&last),
            false,
        );
        self.field_row(ui, &mut y, "sx_ntp", "Time server", "pool.ntp.org");
        let busy = *self.sx.diag_busy.lock().unwrap();
        let old = ui.enabled;
        ui.enabled = old && !busy;
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t(if busy {
                "Synchronizing…"
            } else {
                "Synchronize now"
            }),
            Btn::Primary,
        ) {
            let srv = self.f("sx_ntp").trim().to_string();
            self.sx.cfg.ntp_server = if srv.is_empty() {
                "pool.ntp.org".into()
            } else {
                srv
            };
            self.sx_save();
            let (out, b, srv) = (
                self.sx.diag.clone(),
                self.sx.diag_busy.clone(),
                self.sx.cfg.ntp_server.clone(),
            );
            *b.lock().unwrap() = true;
            std::thread::spawn(move || {
                let r = sysd::sync_time(&srv);
                *out.lock().unwrap() = vec![match r {
                    Ok(d) => format!("OK ({d:+} s)"),
                    Err(e) => format!("✗ {e}"),
                }];
                *b.lock().unwrap() = false;
            });
            ui.dirty = true;
        }
        ui.enabled = old;
        y += ui.dp(58.0);
        if let Some(l) = self.sx.diag.lock().unwrap().first().cloned() {
            let c = ui.th.sub;
            y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &l, 14.0, c) + ui.dp(8.0);
        }
        ui.section(&mut y, self.t("Time zone"));
        let now = sys::unix_now();
        let mut pick = None;
        let manual = self.sx.cfg.zone.is_empty();
        if ui.row(
            &mut y,
            None,
            self.t("Manual (fixed offset)"),
            None,
            if manual {
                Some(self.t("Selected"))
            } else {
                None
            },
            false,
        ) {
            self.sx.cfg.zone.clear();
            self.sx_save();
        }
        for (id, name, _, rule) in sysx::ZONES {
            let off = sysx::offset_at(id, now).unwrap_or(0);
            let sub = format!(
                "{}{}",
                sysx::fmt_offset(off),
                if *rule != sysx::Dst::None {
                    format!(" · {}", self.t("summer time"))
                } else {
                    String::new()
                }
            );
            let sel = self.sx.cfg.zone == *id;
            if ui.row(
                &mut y,
                None,
                name,
                Some(&sub),
                if sel { Some(self.t("Selected")) } else { None },
                false,
            ) {
                pick = Some((id.to_string(), off));
            }
        }
        if let Some((id, off)) = pick {
            self.sx.cfg.zone = id;
            self.sx_save();
            self.set.tz_min = off;
            self.save();
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Energie

    fn s_battery(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Battery & performance"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if let Some(b) = sys::battery_info() {
            let st = format!(
                "{} % · {}",
                b.percent,
                if b.charging {
                    self.t("Charging")
                } else {
                    self.t("On battery")
                }
            );
            ui.row(
                &mut y,
                Some(Ic::Chip),
                self.t("Battery"),
                Some(&st),
                None,
                false,
            );
            let temp = b.temp_c.map(|t| format!("{t:.1} °C")).unwrap_or("–".into());
            ui.row(
                &mut y,
                None,
                self.t("Temperature"),
                None,
                Some(&temp),
                false,
            );
            let health = if b.health.is_empty() {
                "–".to_string()
            } else {
                b.health.clone()
            };
            ui.row(&mut y, None, self.t("Health"), None, Some(&health), false);
            if let Some(v) = b.volt {
                ui.row(
                    &mut y,
                    None,
                    self.t("Voltage"),
                    None,
                    Some(&format!("{v:.2} V")),
                    false,
                );
            }
        } else {
            self.caption(ui, &mut y, self.t("No battery found"));
        }
        if let Some((z, c)) = sysx::hottest_zone() {
            ui.row(
                &mut y,
                None,
                self.t("Hottest component"),
                Some(&z),
                Some(&format!("{c:.0} °C")),
                false,
            );
        }
        ui.section(&mut y, self.t("Battery saver"));
        let mut s = self.sx.cfg.saver;
        if ui.toggle_row(
            &mut y,
            self.t("Battery saver"),
            Some(self.t("Limits brightness and CPU speed")),
            &mut s,
        ) {
            self.sx.cfg.saver = s;
            self.sx_save();
            self.apply_saver();
        }
        let mut a = self.sx.cfg.auto_saver;
        if ui.toggle_row(&mut y, self.t("Turn on automatically"), None, &mut a) {
            self.sx.cfg.auto_saver = a;
            self.sx_save();
        }
        let d = ui.stepper(
            &mut y,
            self.t("At battery level"),
            &format!("{} %", self.sx.cfg.saver_at),
        );
        if d != 0 {
            self.sx.cfg.saver_at = (self.sx.cfg.saver_at as i32 + d * 5).clamp(5, 50) as u8;
            self.sx_save();
        }
        let mut aw = self.sx.cfg.stay_awake_charging;
        if ui.toggle_row(&mut y, self.t("Stay awake while charging"), None, &mut aw) {
            self.sx.cfg.stay_awake_charging = aw;
            self.sx_save();
        }
        ui.section(&mut y, self.t("Performance"));
        let (av, cur) = sysx::governors();
        let opts = [
            self.t("Power saving"),
            self.t("Balanced"),
            self.t("Performance"),
        ];
        let keys = ["power", "balanced", "performance"];
        let sel = keys
            .iter()
            .position(|k| *k == self.sx.cfg.perf)
            .unwrap_or(1);
        if let Some(i) = ui.chips(&mut y, &opts, sel) {
            self.sx.cfg.perf = keys[i].into();
            self.sx_save();
            self.apply_saver();
        }
        let info = if av.is_empty() {
            self.t("CPU governor not available").to_string()
        } else {
            format!("CPU: {cur} ({})", av.join(", "))
        };
        let c = ui.th.sub;
        y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &info, 13.0, c) + ui.dp(16.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Anzeige

    fn s_displayx(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Night light & brightness"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut n = self.sx.cfg.night;
        if ui.toggle_row(
            &mut y,
            self.t("Night light"),
            Some(self.t("Warmer colors, less blue light")),
            &mut n,
        ) {
            self.sx.cfg.night = n;
            self.sx_save();
        }
        self.caption(ui, &mut y, self.t("Strength"));
        let mut v = self.sx.cfg.night_strength;
        if ui.slider(
            "sx_night",
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(44.0)),
            &mut v,
        ) {
            self.sx.cfg.night_strength = v;
            self.sx_save();
        }
        y += ui.dp(54.0);
        let mut s = self.sx.cfg.night_sched;
        let sub = format!(
            "{} – {}",
            self.clock_str(
                (self.sx.cfg.night_from / 60) as i64,
                (self.sx.cfg.night_from % 60) as i64
            ),
            self.clock_str(
                (self.sx.cfg.night_to / 60) as i64,
                (self.sx.cfg.night_to % 60) as i64
            )
        );
        if ui.toggle_row(&mut y, self.t("Schedule"), Some(&sub), &mut s) {
            self.sx.cfg.night_sched = s;
            self.sx_save();
        }
        if s {
            let d = ui.stepper(
                &mut y,
                self.t("From"),
                &format!("{:02}:00", self.sx.cfg.night_from / 60),
            );
            if d != 0 {
                self.sx.cfg.night_from =
                    ((self.sx.cfg.night_from as i32 / 60 + d).rem_euclid(24) * 60) as u16;
                self.sx_save();
            }
            let d = ui.stepper(
                &mut y,
                self.t("Until"),
                &format!("{:02}:00", self.sx.cfg.night_to / 60),
            );
            if d != 0 {
                self.sx.cfg.night_to =
                    ((self.sx.cfg.night_to as i32 / 60 + d).rem_euclid(24) * 60) as u16;
                self.sx_save();
            }
        }
        ui.section(&mut y, self.t("Brightness"));
        let has_light = light_lux().is_some();
        let lux = light_lux()
            .map(|l| format!("{l:.0} lx"))
            .unwrap_or_else(|| self.t("no light sensor").into());
        let mut ab = self.sx.cfg.auto_bright;
        if ui.toggle_row(&mut y, self.t("Adaptive brightness"), Some(&lux), &mut ab) {
            self.sx.cfg.auto_bright = ab && has_light;
            self.sx_save();
        }
        let mut hc = self.sx.cfg.contrast;
        if ui.toggle_row(
            &mut y,
            self.t("High contrast"),
            Some(self.t("Stronger text and lines")),
            &mut hc,
        ) {
            self.sx.cfg.contrast = hc;
            self.sx_save();
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Ton

    fn s_soundx(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Sound profiles"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let opts = [self.t("Normal"), self.t("Vibrate"), self.t("Silent")];
        if let Some(i) = ui.chips(&mut y, &opts, self.sx.cfg.profile.min(2) as usize) {
            self.sx.cfg.profile = i as u8;
            self.sx_save();
            if i == 1 {
                sys::vibrate(200);
            }
        }
        let mut h = self.sx.cfg.haptic;
        if ui.toggle_row(&mut y, self.t("Vibrate on touch"), None, &mut h) {
            self.sx.cfg.haptic = h;
            self.sx_save();
        }
        ui.section(&mut y, self.t("Do not disturb"));
        let c = ui.th.sub;
        y += ui.para(
            ui.dp(22.0),
            y,
            ui.w - ui.dp(44.0),
            self.t("Silences notifications. Calls, alarms and reminders still come through."),
            13.5,
            c,
        ) + ui.dp(10.0);
        let mut d = self.sx.cfg.dnd;
        if ui.toggle_row(&mut y, self.t("Do not disturb"), None, &mut d) {
            self.sx.cfg.dnd = d;
            self.sx_save();
        }
        let mut s = self.sx.cfg.dnd_sched;
        let sub = format!(
            "{:02}:00 – {:02}:00",
            self.sx.cfg.dnd_from / 60,
            self.sx.cfg.dnd_to / 60
        );
        if ui.toggle_row(&mut y, self.t("Schedule"), Some(&sub), &mut s) {
            self.sx.cfg.dnd_sched = s;
            self.sx_save();
        }
        if s {
            let dd = ui.stepper(
                &mut y,
                self.t("From"),
                &format!("{:02}:00", self.sx.cfg.dnd_from / 60),
            );
            if dd != 0 {
                self.sx.cfg.dnd_from =
                    ((self.sx.cfg.dnd_from as i32 / 60 + dd).rem_euclid(24) * 60) as u16;
                self.sx_save();
            }
            let dd = ui.stepper(
                &mut y,
                self.t("Until"),
                &format!("{:02}:00", self.sx.cfg.dnd_to / 60),
            );
            if dd != 0 {
                self.sx.cfg.dnd_to =
                    ((self.sx.cfg.dnd_to as i32 / 60 + dd).rem_euclid(24) * 60) as u16;
                self.sx_save();
            }
        }
        let active = self.dnd_active();
        ui.row(
            &mut y,
            None,
            self.t("Status"),
            None,
            Some(self.t(if active { "Active" } else { "Off" })),
            false,
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Benachrichtigungen

    fn s_hist(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Notification history"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let bw = (ui.w - ui.dp(32.0)) / 2.0;
        if ui.button(
            Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
            self.t("Clear current"),
            Btn::Plain,
        ) {
            sysd::clear_notifications();
            self.toast(self.t("Cleared").into());
            ui.dirty = true;
        }
        if ui.button(
            Rect::new(ui.dp(20.0) + bw, y, bw, ui.dp(46.0)),
            self.t("Clear history"),
            Btn::Danger,
        ) {
            sysd::clear_history();
            ui.dirty = true;
        }
        y += ui.dp(58.0);
        let h = sysd::history();
        if h.is_empty() {
            self.caption(ui, &mut y, self.t("No notifications"));
        }
        for n in h.iter().take(120) {
            let d = sys::from_unix(n.ts, self.set.tz_min);
            let when = format!("{:02}.{:02}. {}", d.d, d.mo, self.clock_str(d.h, d.mi));
            let sub = if n.body.is_empty() {
                n.app.clone()
            } else {
                format!("{} · {}", n.app, n.body)
            };
            ui.row(&mut y, None, &n.title, Some(&sub), Some(&when), false);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Netzwerk

    fn s_nettools(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Network tools"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        // WLAN-Details
        ui.section(&mut y, self.t("Wi-Fi details"));
        if let Some(ifc) = sys::wifi_iface() {
            let st = crate::wifi::status(&ifc).unwrap_or_default();
            let g = |k: &str| {
                crate::wifi::get(&st, k)
                    .map(crate::wifi::unescape)
                    .unwrap_or_else(|| "–".into())
            };
            let sig = crate::wifi::signal(&ifc)
                .map(|(r, f)| format!("{r} dBm · {f} MHz"))
                .unwrap_or("–".into());
            let net = sys::net_status()
                .into_iter()
                .find(|n| n.name == ifc)
                .unwrap_or_default();
            for (k, v) in [
                ("Network", g("ssid")),
                ("Signal", sig),
                ("IP address", g("ip_address")),
                (
                    "Gateway",
                    if net.gw.is_empty() {
                        "–".into()
                    } else {
                        net.gw.clone()
                    },
                ),
                (
                    "DNS",
                    if net.dns.is_empty() {
                        "–".into()
                    } else {
                        net.dns.clone()
                    },
                ),
                ("MAC", g("address")),
                ("Security", g("key_mgmt")),
                ("BSSID", g("bssid")),
            ] {
                ui.row(&mut y, None, self.t(k), None, Some(&v), false);
            }
        } else {
            self.caption(ui, &mut y, self.t("No Wi-Fi device found"));
        }
        if ui.row(
            &mut y,
            Some(Ic::Plus),
            self.t("Add hidden network"),
            None,
            None,
            true,
        ) {
            self.go(Screen::Sys(Sx::HiddenWifi));
            ui.dirty = true;
        }
        // Diagnose
        ui.section(&mut y, self.t("Diagnostics"));
        let busy = *self.sx.diag_busy.lock().unwrap();
        let old = ui.enabled;
        ui.enabled = old && !busy;
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t(if busy {
                "Testing…"
            } else {
                "Test connection"
            }),
            Btn::Primary,
        ) {
            self.run_diag();
            ui.dirty = true;
        }
        ui.enabled = old;
        y += ui.dp(58.0);
        for l in self.sx.diag.lock().unwrap().clone() {
            let ok = l.starts_with('✓');
            let c = if ok {
                ui.th.ok
            } else if l.starts_with('✗') {
                ui.th.danger
            } else {
                ui.th.sub
            };
            y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), &l, 14.0, c) + ui.dp(6.0);
        }
        // DNS
        ui.section(&mut y, "DNS");
        self.field_row(ui, &mut y, "sx_dns", "Own DNS servers", "1.1.1.1, 9.9.9.9");
        let bw = (ui.w - ui.dp(32.0)) / 2.0;
        if ui.button(
            Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
            self.t("Save"),
            Btn::Primary,
        ) {
            let v: Vec<String> = self
                .f("sx_dns")
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            if v.iter().all(|s| s.parse::<std::net::Ipv4Addr>().is_ok()) {
                self.sx.cfg.dns = v.join(",");
                self.sx_save();
                apply_dns(&self.sx.cfg.dns);
                self.toast(self.t("Saved").into());
            } else {
                self.toast(self.t("Invalid input").into());
            }
        }
        if ui.button(
            Rect::new(ui.dp(20.0) + bw, y, bw, ui.dp(46.0)),
            self.t("Automatic"),
            Btn::Plain,
        ) {
            self.sx.cfg.dns.clear();
            self.fields.insert("sx_dns", String::new());
            self.sx_save();
            apply_dns("");
        }
        y += ui.dp(58.0);
        // Datenlimit
        ui.section(&mut y, self.t("Data limit"));
        let usage: Usage = sys::load("net-usage.toml");
        let d = self.now_dt();
        let used = sysx::month_usage(&usage.days, d.y, d.mo);
        ui.row(
            &mut y,
            None,
            self.t("This month"),
            None,
            Some(&sys::human(used)),
            false,
        );
        self.field_row(
            ui,
            &mut y,
            "sx_lim",
            "Monthly limit (MB, empty = off)",
            "5000",
        );
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Save"),
            Btn::Plain,
        ) {
            let t = self.f("sx_lim").trim().to_string();
            match t.parse::<u64>() {
                Ok(v) => self.sx.cfg.data_limit_mb = v,
                Err(_) if t.is_empty() => self.sx.cfg.data_limit_mb = 0,
                Err(_) => self.toast(self.t("Invalid input").into()),
            }
            self.sx_save();
        }
        y += ui.dp(58.0);
        if self.sx.cfg.data_limit_mb > 0 {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(10.0));
            ui.progress(r, used as f32 / (self.sx.cfg.data_limit_mb as f32 * 1e6));
            y += ui.dp(24.0);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn run_diag(&mut self) {
        let (out, busy) = (self.sx.diag.clone(), self.sx.diag_busy.clone());
        *busy.lock().unwrap() = true;
        out.lock().unwrap().clear();
        let gw = sys::net_status()
            .into_iter()
            .find(|n| n.state == "verbunden")
            .map(|n| n.gw)
            .unwrap_or_default();
        let lang = self.set.lang.clone();
        std::thread::spawn(move || {
            let push = |s: String| out.lock().unwrap().push(s);
            let l = |de: &str, en: &str, ar: &str| -> String {
                match lang.as_str() {
                    "de" => de,
                    "en" => en,
                    _ => ar,
                }
                .to_string()
            };
            match gw.parse::<std::net::Ipv4Addr>() {
                Ok(ip) => match sysx::ping(ip, 1500) {
                    Ok(ms) => push(format!(
                        "✓ {} {gw}: {ms:.0} ms",
                        l("Router", "Router", "الموجّه")
                    )),
                    Err(e) => push(format!("✗ {} {gw}: {e}", l("Router", "Router", "الموجّه"))),
                },
                Err(_) => push(format!(
                    "✗ {}",
                    l(
                        "Keine Verbindung (kein Gateway)",
                        "Not connected (no gateway)",
                        "غير متصل (لا يوجد موجّه)"
                    )
                )),
            }
            match sysx::ping("1.1.1.1".parse().unwrap(), 2000) {
                Ok(ms) => push(format!("✓ Internet (1.1.1.1): {ms:.0} ms")),
                Err(e) => push(format!("✗ Internet (1.1.1.1): {e}")),
            }
            match sysx::dns_test("code-craft.tech") {
                Ok((ms, ip)) => push(format!("✓ DNS code-craft.tech → {ip} ({ms:.0} ms)")),
                Err(e) => push(format!("✗ DNS: {e}")),
            }
            match sysx::tcp_test("1.1.1.1:443") {
                Ok(ms) => push(format!("✓ HTTPS (TCP 443): {ms:.0} ms")),
                Err(e) => push(format!("✗ HTTPS (TCP 443): {e}")),
            }
            *busy.lock().unwrap() = false;
        });
    }

    fn s_hidden(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Add hidden network"));
        let mut y = area.y;
        self.field_row(ui, &mut y, "sx_ssid", "Network name (SSID)", "");
        self.field_row(
            ui,
            &mut y,
            "sx_wpw",
            "Password",
            self.t("empty = open network"),
        );
        if ui.button(
            Rect::new(ui.dp(16.0), y + ui.dp(6.0), ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Connect"),
            Btn::Primary,
        ) {
            let ssid = self.f("sx_ssid").trim().to_string();
            let pw = self.f("sx_wpw");
            if ssid.is_empty() || ssid.len() > 32 {
                self.toast(self.t("Invalid input").into());
            } else if let Err(e) = crate::wifi::check_pw(&pw) {
                self.toast(format!("{}: {e}", self.t("Invalid input")));
            } else {
                self.wifi_connecting = Some(ssid.clone());
                sys::wifi_connect(&self.bg, ssid, Some(pw));
                self.focus = None;
                self.back();
            }
            ui.dirty = true;
        }
    }

    // ------------------------------------------------------------ Protokolle

    fn sx_load_logs(&mut self) {
        self.sx.logs = if self.sx.log_tab == 0 {
            sysx::kernel_log(400)
        } else {
            sysx::service_log(400)
        };
        if self.sx.logs.is_empty() {
            self.sx.logs.push("–".into());
        }
    }

    fn s_logs(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("System logs"));
        let mut y0 = area.y;
        if let Some(i) = ui.chips(
            &mut y0,
            &[self.t("Kernel"), self.t("Services")],
            self.sx.log_tab,
        ) {
            self.sx.log_tab = i;
            self.sx_load_logs();
            *sc = 0.0;
        }
        let bw = ui.w - ui.dp(24.0);
        if ui.button(
            Rect::new(ui.dp(12.0), y0, bw, ui.dp(40.0)),
            self.t("Refresh"),
            Btn::Plain,
        ) {
            self.sx_load_logs();
        }
        y0 += ui.dp(48.0);
        let list = Rect::new(0.0, y0, ui.w, area.bottom() - y0);
        ui.scroll_begin(list, sc);
        let mut y = list.y - *sc;
        let (c, old) = (ui.th.text, ui.rtl);
        ui.rtl = false;
        // neueste oben
        for l in self.sx.logs.iter().rev() {
            let h = ui.dp(17.0)
                * ((l.chars().count() as f32 * 6.6 * ui.k / (ui.w - ui.dp(24.0)))
                    .ceil()
                    .max(1.0));
            if y + h > list.y && y < list.bottom() {
                ui.para(ui.dp(12.0), y, ui.w - ui.dp(24.0), l, 11.5, c);
            }
            y += h + ui.dp(4.0);
        }
        ui.rtl = old;
        ui.scroll_end(list, y + *sc - list.y, sc);
    }

    // ------------------------------------------------------------ Sicherung

    fn s_backup(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Backup & reset"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        self.sx_msg_line(ui, &mut y);
        ui.section(&mut y, self.t("Backup"));
        let c = ui.th.sub;
        y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("Saves settings, notes, contacts, calendar and alarms to /data/Backups. Passwords are not included."), 13.5, c) + ui.dp(10.0);
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Create backup"),
            Btn::Primary,
        ) {
            let d = self.now_dt();
            let stamp = format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                d.y, d.mo, d.d, d.h, d.mi, d.s
            );
            self.sx.msg = match sysx::backup_create(&stamp) {
                Ok((p, n)) => format!("✓ {n} {} → {p}", self.t("files")),
                Err(e) => format!("✗ {e}"),
            };
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        let mut restore = None;
        for b in sysx::backup_list().iter().take(8) {
            if ui.row(
                &mut y,
                Some(Ic::Disk),
                b,
                Some(self.t("Tap to restore")),
                None,
                true,
            ) {
                restore = Some(b.clone());
            }
        }
        if let Some(b) = restore {
            self.sx.msg = match sysx::backup_restore(&b) {
                Ok(n) => {
                    self.set = sys::load("settings.toml");
                    self.sx.cfg = sys::load("system.toml");
                    format!("✓ {n} {}", self.t("files restored"))
                }
                Err(e) => format!("✗ {e}"),
            };
            ui.dirty = true;
        }
        ui.section(&mut y, self.t("Free up storage"));
        let total: u64 = self.sx.cleanup.iter().map(|c| c.1).sum();
        for (p, s) in self.sx.cleanup.clone() {
            ui.row(&mut y, None, &p, None, Some(&sys::human(s)), false);
        }
        let label = format!("{} ({})", self.t("Clean up"), sys::human(total));
        let old = ui.enabled;
        ui.enabled = old && total > 0;
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            &label,
            Btn::Plain,
        ) {
            let freed = sysx::cleanup_run(&self.sx.cleanup);
            self.sx.msg = format!("✓ {} {}", sys::human(freed), self.t("freed"));
            self.sx.cleanup = sysx::cleanup_candidates();
        }
        ui.enabled = old;
        y += ui.dp(60.0);
        ui.section(&mut y, self.t("Factory reset"));
        y += ui.para(
            ui.dp(22.0),
            y,
            ui.w - ui.dp(44.0),
            self.t(
                "Deletes all settings, accounts, Wi-Fi networks, notes and contacts. Photos stay.",
            ),
            13.5,
            c,
        ) + ui.dp(10.0);
        let label = match self.sx.confirm {
            0 => self.t("Reset device"),
            1 => self.t("Really reset? Tap again"),
            _ => self.t("Last chance – tap to delete everything"),
        };
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            label,
            Btn::Danger,
        ) {
            self.sx.confirm += 1;
            if self.sx.confirm >= 3 {
                sysx::factory_reset(true);
                // Oberfläche neu starten -> Ersteinrichtung
                std::process::exit(0);
            }
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Entwickler

    fn s_dev(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Developer options"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let mut t = self.sx.cfg.show_touches;
        if ui.toggle_row(&mut y, self.t("Show touches"), None, &mut t) {
            self.sx.cfg.show_touches = t;
            self.sx_save();
        }
        let mut f = self.sx.cfg.show_fps;
        if ui.toggle_row(&mut y, self.t("Show frame rate"), None, &mut f) {
            self.sx.cfg.show_fps = f;
            self.sx_save();
        }
        let mut aw = self.sx.cfg.stay_awake_charging;
        if ui.toggle_row(&mut y, self.t("Stay awake while charging"), None, &mut aw) {
            self.sx.cfg.stay_awake_charging = aw;
            self.sx_save();
        }
        ui.row(
            &mut y,
            None,
            self.t("Render time"),
            None,
            Some(&format!("{:.1} ms (Ø {:.1})", self.perf.1, self.perf.2)),
            false,
        );
        self.s_dev_ssh(ui, &mut y);
        ui.section(&mut y, self.t("Restart into"));
        let c = ui.th.sub;
        y += ui.para(
            ui.dp(22.0),
            y,
            ui.w - ui.dp(44.0),
            self.t("Download mode is needed to flash a new system image with Ultar Flash."),
            13.5,
            c,
        ) + ui.dp(10.0);
        let bw = (ui.w - ui.dp(32.0)) / 2.0;
        let label_dl = if self.sx.confirm == 1 {
            self.t("Tap again")
        } else {
            self.t("Download mode")
        };
        if ui.button(
            Rect::new(ui.dp(12.0), y, bw, ui.dp(46.0)),
            label_dl,
            Btn::Danger,
        ) {
            if self.sx.confirm == 1 {
                if let Err(e) = sysx::reboot_to("download") {
                    self.toast(e);
                }
            }
            self.sx.confirm = 1;
            ui.dirty = true;
        }
        let label_rc = if self.sx.confirm == 2 {
            self.t("Tap again")
        } else {
            self.t("Recovery")
        };
        if ui.button(
            Rect::new(ui.dp(20.0) + bw, y, bw, ui.dp(46.0)),
            label_rc,
            Btn::Plain,
        ) {
            if self.sx.confirm == 2 {
                if let Err(e) = sysx::reboot_to("recovery") {
                    self.toast(e);
                }
            }
            self.sx.confirm = 2;
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// Entwickler-Zugang (SSH): PC steuert das Gerät während der Entwicklung (Befehle, Kernprotokoll, Dateien,
    /// Bildschirmfoto, Tippen). Nur Schlüssel; einschalten, Adresse/Fingerabdruck zeigen, Schlüssel des PCs eintragen.
    fn s_dev_ssh(&mut self, ui: &mut Ui, y: &mut f32) {
        if self
            .sx
            .ssh
            .as_ref()
            .map_or(true, |(t, _)| t.elapsed() > Duration::from_secs(4))
        {
            self.sx.ssh = Some((Instant::now(), EntwicklerSsh::lesen()));
        }
        let st = self
            .sx
            .ssh
            .as_ref()
            .map(|x| x.1.clone())
            .unwrap_or_default();
        if !st.vorhanden {
            return;
        }
        ui.section(y, self.t("Developer access (SSH)"));
        let c = ui.th.sub;
        *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), self.t("Lets your PC control this device during development: commands, kernel log, files, screenshots and taps. Only with a key from your PC, never with a password."), 13.5, c) + ui.dp(10.0);
        let mut an = st.an;
        if ui.toggle_row(y, self.t("SSH access"), None, &mut an) {
            let _ = std::process::Command::new("ultar-sshd")
                .arg(if an { "an" } else { "aus" })
                .output();
            let _ = sys::ctl(if an {
                "restart entwickler"
            } else {
                "stop entwickler"
            });
            self.sx.ssh = None;
            self.toast(if an {
                self.t("SSH access on").to_string()
            } else {
                self.t("SSH access off").to_string()
            });
            ui.dirty = true;
        }
        let adr = if st.adresse.is_empty() {
            "—".to_string()
        } else {
            format!("{}:{}", st.adresse, st.port)
        };
        ui.row(y, None, self.t("Address"), None, Some(&adr), false);
        ui.row(
            y,
            None,
            self.t("Device key"),
            Some(&st.fingerabdruck),
            None,
            false,
        );
        ui.row(
            y,
            None,
            self.t("Allowed keys"),
            None,
            Some(&st.schluessel),
            false,
        );
        let m = ui.dp(16.0);
        let r = Rect::new(m, *y + ui.dp(4.0), ui.w - 2.0 * m, ui.dp(48.0));
        ui.field(
            r,
            "ssh_key",
            &self.f("ssh_key"),
            self.t("Paste the public key of your PC (ssh-ed25519 …)"),
        );
        *y += ui.dp(58.0);
        if ui.button(
            Rect::new(m, *y, ui.w - 2.0 * m, ui.dp(44.0)),
            self.t("Allow key"),
            Btn::Plain,
        ) {
            let k = self.f("ssh_key");
            match std::process::Command::new("ultar-sshd")
                .arg("schluessel")
                .arg(k.trim())
                .output()
            {
                Ok(o) if o.status.success() => {
                    self.fields.insert("ssh_key", String::new());
                    self.toast(self.t("Key allowed").to_string());
                }
                _ => self.toast(self.t("Not a valid public key").to_string()),
            }
            self.sx.ssh = None;
            ui.dirty = true;
        }
        *y += ui.dp(54.0);
    }

    // ------------------------------------------------------------ Kalender

    /// Kalender wie in der Web-Demo: Monatskopf mit Hijri-Monat, Raster mit Hijri-Tagen, Termine des Tages
    fn s_calendar(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Calendar"));
        if std::mem::take(&mut self.hdr_tap) {
            self.fields.insert("evt", String::new());
            self.fields.insert("evh", "09:00".into());
            self.sheet = Some(super::app_demo::Sheet::NewEvent);
            self.focus = Some("evt");
            self.focus_reset = true;
            ui.dirty = true;
        }
        let (cy, cm) = self.sx.cal_ym;
        let today = self.now_dt();
        let cal: sysx::Calendar = sys::load("calendar.toml");
        let m = ui.dp(16.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        // Monatskopf: Pfeile außen, Monat + Hijri-Monat in der Mitte
        let title = format!("{} {cy}", self.t(MONTHS[(cm - 1) as usize]));
        ui.label(
            Rect::new(0.0, y + ui.dp(2.0), ui.w, ui.dp(24.0)),
            &title,
            17.0,
            c,
            true,
            Al::Center,
        );
        let hs = super::app_demo::hijri_str(&self.set.lang, cy, cm, 1);
        ui.label(
            Rect::new(0.0, y + ui.dp(26.0), ui.w, ui.dp(18.0)),
            &hs,
            12.0,
            cs,
            false,
            Al::Center,
        );
        let (lx, rx) = (m + ui.dp(22.0), ui.w - m - ui.dp(22.0));
        let (px, nx) = if ui.rtl { (rx, lx) } else { (lx, rx) };
        if ui.icon_btn(
            px,
            y + ui.dp(22.0),
            ui.dp(18.0),
            Ic::Back,
            ui.th.card.alpha(0.0),
            c,
        ) {
            self.sx.cal_ym = if cm == 1 { (cy - 1, 12) } else { (cy, cm - 1) };
            ui.dirty = true;
        }
        if ui.icon_btn(
            nx,
            y + ui.dp(22.0),
            ui.dp(18.0),
            Ic::Chevron,
            ui.th.card.alpha(0.0),
            c,
        ) {
            self.sx.cal_ym = if cm == 12 { (cy + 1, 1) } else { (cy, cm + 1) };
            ui.dirty = true;
        }
        y += ui.dp(52.0);
        let gap = ui.dp(4.0);
        let cw = (ui.w - 2.0 * m - 6.0 * gap) / 7.0;
        let narrow: [&str; 7] = match self.set.lang.as_str() {
            "de" => ["M", "D", "M", "D", "F", "S", "S"],
            "ar" => ["ن", "ث", "ر", "خ", "ج", "س", "ح"],
            _ => ["M", "T", "W", "T", "F", "S", "S"],
        };
        for (i, d) in narrow.iter().enumerate() {
            let col = if ui.rtl { 6 - i } else { i };
            ui.label(
                Rect::new(m + col as f32 * (cw + gap), y, cw, ui.dp(22.0)),
                d,
                11.0,
                cs,
                false,
                Al::Center,
            );
        }
        y += ui.dp(26.0);
        let (first, days) = sysx::month_grid(cy, cm);
        let ch = cw.min(ui.dp(56.0));
        let rows = (first + days as usize).div_ceil(7);
        for d in 1..=days {
            let i = first + d as usize - 1;
            let col = if ui.rtl { 6 - i % 7 } else { i % 7 };
            let r = Rect::new(
                m + col as f32 * (cw + gap),
                y + (i / 7) as f32 * (ch + gap),
                cw,
                ch,
            );
            let is_today = d == today.d && cm == today.mo && cy == today.y;
            let sel = d == self.sx.cal_day && !is_today;
            if is_today {
                ui.cv.rrect(r, ui.dp(12.0), ui.th.accent);
            } else if sel {
                ui.cv.rrect(r, ui.dp(12.0), ui.th.accent);
                ui.cv.rrect(r.inset(ui.dp(1.5)), ui.dp(10.5), ui.th.bg);
            }
            let tc = if is_today { ui.th.on_accent } else { c };
            let sc2 = if is_today { ui.th.on_accent } else { cs };
            let has_ev = cal
                .events
                .iter()
                .any(|e| e.y == cy && e.mo == cm && e.d == d);
            let dy = if has_ev { -ui.dp(4.0) } else { 0.0 };
            ui.label(
                Rect::new(r.x, r.cy() - ui.dp(13.0) + dy, r.w, ui.dp(18.0)),
                &d.to_string(),
                14.0,
                tc,
                false,
                Al::Center,
            );
            let (_, _, hd) = crate::sysy::hijri(cy, cm, d);
            ui.label(
                Rect::new(r.x, r.cy() + ui.dp(4.0) + dy, r.w, ui.dp(12.0)),
                &hd.to_string(),
                8.5,
                sc2,
                false,
                Al::Center,
            );
            if has_ev {
                ui.cv
                    .circle(r.cx(), r.cy() + ui.dp(17.0), ui.dp(2.0), rgb(0xF80000));
            }
            if ui.tap(r) {
                self.sx.cal_day = d;
                ui.dirty = true;
            }
        }
        y += rows as f32 * (ch + gap) + ui.dp(12.0);
        // gewählter Tag als Abschnitt
        let day = self.sx.cal_day.clamp(1, days);
        let wd = (first + day as usize - 1) % 7; // 0 = Montag
        let wname = self.t(crate::i18n::DAYS[(wd + 1) % 7]);
        let sec = if self.set.lang == "ar" {
            format!("{wname}، {day} {}", self.t(MONTHS[(cm - 1) as usize]))
        } else {
            format!("{wname}, {day}. {}", self.t(MONTHS[(cm - 1) as usize]))
        };
        ui.section(&mut y, &sec);
        let mut evs: Vec<(usize, sysx::Event)> = cal
            .events
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, e)| e.y == cy && e.mo == cm && e.d == day)
            .collect();
        evs.sort_by_key(|e| e.1.at);
        ui.kb.row_end.set(-1.0);
        if evs.is_empty() {
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(96.0));
            ui.card(r);
            ui.label(r, self.t("No events"), 14.0, cs, false, Al::Center);
            y += r.h;
        }
        let mut open = None;
        for (i, e) in evs {
            let when = if e.at < 0 {
                self.t("All day").to_string()
            } else {
                format!("{:02}:{:02}", e.at / 60, e.at % 60)
            };
            let sub = if e.remind > 0 {
                format!(
                    "{when} · {} {} {}",
                    self.t("Reminder"),
                    e.remind,
                    self.t("min before – the helper calls")
                )
            } else {
                when
            };
            if ui.row(&mut y, Some(Ic::Clock), &e.title, Some(&sub), None, false) {
                open = Some(i);
            }
        }
        y += ui.dp(20.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
        if let Some(i) = open {
            self.go(Screen::Sys(Sx::EventEdit(Some(i))));
            ui.dirty = true;
        }
    }

    fn s_event(&mut self, ui: &mut Ui, area: Rect, idx: Option<usize>) {
        let area = self.header(
            ui,
            area,
            self.t(if idx.is_some() {
                "Edit event"
            } else {
                "New event"
            }),
        );
        let mut y = area.y;
        let (cy, cm) = self.sx.cal_ym;
        let day = format!("{:02}.{:02}.{cy}", self.sx.cal_day, cm);
        ui.row(
            &mut y,
            Some(Ic::Clock),
            self.t("Date"),
            None,
            Some(&day),
            false,
        );
        self.field_row(ui, &mut y, "sx_ev_title", "Title", "");
        self.field_row(
            ui,
            &mut y,
            "sx_ev_time",
            "Time (HH:MM, empty = all day)",
            "09:30",
        );
        self.field_row(
            ui,
            &mut y,
            "sx_ev_rem",
            "Reminder (minutes before, 0 = none)",
            "15",
        );
        let bw = (ui.w - ui.dp(32.0)) / 2.0;
        if ui.button(
            Rect::new(
                ui.dp(12.0),
                y,
                if idx.is_some() {
                    bw
                } else {
                    ui.w - ui.dp(24.0)
                },
                ui.dp(46.0),
            ),
            self.t("Save"),
            Btn::Primary,
        ) {
            let title = self.f("sx_ev_title").trim().to_string();
            let tm = self.f("sx_ev_time").trim().to_string();
            let at = if tm.is_empty() {
                Some(-1)
            } else {
                tm.split_once(':')
                    .and_then(|(h, m)| {
                        Some((h.trim().parse::<i32>().ok()?, m.trim().parse::<i32>().ok()?))
                    })
                    .filter(|(h, m)| (0..24).contains(h) && (0..60).contains(m))
                    .map(|(h, m)| h * 60 + m)
            };
            let rem = self
                .f("sx_ev_rem")
                .trim()
                .parse::<i32>()
                .unwrap_or(0)
                .clamp(0, 10080);
            match at {
                Some(at) if !title.is_empty() => {
                    let mut cal: sysx::Calendar = sys::load("calendar.toml");
                    let e = sysx::Event {
                        y: cy,
                        mo: cm,
                        d: self.sx.cal_day,
                        at,
                        title,
                        remind: if at < 0 { 0 } else { rem },
                        notified: false,
                    };
                    match idx {
                        Some(i) if i < cal.events.len() => cal.events[i] = e,
                        _ => cal.events.push(e),
                    }
                    let _ = sys::save("calendar.toml", &cal);
                    self.focus = None;
                    self.back();
                }
                _ => self.toast(self.t("Invalid input").into()),
            }
            ui.dirty = true;
        }
        if let Some(i) = idx {
            if ui.button(
                Rect::new(ui.dp(20.0) + bw, y, bw, ui.dp(46.0)),
                self.t("Delete"),
                Btn::Danger,
            ) {
                let mut cal: sysx::Calendar = sys::load("calendar.toml");
                if i < cal.events.len() {
                    cal.events.remove(i);
                    let _ = sys::save("calendar.toml", &cal);
                }
                self.focus = None;
                self.back();
                ui.dirty = true;
            }
        }
    }

    // ------------------------------------------------------------ Weltuhr

    fn s_world(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("World clock"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let now = sys::unix_now();
        let mut remove = None;
        for (i, id) in self.sx.cfg.world.clone().iter().enumerate() {
            let Some(z) = sysx::zone(id) else { continue };
            let off = sysx::offset_at(id, now).unwrap_or(0);
            let d = sys::from_unix(now, off);
            let diff = (off - self.set.tz_min) as f32 / 60.0;
            let sub = format!(
                "{} · {}{} h",
                sysx::fmt_offset(off),
                if diff >= 0.0 { "+" } else { "" },
                diff
            );
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(78.0));
            ui.card(r);
            let (c, cs) = (ui.th.text, ui.th.sub);
            let inner = r.inset(ui.dp(16.0));
            ui.label(
                Rect::new(inner.x, inner.y, inner.w * 0.55, ui.dp(24.0)),
                z.1,
                17.0,
                c,
                true,
                Al::Start,
            );
            ui.label(
                Rect::new(inner.x, inner.y + ui.dp(24.0), inner.w * 0.6, ui.dp(20.0)),
                &sub,
                12.5,
                cs,
                false,
                Al::Start,
            );
            ui.label(
                Rect::new(inner.x, inner.y, inner.w - ui.dp(34.0), inner.h),
                &self.clock_str(d.h, d.mi),
                28.0,
                c,
                true,
                Al::End,
            );
            let x = Rect::new(r.right() - ui.dp(40.0), r.y, ui.dp(36.0), r.h);
            ui.icon(Ic::Close, x.cx(), x.cy(), ui.dp(14.0), cs);
            if ui.tap(x) {
                remove = Some(i);
            }
            y += r.h + ui.dp(10.0);
        }
        if let Some(i) = remove {
            self.sx.cfg.world.remove(i);
            self.sx_save();
            ui.dirty = true;
        }
        ui.section(&mut y, self.t("Add city"));
        let mut add = None;
        for (id, name, _, _) in sysx::ZONES
            .iter()
            .filter(|z| !self.sx.cfg.world.iter().any(|w| w == z.0))
        {
            let off = sysx::offset_at(id, now).unwrap_or(0);
            if ui.row(
                &mut y,
                Some(Ic::Plus),
                name,
                Some(&sysx::fmt_offset(off)),
                None,
                false,
            ) {
                add = Some(id.to_string());
            }
        }
        if let Some(id) = add {
            self.sx.cfg.world.push(id);
            self.sx_save();
            *sc = 0.0;
            ui.dirty = true;
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Einheiten

    pub(super) fn s_units(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Unit converter"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let cats: Vec<&str> = sysx::UNITS.iter().map(|c| self.t(c.0)).collect();
        let (cat, from, to) = self.sx.unit;
        // Kategorien in zwei Reihen
        for row in [&cats[..4], &cats[4..]] {
            let base = if row.len() == 4 { 0 } else { 4 };
            let sel = if cat >= base && cat < base + row.len() {
                cat - base
            } else {
                usize::MAX
            };
            if let Some(i) = ui.chips(&mut y, row, sel) {
                self.sx.unit = (base + i, 0, 1);
                ui.dirty = true;
            }
        }
        let units: Vec<&str> = sysx::UNITS[cat].1.iter().map(|u| u.0).collect();
        self.field_row(ui, &mut y, "sx_unit", "Value", "1");
        self.caption(ui, &mut y, self.t("From"));
        for chunk in 0..units.len().div_ceil(4) {
            let part = &units[chunk * 4..(chunk * 4 + 4).min(units.len())];
            let sel = if from / 4 == chunk {
                from % 4
            } else {
                usize::MAX
            };
            if let Some(i) = ui.chips(&mut y, part, sel) {
                self.sx.unit.1 = chunk * 4 + i;
            }
        }
        self.caption(ui, &mut y, self.t("To"));
        for chunk in 0..units.len().div_ceil(4) {
            let part = &units[chunk * 4..(chunk * 4 + 4).min(units.len())];
            let sel = if to / 4 == chunk { to % 4 } else { usize::MAX };
            if let Some(i) = ui.chips(&mut y, part, sel) {
                self.sx.unit.2 = chunk * 4 + i;
            }
        }
        let (cat, from, to) = self.sx.unit;
        let v = self
            .f("sx_unit")
            .replace(',', ".")
            .trim()
            .parse::<f64>()
            .unwrap_or(1.0);
        let res = sysx::convert(cat, from.min(units.len() - 1), to.min(units.len() - 1), v);
        let r = Rect::new(ui.dp(16.0), y + ui.dp(6.0), ui.w - ui.dp(32.0), ui.dp(84.0));
        ui.card(r);
        let s = format!(
            "{} {} = {} {}",
            sysx::fmt_num(v),
            units[from.min(units.len() - 1)],
            sysx::fmt_num(res),
            units[to.min(units.len() - 1)]
        );
        let (c, old) = (ui.th.text, ui.rtl);
        ui.rtl = false;
        let fs = if s.chars().count() > 26 { 18.0 } else { 24.0 };
        ui.label(r.inset(ui.dp(14.0)), &s, fs, c, true, Al::Center);
        ui.rtl = old;
        y += ui.dp(100.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    // ------------------------------------------------------------ Suche

    fn s_search(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Search"));
        let mut y0 = area.y;
        let r = Rect::new(ui.dp(16.0), y0, ui.w - ui.dp(32.0), ui.dp(48.0));
        ui.field(r, "sx_q", &self.f("sx_q"), self.t("Search …"));
        y0 += ui.dp(56.0);
        let q = self.f("sx_q");
        let list = Rect::new(0.0, y0, ui.w, area.bottom() - y0);
        ui.scroll_begin(list, sc);
        let mut y = list.y - *sc;
        // Treffer sammeln: (Wert, Symbol, Titel, Untertitel, Ziel)
        let mut hits: Vec<(u32, Ic, String, String, Option<Screen>)> = Vec::new();
        for (ic, key, target) in self.all_apps() {
            for name in [self.t(key), key] {
                if let Some(s) = sysx::score(&q, name) {
                    hits.push((
                        s + 50,
                        ic,
                        self.t(key).into(),
                        self.t("App").into(),
                        target.clone(),
                    ));
                    break;
                }
            }
        }
        let mut settings: Vec<(Ic, &'static str, Screen)> = vec![
            (Ic::Globe, "Network", Screen::Net),
            (Ic::Sun, "Display", Screen::Display),
            (Ic::Music, "Sound", Screen::Sound),
            (Ic::Lang, "Language", Screen::Lang),
            (Ic::Clock, "Date & time", Screen::Time),
            (Ic::Disk, "Storage", Screen::Storage),
            (Ic::Shield, "Privacy", Screen::Privacy),
            (Ic::Shield, "Security", Screen::Security),
            (Ic::Globe, "VPN", Screen::Vpn),
            (Ic::Info, "System", Screen::System),
            (Ic::Chip, "Hardware & drivers", Screen::Hardware),
            (Ic::Tower, "Mobile network", Screen::Radio),
        ];
        for (ic, k, _, s) in self.sys_settings_items() {
            settings.push((ic, k, s));
        }
        for (ic, k, s) in settings {
            for name in [self.t(k), k] {
                if let Some(sc_) = sysx::score(&q, name) {
                    hits.push((
                        sc_ + 40,
                        ic,
                        self.t(k).into(),
                        self.t("Settings").into(),
                        Some(s.clone()),
                    ));
                    break;
                }
            }
        }
        let contacts: super::app_extra::Contacts = sys::load("contacts.toml");
        for (i, c) in contacts.list.iter().enumerate() {
            if let Some(s) = sysx::score(&q, &c.name).or_else(|| {
                (!q.trim().is_empty() && c.number.replace(' ', "").contains(q.trim()))
                    .then_some(300)
            }) {
                hits.push((
                    s,
                    Ic::Person,
                    c.name.clone(),
                    c.number.clone(),
                    Some(Screen::ContactEdit(Some(i))),
                ));
            }
        }
        let notes: super::app_extra::Notes = sys::load("notes.toml");
        for (i, n) in notes.list.iter().enumerate() {
            let first = n.text.lines().next().unwrap_or("").to_string();
            if let Some(s) = sysx::score(&q, &first).or_else(|| {
                (q.trim().chars().count() >= 3
                    && n.text.to_lowercase().contains(&q.trim().to_lowercase()))
                .then_some(200)
            }) {
                hits.push((
                    s,
                    Ic::Note,
                    first,
                    self.t("Note").into(),
                    Some(Screen::NoteEdit(i)),
                ));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        if q.trim().is_empty() {
            // Vorschläge wie in der Web-Demo
            ui.section(&mut y, self.t("Suggestions"));
            let mut sugg: Vec<String> = vec![
                self.t("Wi-Fi").into(),
                "VPN".into(),
                self.t("Camera").into(),
                self.t("Battery").into(),
            ];
            if let Some(c) = contacts.list.first() {
                sugg.push(c.name.split_whitespace().next().unwrap_or("").to_string());
            }
            sugg.push(self.t("Prayer times").into());
            let m = ui.dp(16.0);
            let mut x = m;
            let gap = ui.dp(8.0);
            let mut line: Vec<(String, f32)> = Vec::new();
            let mut rows: Vec<Vec<(String, f32)>> = Vec::new();
            for sg in sugg {
                let w = ui.text_w(&sg, 13.0, false) + ui.dp(26.0);
                if x + w > ui.w - m && !line.is_empty() {
                    rows.push(std::mem::take(&mut line));
                    x = m;
                }
                line.push((sg, w));
                x += w + gap;
            }
            rows.push(line);
            for row in rows {
                let mut x = if ui.rtl { ui.w - m } else { m };
                for (sg, w) in row {
                    let px = if ui.rtl { x - w } else { x };
                    let (_, t) = ui.pill(px, y, &sg, false);
                    if t {
                        self.fields.insert("sx_q", sg.clone());
                        ui.dirty = true;
                    }
                    x += if ui.rtl { -(w + gap) } else { w + gap };
                }
                y += ui.dp(40.0);
            }
            let cs = ui.th.sub;
            y += ui.para(
                m,
                y + ui.dp(4.0),
                ui.w - 2.0 * m,
                self.t("Arabic-tolerant: \"احمد\" also finds \"أحمد\"."),
                12.5,
                cs,
            ) + ui.dp(8.0);
        } else if hits.is_empty() {
            let cs = ui.th.sub;
            ui.label(
                Rect::new(0.0, y + ui.dp(30.0), ui.w, ui.dp(22.0)),
                self.t("Nothing found"),
                14.0,
                cs,
                false,
                Al::Center,
            );
            y += ui.dp(80.0);
        }
        ui.kb.row_end.set(-1.0);
        let mut go = None;
        for (_, ic, title, sub, target) in hits.into_iter().take(40) {
            if ui.row(&mut y, Some(ic), &title, Some(&sub), None, true) {
                go = Some((title, target));
            }
        }
        ui.scroll_end(list, y + *sc - list.y, sc);
        if let Some((title, target)) = go {
            self.focus = None;
            match target {
                Some(s) => self.go(s),
                None => self.toast(title),
            }
            ui.dirty = true;
        }
    }

    // ------------------------------------------------------------ Überlagerungen

    /// Lautstärke-Anzeige, Berührungen, Bildrate, Nachtlicht/Kontrast. Am Ende von draw().
    pub(super) fn sys_overlay(&mut self, ui: &mut Ui) {
        ui.set_clip(None);
        if let Some(t) = self.sx.vol_shown {
            if t.elapsed() < Duration::from_millis(1600) {
                let w = (ui.w - ui.dp(64.0)).min(ui.dp(360.0));
                let r = Rect::new((ui.w - w) / 2.0, ui.dp(44.0), w, ui.dp(52.0));
                ui.cv.soft_shadow(r, ui.dp(26.0), rgb(0));
                ui.cv.rrect(r, ui.dp(26.0), ui.th.card);
                let muted = !self.set.sound || self.sx.cfg.profile > 0;
                let c = ui.th.text;
                let ix = if ui.rtl {
                    r.right() - ui.dp(28.0)
                } else {
                    r.x + ui.dp(28.0)
                };
                ui.icon(
                    Ic::Speaker,
                    ix,
                    r.cy(),
                    ui.dp(18.0),
                    if muted { ui.th.sub } else { c },
                );
                let bar = if ui.rtl {
                    Rect::new(
                        r.x + ui.dp(64.0),
                        r.cy() - ui.dp(4.0),
                        r.w - ui.dp(116.0),
                        ui.dp(8.0),
                    )
                } else {
                    Rect::new(
                        r.x + ui.dp(52.0),
                        r.cy() - ui.dp(4.0),
                        r.w - ui.dp(116.0),
                        ui.dp(8.0),
                    )
                };
                ui.progress(bar, self.set.volume);
                let pct = format!("{}", (self.set.volume * 100.0).round());
                let pr = if ui.rtl {
                    Rect::new(r.x + ui.dp(14.0), r.y, ui.dp(44.0), r.h)
                } else {
                    Rect::new(r.right() - ui.dp(58.0), r.y, ui.dp(44.0), r.h)
                };
                ui.label(pr, &pct, 14.0, c, true, Al::Center);
            }
        }
        if self.sx.cfg.show_touches && (ui.p.down || ui.p.released) {
            ui.cv
                .circle(ui.p.x, ui.p.y, ui.dp(22.0), rgb(0xFFFFFF).alpha(0.35));
            ui.cv.ring(
                ui.p.x,
                ui.p.y,
                ui.dp(22.0),
                ui.dp(2.0),
                rgb(0xFFFFFF).alpha(0.8),
            );
        }
        if self.sx.cfg.show_fps {
            let now = Instant::now();
            self.sx.frames.push(now);
            self.sx
                .frames
                .retain(|t| now.duration_since(*t) < Duration::from_secs(1));
            let s = format!("{} fps · {:.1} ms", self.sx.frames.len(), self.perf.1);
            let r = Rect::new(ui.dp(8.0), ui.h - ui.dp(84.0), ui.dp(130.0), ui.dp(24.0));
            ui.cv.rrect(r, ui.dp(6.0), rgb(0).alpha(0.65));
            let old = ui.rtl;
            ui.rtl = false;
            ui.label(r, &s, 11.5, rgb(0x7CFC9A), true, Al::Center);
            ui.rtl = old;
        }
        if self.night_active() {
            sysx::night_tint(&mut ui.cv.px, self.sx.cfg.night_strength.max(0.1));
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct CalcHist {
    list: Vec<(String, String)>,
}

/// Monitor-Statistik (Ausschnitt)
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Usage {
    days: std::collections::BTreeMap<String, (u64, u64)>,
}

/// Umgebungslicht in Lux (IIO oder Samsung-Lichtsensor).
pub fn light_lux() -> Option<f32> {
    if let Ok(rd) = std::fs::read_dir("/sys/bus/iio/devices") {
        for e in rd.flatten() {
            for f in ["in_illuminance_input", "in_illuminance_raw"] {
                if let Some(v) = std::fs::read_to_string(e.path().join(f))
                    .ok()
                    .and_then(|s| s.trim().parse::<f32>().ok())
                {
                    return Some(v);
                }
            }
        }
    }
    // Samsung: Sensor liefert erst nach „enable" Werte (input-Gerät „light_sensor")
    static ENABLED: std::sync::Once = std::sync::Once::new();
    ENABLED.call_once(|| {
        if let Ok(rd) = std::fs::read_dir("/sys/class/input") {
            for e in rd.flatten() {
                if std::fs::read_to_string(e.path().join("name"))
                    .map(|n| n.trim() == "light_sensor")
                    .unwrap_or(false)
                {
                    let _ = std::fs::write(e.path().join("enable"), "1");
                }
            }
        }
    });
    if let Ok(rd) = std::fs::read_dir("/sys/class/sensors") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.contains("light") {
                for f in ["lux", "raw_data", "light_lux"] {
                    let Ok(s) = std::fs::read_to_string(e.path().join(f)) else {
                        continue;
                    };
                    let v: Vec<f32> = s
                        .split([',', ' ', '\n'])
                        .filter_map(|x| x.trim().parse::<f32>().ok())
                        .collect();
                    match v.len() {
                        0 => continue,
                        // Farbsensor (R,G,B,W,Belichtung,Verstärkung): hellsten Kanal als Lux-Näherung
                        n if n >= 4 => return Some(v[..4].iter().cloned().fold(0.0, f32::max)),
                        _ => return Some(v[0]),
                    }
                }
            }
        }
    }
    None
}

/// Eigene DNS-Server dauerhaft hinterlegen (ultar-net beachtet /data/ultar/dns.override) und sofort setzen.
pub fn apply_dns(list: &str) {
    let p = format!("{}/dns.override", sys::data_dir());
    if list.trim().is_empty() {
        let _ = std::fs::remove_file(&p);
        return;
    }
    let _ = std::fs::write(&p, list);
    let conf: String = list
        .split(',')
        .map(|s| format!("nameserver {}\n", s.trim()))
        .collect();
    let _ = std::fs::write("/run/ultar/resolv.conf", &conf);
    let _ = std::fs::write("/etc/resolv.conf", &conf);
}
