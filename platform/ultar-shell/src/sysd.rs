//! Hintergrunddienst der Oberfläche (eigener Thread, läuft auch bei ausgeschaltetem Bildschirm):
//! Akku-Hinweise und Energiesparen, Hitzewarnung, Zeitabgleich (SNTP), Termin-Erinnerungen,
//! Datenlimit, Benachrichtigungsverlauf. Meldet Zustandswechsel per `Msg::Sys` an die Oberfläche.

use crate::sys::{self, Bg, Msg};
use crate::sysx::{self, BattEvent, SysCfg};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use ultar_notify::Urgency;

/// Zustand des Dienstes (eigene Datei, damit er nicht mit der Oberfläche um system.toml konkurriert).
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct State {
    pub last_sync: i64,
    pub last_sync_ok: bool,
    pub last_sync_msg: String,
    /// letzter (auch erfolgloser) Versuch – nach Fehlschlag in 5 min erneut
    pub last_try: i64,
    /// Monat (JJJJ-MM), für den die Datenlimit-Warnung 80 % / 100 % schon kam
    pub limit_warned: String,
    pub limit_level: u8,
    pub hist_seen: u64,
}

/// Ausschnitt der Monitor-Statistik (net-usage.toml): Tag -> (empfangen, gesendet)
#[derive(Deserialize, Default)]
#[serde(default)]
#[cfg_attr(test, allow(dead_code))] // only the running shell uses it, tests do not start it
struct Usage {
    days: std::collections::BTreeMap<String, (u64, u64)>,
}

pub const STATE: &str = "sysd-state.toml";
pub const HISTORY: &str = "notify-history.log";

#[derive(Clone, Debug)]
pub enum SysMsg {
    /// Energiesparen automatisch an/aus
    #[cfg_attr(test, allow(dead_code))]
    // only the running shell uses it, tests do not start it
    Saver(bool),
    /// Uhrzeit wurde gestellt (Abweichung in Sekunden)
    #[cfg_attr(test, allow(dead_code))]
    // only the running shell uses it, tests do not start it
    TimeSynced(i64),
    /// Meldung des Fingerabdruck-Dienstes
    Fp(crate::fp::FpEv),
    /// Meldung der Gesichtserkennung (Lauf-Nummer, Ereignis)
    Face(u32, crate::face::FaceEv),
}

#[cfg_attr(test, allow(dead_code))] // only the running shell uses it, tests do not start it
fn text(lang: &str, de: &str, en: &str, ar: &str) -> String {
    match lang {
        "de" => de,
        "en" => en,
        _ => ar,
    }
    .to_string()
}

/// Zeitabgleich jetzt ausführen (auch von der Oberfläche aus aufrufbar).
pub fn sync_time(server: &str) -> Result<i64, String> {
    // SNTP zuerst; ist UDP 123 gesperrt, über den Date-Kopf eines Webservers (TCP 80)
    let (ms, src) = match sysx::sntp_query(server) {
        Ok(ms) => (ms, server.to_string()),
        Err(e1) => match sysx::http_time() {
            Ok(ms) => (ms, "HTTP".to_string()),
            Err(e2) => return Err(format!("NTP: {e1}; HTTP: {e2}")),
        },
    };
    let before = sys::unix_now();
    if !sysx::set_clock(ms) {
        return Err("clock_settime".into());
    }
    let mut st: State = sys::load(STATE);
    st.last_sync = ms / 1000;
    st.last_sync_ok = true;
    st.last_sync_msg = src;
    let _ = sys::save(STATE, &st);
    Ok(ms / 1000 - before)
}

/// Neue Zeilen aus /run/ultar/notify.log an den Verlauf anhängen (höchstens 300 Zeilen behalten).
fn archive_notifications(st: &mut State) {
    let len = std::fs::metadata(ultar_notify::LOG)
        .map(|m| m.len())
        .unwrap_or(0);
    if len < st.hist_seen {
        st.hist_seen = 0;
    }
    if len == st.hist_seen {
        return;
    }
    let Ok(data) = std::fs::read(ultar_notify::LOG) else {
        return;
    };
    let new = String::from_utf8_lossy(&data[st.hist_seen as usize..]).to_string();
    let complete = &new[..new.rfind('\n').map(|i| i + 1).unwrap_or(0)];
    if complete.is_empty() {
        return;
    }
    st.hist_seen += complete.len() as u64;
    let path = format!("{}/{HISTORY}", sys::data_dir());
    let mut all = std::fs::read_to_string(&path).unwrap_or_default();
    all.push_str(complete);
    let lines: Vec<&str> = all.lines().collect();
    let keep = lines[lines.len().saturating_sub(300)..].join("\n") + "\n";
    let _ = std::fs::write(&path, keep);
}

/// Benachrichtigungsverlauf (neueste zuerst).
pub fn history() -> Vec<ultar_notify::Note> {
    let s = std::fs::read_to_string(format!("{}/{HISTORY}", sys::data_dir())).unwrap_or_default();
    s.lines()
        .rev()
        .filter_map(ultar_notify::parse_line)
        .collect()
}

/// Aktuelle Benachrichtigungen leeren (der Verlauf bleibt).
pub fn clear_notifications() {
    crate::sys::memo_clear();
    let mut st: State = sys::load(STATE);
    archive_notifications(&mut st);
    for p in [ultar_notify::LOG, "/data/ultar/notify.log"] {
        if std::path::Path::new(p).exists() {
            let _ = std::fs::write(p, "");
        }
    }
    st.hist_seen = 0;
    let _ = sys::save(STATE, &st);
}

pub fn clear_history() {
    let _ = std::fs::remove_file(format!("{}/{HISTORY}", sys::data_dir()));
}

