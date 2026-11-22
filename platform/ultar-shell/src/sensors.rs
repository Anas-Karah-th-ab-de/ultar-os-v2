//! Sensoren: liest alle Hardware-Sensoren live aus /sys/class/sensors und /sys/bus/iio/devices.
//! Untermodul von `app` (wie `monitor`), damit es die Zeichen-Helfer (header, meter, caption) nutzen kann.
//! Nur sysfs – kein zusätzlicher Dienst. Robust gegen fehlende Dateien, keine Panics.

use super::{App, Screen};
use crate::gfx::Rect;
use crate::ui::{Ic, Ui};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Accel,
    Gyro,
    Light,
    Proximity,
    Magnetic,
    Steps,
    Grip,
    Tilt,
    Motion,
    Pressure,
    Other,
}

#[derive(Clone)]
pub struct SensorReading {
    /// Roh-Name aus sysfs (z. B. "accelerometer_sensor")
    pub name: String,
    pub kind: Kind,
    /// 1 Wert (Licht, Näherung, Schritte) oder 3 (Beschleunigung, Gyro, Magnet)
    pub values: Vec<f32>,
    pub unit: &'static str,
    pub enabled: Option<bool>,
    pub present: bool,
}

fn rd(p: &str) -> String {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Erste existierende Datei aus einer Kandidatenliste lesen.
fn rd_any(base: &str, cands: &[&str]) -> Option<String> {
    for c in cands {
        let v = rd(&format!("{base}/{c}"));
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

/// Mehrere Zahlen aus einem String ("x y z" oder "x,y,z") ziehen.
fn nums(s: &str) -> Vec<f32> {
    s.split([' ', ',', '\t'])
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f32>().ok())
        .collect()
}

fn kind_for(name: &str) -> (Kind, &'static str) {
    let n = name.to_lowercase();
    if n.contains("accel") {
        (Kind::Accel, "m/s² (roh)")
    } else if n.contains("gyro") {
        (Kind::Gyro, "°/s (roh)")
    } else if n.contains("magn") || n.contains("magnetic") || n.contains("compass") {
        (Kind::Magnetic, "µT (roh)")
    } else if n.contains("light") || n.contains("illum") {
        (Kind::Light, "lux")
    } else if n.contains("prox") {
        (Kind::Proximity, "")
    } else if n.contains("step") {
        (Kind::Steps, "Schritte")
    } else if n.contains("grip") {
        (Kind::Grip, "")
    } else if n.contains("tilt") {
        (Kind::Tilt, "")
    } else if n.contains("motion") || n.contains("smd") {
        (Kind::Motion, "")
    } else if n.contains("pressure") || n.contains("baro") {
        (Kind::Pressure, "hPa")
    } else {
        (Kind::Other, "")
    }
}

/// Werte je nach Sensortyp aus den üblichen Samsung-Attributen lesen.
fn read_sensor_values(base: &str, kind: Kind) -> Vec<f32> {
    match kind {
        Kind::Accel | Kind::Gyro | Kind::Magnetic => {
            rd_any(base, &["raw_data", "data", "raw_value", "value"])
                .map(|s| nums(&s))
                .unwrap_or_default()
        }
        Kind::Light => rd_any(base, &["lux", "raw_data", "value", "adc", "data"])
            .map(|s| nums(&s))
            .unwrap_or_default(),
        Kind::Proximity => rd_any(
            base,
            &["state", "prox_raw", "raw_data", "adc", "value", "data"],
        )
        .map(|s| nums(&s))
        .unwrap_or_default(),
        Kind::Steps => rd_any(base, &["step_cnt", "step_count", "value", "raw_data"])
            .map(|s| nums(&s))
            .unwrap_or_default(),
        Kind::Pressure => rd_any(base, &["sea_level_pressure", "raw_data", "value"])
            .map(|s| nums(&s))
            .unwrap_or_default(),
        _ => rd_any(base, &["raw_data", "value", "state", "data"])
            .map(|s| nums(&s))
            .unwrap_or_default(),
    }
}

/// Alle Sensoren erfassen: zuerst /sys/class/sensors (Samsung-Sensorhub), dann IIO.
pub fn read_all() -> Vec<SensorReading> {
    let mut out: Vec<SensorReading> = Vec::new();

    // 1) /sys/class/sensors/<name>/
    if let Ok(rdir) = std::fs::read_dir("/sys/class/sensors") {
        for e in rdir.flatten() {
            let dname = e.file_name().to_string_lossy().into_owned();
            // Steuerknoten überspringen
            if dname == "ssc_core" || dname.is_empty() {
                continue;
            }
            let base = e.path().to_string_lossy().into_owned();
            let label = rd(&format!("{base}/name"));
            let shown = if label.is_empty() {
                dname.clone()
            } else {
                label
            };
            let (kind, unit) = kind_for(&shown);
            let values = read_sensor_values(&base, kind);
            let enabled = match rd_any(&base, &["enable"]) {
                Some(v) => Some(v.trim() != "0" && !v.trim().is_empty()),
                None => None,
            };
            out.push(SensorReading {
                name: shown,
                kind,
                values,
                unit,
                enabled,
                present: true,
            });
        }
    }

    // 2) IIO-Geräte (/sys/bus/iio/devices/iio:deviceN) – generisch alle in_*_raw / in_*_input
    if let Ok(rdir) = std::fs::read_dir("/sys/bus/iio/devices") {
        for e in rdir.flatten() {
            let dname = e.file_name().to_string_lossy().into_owned();
            if !dname.starts_with("iio:device") {
                continue;
            }
            let base = e.path().to_string_lossy().into_owned();
            let label = rd(&format!("{base}/name"));
            let shown = if label.is_empty() {
                dname.clone()
            } else {
                label
            };
            // schon über /sys/class/sensors erfasst? dann überspringen
            if out.iter().any(|s| s.name.eq_ignore_ascii_case(&shown)) {
                continue;
            }
            let (kind, unit) = kind_for(&shown);
            let mut values = Vec::new();
            // gängige IIO-Kanäle in fester Reihenfolge
            for ch in [
                "in_accel_x_raw",
                "in_accel_y_raw",
                "in_accel_z_raw",
                "in_anglvel_x_raw",
                "in_anglvel_y_raw",
                "in_anglvel_z_raw",
                "in_magn_x_raw",
                "in_magn_y_raw",
                "in_magn_z_raw",
                "in_illuminance_input",
                "in_illuminance_raw",
                "in_proximity_raw",
                "in_pressure_input",
            ] {
                let v = rd(&format!("{base}/{ch}"));
                if let Ok(n) = v.parse::<f32>() {
                    values.push(n);
                }
            }
            if values.is_empty() {
                continue; // reiner Trigger/ohne lesbare Kanäle
            }
            out.push(SensorReading {
                name: shown,
                kind,
                values,
                unit,
                enabled: None,
                present: true,
            });
        }
    }

    out.sort_by_key(|s| kind_order(s.kind));
    out
}

fn kind_order(k: Kind) -> u8 {
    match k {
        Kind::Accel => 0,
        Kind::Gyro => 1,
        Kind::Magnetic => 2,
        Kind::Light => 3,
        Kind::Proximity => 4,
        Kind::Pressure => 5,
        Kind::Steps => 6,
        Kind::Tilt => 7,
        Kind::Grip => 8,
        Kind::Motion => 9,
        Kind::Other => 10,
    }
}

/// i18n-Schlüssel je Sensortyp (Anzeigename).
fn label_key(k: Kind) -> &'static str {
    match k {
        Kind::Accel => "Acceleration",
        Kind::Gyro => "Gyroscope",
        Kind::Magnetic => "Magnetic field",
        Kind::Light => "Light",
        Kind::Proximity => "Proximity",
        Kind::Pressure => "Pressure",
        Kind::Steps => "Steps",
        Kind::Tilt => "Tilt",
        Kind::Grip => "Grip",
        Kind::Motion => "Motion",
        Kind::Other => "Sensor",
    }
}

fn fmt_values(r: &SensorReading) -> String {
    if r.values.is_empty() {
        return "—".into();
    }
    match r.kind {
        Kind::Accel | Kind::Gyro | Kind::Magnetic if r.values.len() >= 3 => {
            format!(
                "x {:.1}  y {:.1}  z {:.1}  {}",
                r.values[0], r.values[1], r.values[2], r.unit
            )
        }
        Kind::Proximity => {
            let v = r.values[0];
            format!("{}", if v > 0.0 { "nah" } else { "fern" })
        }
        Kind::Light => format!("{:.0} {}", r.values[0], r.unit),
        Kind::Steps => format!("{:.0} {}", r.values[0], r.unit),
        _ => {
            let s: Vec<String> = r.values.iter().map(|v| format!("{v:.1}")).collect();
            format!("{} {}", s.join(" · "), r.unit).trim().to_string()
        }
    }
}

impl App {
    /// Bildschirm „Sensoren": zeigt alle erkannten Sensoren mit Live-Werten.
    pub(super) fn s_sensors(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Sensors"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Sensors) {
            self.caption(ui, &mut y, self.t("Turned off in privacy settings"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        let list = read_all();
        if list.is_empty() {
            self.caption(ui, &mut y, self.t("No sensors found"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        let n = list.len();
        self.caption(ui, &mut y, &format!("{n} {}", self.t("sensors detected")));
        for r in &list {
            let title = self.t(label_key(r.kind)).to_string();
            let val = fmt_values(r);
            let sub = if r.name.eq_ignore_ascii_case(&title) {
                None
            } else {
                Some(r.name.as_str())
            };
            let en = match r.enabled {
                Some(true) => format!("{}  ·  {}", val, self.t("on")),
                Some(false) => format!("{}  ·  {}", val, self.t("off")),
                None => val,
            };
            ui.row(&mut y, Some(Ic::Chip), &title, sub, Some(&en), false);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// true, wenn der Sensoren-Bildschirm offen ist (für laufende Aktualisierung).
    pub(super) fn sensors_open(&self) -> bool {
        matches!(self.stack.last().map(|s| &s.0), Some(Screen::Sensors))
    }
}