#[cfg_attr(test, allow(dead_code))] // only the running shell uses it, tests do not start it
pub fn start(bg: Bg) {
    if std::env::var("ULTAR_DRY").is_ok() {
        return;
    }
    std::thread::spawn(move || run(bg));
}

#[cfg_attr(test, allow(dead_code))] // only the running shell uses it, tests do not start it
fn run(bg: Bg) {
    let mut prev_batt: Option<(u8, bool)> = None;
    let mut last_hot: Option<Instant> = None;
    let started = Instant::now();
    loop {
        let cfg: SysCfg = sys::load("system.toml");
        let set: sys::Settings = sys::load("settings.toml");
        let lang = set.lang.clone();
        let mut st: State = sys::load(STATE);
        let now = sys::unix_now();

        // Akku
        if let Some(b) = sys::battery_info() {
            let cur = (b.percent, b.charging);
            for ev in sysx::battery_events(prev_batt, cur, cfg.auto_saver, cfg.saver_at, cfg.saver)
            {
                match ev {
                    BattEvent::Low(p) => ultar_notify::post(
                        "system",
                        "battery",
                        &text(&lang, "Akku schwach", "Battery low", "البطارية منخفضة"),
                        &format!("{p} %"),
                        if p <= 5 {
                            Urgency::High
                        } else {
                            Urgency::Normal
                        },
                    ),
                    BattEvent::ChargingStarted => ultar_notify::post(
                        "system",
                        "battery",
                        &text(&lang, "Wird geladen", "Charging", "جارٍ الشحن"),
                        &format!("{} %", b.percent),
                        Urgency::Low,
                    ),
                    BattEvent::Full => ultar_notify::post(
                        "system",
                        "battery",
                        &text(&lang, "Akku voll", "Battery full", "البطارية ممتلئة"),
                        &text(
                            &lang,
                            "Ladegerät kann getrennt werden",
                            "You can unplug the charger",
                            "يمكنك فصل الشاحن",
                        ),
                        Urgency::Normal,
                    ),
                    BattEvent::SaverOn => {
                        bg.send(Msg::Sys(SysMsg::Saver(true)));
                        ultar_notify::post(
                            "system",
                            "battery",
                            &text(
                                &lang,
                                "Energiesparmodus an",
                                "Battery saver on",
                                "تم تشغيل توفير الطاقة",
                            ),
                            &format!("≤ {} %", cfg.saver_at),
                            Urgency::Low,
                        );
                    }
                    BattEvent::SaverOff => bg.send(Msg::Sys(SysMsg::Saver(false))),
                }
            }
            prev_batt = Some(cur);
        }

        // Wärme: ab 70 °C warnen, höchstens alle 30 min
        if let Some((zone, c)) = sysx::hottest_zone() {
            if c >= 70.0 && last_hot.map_or(true, |t| t.elapsed() > Duration::from_secs(1800)) {
                last_hot = Some(Instant::now());
                ultar_notify::post(
                    "system",
                    "thermal",
                    &text(&lang, "Gerät ist heiß", "Device is hot", "الجهاز ساخن"),
                    &format!("{c:.0} °C ({zone})"),
                    Urgency::Normal,
                );
            }
        }

        // Zeitabgleich: beim Start (sobald online) und danach alle 12 h
        let due = if st.last_sync_ok {
            now - st.last_sync > 12 * 3600
        } else {
            now - st.last_try > 300
        };
        if cfg.auto_time && sys::online() && due && started.elapsed() > Duration::from_secs(15) {
            st.last_try = now;
            match sync_time(&cfg.ntp_server) {
                Ok(d) => {
                    let try_at = st.last_try;
                    st = sys::load(STATE);
                    st.last_try = try_at;
                    if d.abs() > 2 {
                        bg.send(Msg::Sys(SysMsg::TimeSynced(d)));
                    }
                }
                Err(e) => {
                    st.last_sync_ok = false;
                    st.last_sync_msg = e;
                }
            }
        }

        // Termin-Erinnerungen
        let mut cal: sysx::Calendar = sys::load("calendar.toml");
        let due = sysx::due_reminders(&cal, now, set.tz_min);
        if !due.is_empty() {
            for i in due {
                let e = &mut cal.events[i];
                e.notified = true;
                let t = sysx::fmt_clock((e.at / 60) as i64, (e.at % 60) as i64, cfg.h12);
                ultar_notify::post(
                    "system",
                    "calendar",
                    &e.title,
                    &format!(
                        "{} {t}",
                        text(&lang, "Termin um", "Event at", "موعد الساعة")
                    ),
                    Urgency::High,
                );
            }
            let _ = sys::save("calendar.toml", &cal);
        }

        // Datenlimit (Monat)
        if cfg.data_limit_mb > 0 {
            let dt = sys::from_unix(now, set.tz_min);
            let usage: Usage = sys::load("net-usage.toml");
            let used = sysx::month_usage(&usage.days, dt.y, dt.mo);
            let lim = cfg.data_limit_mb * 1_000_000;
            let key = format!("{:04}-{:02}", dt.y, dt.mo);
            if st.limit_warned != key {
                st.limit_warned = key;
                st.limit_level = 0;
            }
            let level = if used >= lim {
                2
            } else if used * 10 >= lim * 8 {
                1
            } else {
                0
            };
            if level > st.limit_level {
                st.limit_level = level;
                ultar_notify::post(
                    "system",
                    "data",
                    &text(&lang, "Datenlimit", "Data limit", "حد البيانات"),
                    &format!("{} / {} MB", used / 1_000_000, cfg.data_limit_mb),
                    if level == 2 {
                        Urgency::High
                    } else {
                        Urgency::Normal
                    },
                );
            }
        }

        archive_notifications(&mut st);
        let _ = sys::save(STATE, &st);
        std::thread::sleep(Duration::from_secs(20));
    }
}
