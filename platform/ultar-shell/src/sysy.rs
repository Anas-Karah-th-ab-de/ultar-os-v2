//! Logik der zweiten Runde Systemfunktionen (Oberfläche: `app_more.rs`): Hijri-Datum, Gebetszeiten, Qibla,
//! Kompass/Wasserwaage, Rechner (Rate, Datum, Zahlensysteme), Kennwörter, Morse, Akku-Schätzung, Nutzungszeit,
//! Gerätepflege, Speicher-Analyse. Reine Funktionen, damit sie ohne Gerät prüfbar sind.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------- Einstellungen

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MoreCfg {
    // Sperrbildschirm / Notfall
    pub owner_info: String,
    pub emergency_number: String,
    pub emergency_contact: String,
    pub medical: String,
    // Anrufe
    pub blocked: Vec<String>,
    // Modi
    pub sleep_mode: bool,
    /// gemerkte Werte vor dem Schlafmodus (Helligkeit, Nachtlicht, Nicht stören)
    pub sleep_prev: (f32, bool, bool),
    pub auto_theme: bool,
    pub day_from: u16,
    pub day_to: u16,
    pub restart_on: bool,
    pub restart_at: u16,
    /// 0 = täglich, 1..=7 = Wochentag (1 = Montag)
    pub restart_day: u8,
    pub hourly_chime: bool,
    pub charge_alert: bool,
    pub break_min: u32,
    pub pocket: bool,
    pub storage_warn: bool,
    pub backup_remind: bool,
    // Ort für Gebetszeiten/Qibla
    pub city: usize,
    pub lat: f64,
    pub lon: f64,
    /// 0 MWL, 1 Ägypten, 2 Umm al-Qura, 3 Karatschi
    pub method: usize,
    pub compass_offset: f32,
    // Werkzeuge
    pub rates: Vec<(String, f64)>,
    pub counter: u32,
    pub counter_goal: u32,
    pub todos: Vec<(String, bool)>,
    pub reminders: Vec<(i64, String)>,
    pub countdowns: Vec<(String, i64, i64, i64)>,
    pub clip: Vec<String>,
    pub focus_min: u32,
}

impl Default for MoreCfg {
    fn default() -> Self {
        Self {
            owner_info: String::new(),
            emergency_number: "112".into(),
            emergency_contact: String::new(),
            medical: String::new(),
            blocked: Vec::new(),
            sleep_mode: false,
            sleep_prev: (1.0, false, false),
            auto_theme: false,
            day_from: 7 * 60,
            day_to: 19 * 60,
            restart_on: false,
            restart_at: 4 * 60,
            restart_day: 0,
            hourly_chime: false,
            charge_alert: false,
            break_min: 0,
            pocket: true,
            storage_warn: true,
            backup_remind: true,
            city: 0,
            lat: 0.0,
            lon: 0.0,
            method: 0,
            compass_offset: 0.0,
            // Basis: 1 Einheit der ersten Währung; Kurse trägt der Nutzer selbst ein (kein Netz nötig)
            rates: vec![
                ("USD".into(), 1.0),
                ("EUR".into(), 0.92),
                ("SYP".into(), 13000.0),
                ("TRY".into(), 34.0),
                ("SAR".into(), 3.75),
            ],
            counter: 0,
            counter_goal: 33,
            todos: Vec::new(),
            reminders: Vec::new(),
            countdowns: Vec::new(),
            clip: Vec::new(),
            focus_min: 25,
        }
    }
}

/// Laufende Statistik (eigene Datei, damit die Einstellungen klein bleiben)
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Stats {
    /// Tag "JJJJ-MM-TT" -> App -> Sekunden im Vordergrund
    pub usage: BTreeMap<String, BTreeMap<String, u64>>,
    /// Tag -> (Bildschirm eingeschaltet, entsperrt)
    pub wakes: BTreeMap<String, (u32, u32)>,
    /// Akkuverlauf (Unix-Zeit, Prozent, lädt)
    pub batt: Vec<(i64, u8, bool)>,
    pub last_backup_hint: i64,
    pub last_storage_warn: i64,
    pub last_restart: i64,
}

impl Stats {
    pub fn add_usage(&mut self, day: &str, app: &str, secs: u64) {
        *self
            .usage
            .entry(day.to_string())
            .or_default()
            .entry(app.to_string())
            .or_default() += secs;
        while self.usage.len() > 14 {
            let k = self.usage.keys().next().cloned().unwrap();
            self.usage.remove(&k);
        }
        while self.wakes.len() > 14 {
            let k = self.wakes.keys().next().cloned().unwrap();
            self.wakes.remove(&k);
        }
    }
    pub fn add_batt(&mut self, t: i64, pct: u8, charging: bool) {
        self.batt.push((t, pct, charging));
        let cut = t - 48 * 3600;
        self.batt.retain(|s| s.0 >= cut);
    }
    /// Nutzung eines Tages, größte zuerst
    pub fn day_usage(&self, day: &str) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self
            .usage
            .get(day)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
}

// ---------------------------------------------------------------- Kalenderrechnung

/// Julianische Tagesnummer (gregorianisch)
pub fn jdn(y: i64, m: i64, d: i64) -> i64 {
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + 12 * a - 3;
    d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045
}

/// Wochentag 0 = Montag … 6 = Sonntag
pub fn weekday(y: i64, m: i64, d: i64) -> usize {
    jdn(y, m, d).rem_euclid(7) as usize
}

pub fn days_between(a: (i64, i64, i64), b: (i64, i64, i64)) -> i64 {
    jdn(b.0, b.1, b.2) - jdn(a.0, a.1, a.2)
}

/// Datum aus julianischer Tagesnummer
pub fn from_jdn(j: i64) -> (i64, i64, i64) {
    let a = j + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - 146097 * b / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - 1461 * d / 4;
    let m = (5 * e + 2) / 153;
    (
        100 * b + d - 4800 + m / 10,
        m + 3 - 12 * (m / 10),
        e - (153 * m + 2) / 5 + 1,
    )
}

pub fn add_days(d: (i64, i64, i64), n: i64) -> (i64, i64, i64) {
    from_jdn(jdn(d.0, d.1, d.2) + n)
}

/// "TT.MM.JJJJ" oder "JJJJ-MM-TT" lesen
pub fn parse_date(s: &str) -> Option<(i64, i64, i64)> {
    let p: Vec<i64> = s
        .split(['.', '-', '/', ' '])
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    if p.len() != 3 {
        return None;
    }
    let (y, m, d) = if p[0] > 31 {
        (p[0], p[1], p[2])
    } else {
        (p[2], p[1], p[0])
    };
    ((1..=12).contains(&m)
        && (1..=31).contains(&d)
        && (1..=9999).contains(&y)
        && d <= crate::sys::days_in_month(y, m))
    .then_some((y, m, d))
}

pub const HIJRI_MONTHS: [&str; 12] = [
    "Muharram",
    "Safar",
    "Rabi' I",
    "Rabi' II",
    "Jumada I",
    "Jumada II",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qi'dah",
    "Dhu al-Hijjah",
];
pub const HIJRI_MONTHS_AR: [&str; 12] = [
    "محرّم",
    "صفر",
    "ربيع الأول",
    "ربيع الآخر",
    "جمادى الأولى",
    "جمادى الآخرة",
    "رجب",
    "شعبان",
    "رمضان",
    "شوّال",
    "ذو القعدة",
    "ذو الحجة",
];

/// Hijri-Datum (rechnerischer/tabellarischer Kalender; die Sichtung des Mondes kann ±1 Tag abweichen).
pub fn hijri(y: i64, m: i64, d: i64) -> (i64, i64, i64) {
    let l = jdn(y, m, d) - 1948440 + 10632;
    let n = (l - 1) / 10631;
    let l = l - 10631 * n + 354;
    let j = ((10985 - l) / 5316) * ((50 * l) / 17719) + (l / 5670) * ((43 * l) / 15238);
    let l = l - ((30 - j) / 15) * ((17719 * j) / 50) - (j / 16) * ((15238 * j) / 43) + 29;
    let hm = (24 * l) / 709;
    let hd = l - (709 * hm) / 24;
    (30 * n + j - 30, hm, hd)
}

// ---------------------------------------------------------------- Orte, Gebetszeiten, Qibla

/// (Name, arabischer Name, Breite, Länge)
pub const CITIES: [(&str, &str, f64, f64); 16] = [
    ("Damascus", "دمشق", 33.5138, 36.2765),
    ("Aleppo", "حلب", 36.2021, 37.1343),
    ("Homs", "حمص", 34.7324, 36.7137),
    ("Latakia", "اللاذقية", 35.5317, 35.7901),
    ("Beirut", "بيروت", 33.8938, 35.5018),
    ("Amman", "عمّان", 31.9454, 35.9284),
    ("Baghdad", "بغداد", 33.3152, 44.3661),
    ("Cairo", "القاهرة", 30.0444, 31.2357),
    ("Riyadh", "الرياض", 24.7136, 46.6753),
    ("Mecca", "مكة المكرمة", 21.4225, 39.8262),
    ("Dubai", "دبي", 25.2048, 55.2708),
    ("Istanbul", "إسطنبول", 41.0082, 28.9784),
    ("Berlin", "برلين", 52.52, 13.405),
    ("Paris", "باريس", 48.8566, 2.3522),
    ("London", "لندن", 51.5074, -0.1278),
    ("New York", "نيويورك", 40.7128, -74.006),
];

pub const METHODS: [(&str, f64, f64, f64); 4] = [
    ("MWL", 18.0, 17.0, 0.0),
    ("Egypt", 19.5, 17.5, 0.0),
    ("Umm al-Qura", 18.5, 0.0, 90.0),
    ("Karachi", 18.0, 18.0, 0.0),
];

fn dsin(d: f64) -> f64 {
    d.to_radians().sin()
}
fn dcos(d: f64) -> f64 {
    d.to_radians().cos()
}

/// Sonnenstand: (Deklination in Grad, Zeitgleichung in Stunden) für eine julianische Tageszahl (mit Bruchteil)
fn sun(jd: f64) -> (f64, f64) {
    let d = jd - 2451545.0;
    let g = (357.529 + 0.98560028 * d).rem_euclid(360.0);
    let q = (280.459 + 0.98564736 * d).rem_euclid(360.0);
    let l = (q + 1.915 * dsin(g) + 0.020 * dsin(2.0 * g)).rem_euclid(360.0);
    let e = 23.439 - 0.00000036 * d;
    let ra = (dcos(e) * dsin(l))
        .atan2(dcos(l))
        .to_degrees()
        .rem_euclid(360.0)
        / 15.0;
    let decl = (dsin(e) * dsin(l)).asin().to_degrees();
    let mut eqt = q / 15.0 - ra;
    if eqt > 12.0 {
        eqt -= 24.0;
    }
    if eqt < -12.0 {
        eqt += 24.0;
    }
    (decl, eqt)
}

/// Gebetszeiten in Minuten seit Mitternacht (Ortszeit): Fajr, Sonnenaufgang, Dhuhr, Asr, Maghrib, Isha.
/// `tz_min` = Versatz zur Weltzeit in Minuten. None, wenn die Sonne den Winkel nicht erreicht (hohe Breiten).
pub fn prayer_times(
    lat: f64,
    lon: f64,
    tz_min: i32,
    date: (i64, i64, i64),
    method: usize,
) -> [Option<u16>; 6] {
    let (_, fajr_a, isha_a, isha_min) = METHODS[method.min(METHODS.len() - 1)];
    let jd = jdn(date.0, date.1, date.2) as f64 - 0.5 - lon / (15.0 * 24.0);
    let tz = tz_min as f64 / 60.0;
    let noon = |t: f64| 12.0 + tz - lon / 15.0 - sun(jd + t / 24.0).1;
    // Stundenwinkel für Sonnenhöhe `alt` (negativ = unter dem Horizont)
    let hour_angle = |alt: f64, t: f64| -> Option<f64> {
        let decl = sun(jd + t / 24.0).0;
        let c = (dsin(alt) - dsin(lat) * dsin(decl)) / (dcos(lat) * dcos(decl));
        (-1.0..=1.0)
            .contains(&c)
            .then(|| c.acos().to_degrees() / 15.0)
    };
    let before = |alt: f64, guess: f64| hour_angle(alt, guess).map(|h| noon(guess) - h);
    let after = |alt: f64, guess: f64| hour_angle(alt, guess).map(|h| noon(guess) + h);
    let asr = {
        let decl = sun(jd + 13.0 / 24.0).0;
        let alt = (1.0 / (1.0 + (lat - decl).abs().to_radians().tan()))
            .atan()
            .to_degrees();
        after(alt, 13.0)
    };
    let maghrib = after(-0.833, 18.0);
    let isha = if isha_min > 0.0 {
        maghrib.map(|m| m + isha_min / 60.0)
    } else {
        after(-isha_a, 18.0)
    };
    let v = [
        before(-fajr_a, 5.0),
        before(-0.833, 6.0),
        Some(noon(12.0) + 1.0 / 60.0),
        asr,
        maghrib,
        isha,
    ];
    v.map(|t| t.map(|h| ((h.rem_euclid(24.0) * 60.0).round() as u16) % 1440))
}

/// Richtung zur Kaaba in Grad (0 = Norden, im Uhrzeigersinn)
pub fn qibla(lat: f64, lon: f64) -> f64 {
    let (klat, klon): (f64, f64) = (21.4225, 39.8262);
    let dl = (klon - lon).to_radians();
    let y = dl.sin();
    let x = dcos(lat) * klat.to_radians().tan() - dsin(lat) * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Kompass-Richtung aus dem Magnetfeld (Gerät liegt flach): 0 = Norden, 90 = Osten.
pub fn heading(mx: f32, my: f32, offset: f32) -> f32 {
    ((-mx).atan2(my).to_degrees() + offset).rem_euclid(360.0)
}

pub fn cardinal(deg: f32) -> &'static str {
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
        [(((deg + 22.5).rem_euclid(360.0)) / 45.0) as usize % 8]
}

/// Neigung in Grad aus dem Beschleunigungssensor: (links/rechts, vor/zurück)
pub fn tilt(ax: f32, ay: f32, az: f32) -> (f32, f32) {
    (
        ax.atan2((ay * ay + az * az).sqrt()).to_degrees(),
        ay.atan2((ax * ax + az * az).sqrt()).to_degrees(),
    )
}

// ---------------------------------------------------------------- Rechner

/// Monatsrate eines Darlehens (Annuität); Zins in % pro Jahr.
pub fn loan_payment(amount: f64, rate_pct: f64, months: u32) -> f64 {
    if months == 0 {
        return 0.0;
    }
    let r = rate_pct / 100.0 / 12.0;
    if r.abs() < 1e-12 {
        amount / months as f64
    } else {
        amount * r / (1.0 - (1.0 + r).powi(-(months as i32)))
    }
}

/// Zahl in Basis 2/8/10/16 lesen
pub fn parse_base(s: &str, base: u32) -> Option<u64> {
    let t = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0b")
        .replace([' ', '_'], "");
    (!t.is_empty())
        .then(|| u64::from_str_radix(&t, base).ok())
        .flatten()
}

pub fn to_base(v: u64, base: u32) -> String {
    match base {
        2 => format!("{v:b}"),
        8 => format!("{v:o}"),
        16 => format!("{v:X}"),
        _ => v.to_string(),
    }
}

/// Währung umrechnen: Kurse sind „Einheiten je 1 Basis".
pub fn convert_rate(rates: &[(String, f64)], from: usize, to: usize, v: f64) -> f64 {
    match (rates.get(from), rates.get(to)) {
        (Some(a), Some(b)) if a.1 > 0.0 => v / a.1 * b.1,
        _ => 0.0,
    }
}

/// Kennwort aus Zufallsbytes: Klassen wählbar; ohne ähnliche Zeichen (0/O, 1/l/I).
pub fn password(random: &[u8], len: usize, upper: bool, digits: bool, symbols: bool) -> String {
    let mut set: Vec<u8> = b"abcdefghijkmnopqrstuvwxyz".to_vec();
    if upper {
        set.extend_from_slice(b"ABCDEFGHJKLMNPQRSTUVWXYZ");
    }
    if digits {
        set.extend_from_slice(b"23456789");
    }
    if symbols {
        set.extend_from_slice(b"!#$%&*+-=?@");
    }
    // Verwerfen statt Modulo, damit jedes Zeichen gleich wahrscheinlich ist
    let lim = (256 / set.len()) * set.len();
    random
        .iter()
        .filter(|b| (**b as usize) < lim)
        .take(len)
        .map(|b| set[*b as usize % set.len()] as char)
        .collect()
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut b = vec![0u8; n];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        // Notbehelf ohne /dev/urandom: Uhrzeit mischen (nur für Würfel, nicht für Kennwörter gedacht)
        let mut x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
        for v in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = (x >> 24) as u8;
        }
    }
    b
}

/// Gleichverteilte Zahl in [lo, hi]
pub fn random_range(lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    let lim = u64::MAX - u64::MAX % span;
    loop {
        let b = random_bytes(8);
        let v = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        if v < lim {
            return lo + (v % span) as i64;
        }
    }
}

/// Morse-Folge als (Licht an?, Millisekunden); Einheit 200 ms. Unbekannte Zeichen werden übersprungen.
pub fn morse(text: &str) -> Vec<(bool, u32)> {
    const U: u32 = 200;
    let code = |c: char| -> &'static str {
        match c.to_ascii_uppercase() {
            'S' => "...",
            'O' => "---",
            'E' => ".",
            'T' => "-",
            'A' => ".-",
            'N' => "-.",
            'H' => "....",
            'L' => ".-..",
            'P' => ".--.",
            _ => "",
        }
    };
    let mut out = Vec::new();
    for c in text.chars() {
        if c == ' ' {
            out.push((false, 7 * U));
            continue;
        }
        for s in code(c).chars() {
            out.push((true, if s == '.' { U } else { 3 * U }));
            out.push((false, U));
        }
        if let Some(l) = out.last_mut() {
            *l = (false, 3 * U);
        }
    }
    out
}

// ---------------------------------------------------------------- Akku, Pflege, Speicher

/// Restlaufzeit (entladen) bzw. Zeit bis voll (laden) in Minuten aus dem Verlauf der letzten Stunden.
pub fn battery_estimate(hist: &[(i64, u8, bool)]) -> Option<(bool, u32)> {
    let last = *hist.last()?;
    // zusammenhängender Abschnitt mit gleichem Ladezustand, höchstens 6 h zurück
    let seg: Vec<&(i64, u8, bool)> = hist
        .iter()
        .rev()
        .take_while(|s| s.2 == last.2 && last.0 - s.0 <= 6 * 3600)
        .collect();
    let first = **seg.last()?;
    let dt = (last.0 - first.0) as f64;
    let dp = last.1 as f64 - first.1 as f64;
    if dt < 900.0 || dp.abs() < 2.0 {
        return None;
    }
    let per_s = dp / dt;
    let mins = if last.2 && per_s > 0.0 {
        (100.0 - last.1 as f64) / per_s / 60.0
    } else if !last.2 && per_s < 0.0 {
        last.1 as f64 / -per_s / 60.0
    } else {
        return None;
    };
    Some((last.2, mins.round().clamp(1.0, 14.0 * 24.0 * 60.0) as u32))
}

pub struct Health {
    pub storage_free_pct: f32,
    pub ram_free_pct: f32,
    pub batt_health: Option<String>,
    pub temp_c: Option<f32>,
    pub backup_age_days: Option<i64>,
    pub uptime_days: f32,
    pub has_pin: bool,
}

/// Zustandswert 0..100 und Hinweise (Schlüssel für die Übersetzung)
pub fn health_score(h: &Health) -> (u8, Vec<&'static str>) {
    let mut s = 100i32;
    let mut tips = Vec::new();
    if h.storage_free_pct < 10.0 {
        s -= 25;
        tips.push("Storage is almost full – free up space");
    } else if h.storage_free_pct < 20.0 {
        s -= 10;
        tips.push("Storage is getting full");
    }
    if h.ram_free_pct < 10.0 {
        s -= 10;
        tips.push("Memory is tight – close running programs");
    }
    if h.batt_health
        .as_deref()
        .is_some_and(|b| !b.eq_ignore_ascii_case("good") && !b.is_empty())
    {
        s -= 15;
        tips.push("Battery reports a problem");
    }
    if h.temp_c.is_some_and(|t| t >= 60.0) {
        s -= 15;
        tips.push("Device is hot – let it cool down");
    }
    match h.backup_age_days {
        None => {
            s -= 10;
            tips.push("No backup yet – create one");
        }
        Some(d) if d > 30 => {
            s -= 5;
            tips.push("Last backup is older than 30 days");
        }
        _ => {}
    }
    if h.uptime_days > 14.0 {
        s -= 5;
        tips.push("Running for a long time – a restart helps");
    }
    if !h.has_pin {
        s -= 10;
        tips.push("No PIN set – protect the device");
    }
    (s.clamp(0, 100) as u8, tips)
}

/// Größte Einträge unter `root` (Ordner mit Gesamtgröße, Dateien), größte zuerst. Folgt keinen Verknüpfungen.
pub fn biggest(root: &str, max: usize) -> Vec<(String, u64, bool)> {
    fn size(p: &std::path::Path, depth: u32) -> u64 {
        let Ok(md) = std::fs::symlink_metadata(p) else {
            return 0;
        };
        if md.is_file() {
            return md.len();
        }
        if !md.is_dir() || depth > 12 {
            return 0;
        }
        std::fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| size(&e.path(), depth + 1)).sum())
            .unwrap_or(0)
    }
    let mut v: Vec<(String, u64, bool)> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    let p = e.path();
                    let is_dir = std::fs::symlink_metadata(&p)
                        .map(|m| m.is_dir())
                        .unwrap_or(false);
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        size(&p, 0),
                        is_dir,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(max);
    v
}

/// Laufende Programme (PID, Name, Speicher in Byte), größte zuerst; Kernel-Threads ausgelassen.
pub fn processes(max: usize) -> Vec<(u32, String, u64)> {
    let mut v: Vec<(u32, String, u64)> = std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let pid: u32 = e.file_name().to_string_lossy().parse().ok()?;
                    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
                    let name = st
                        .lines()
                        .find_map(|l| l.strip_prefix("Name:"))
                        .map(|s| s.trim().to_string())?;
                    let rss = st
                        .lines()
                        .find_map(|l| l.strip_prefix("VmRSS:"))
                        .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok())?;
                    Some((pid, name, rss * 1024))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.2.cmp(&a.2));
    v.truncate(max);
    v
}

/// Programme, die für das System nötig sind – werden nicht zum Beenden angeboten.
pub fn protected_process(name: &str) -> bool {
    [
        "ultar-init",
        "ultar-shell",
        "ultar-net",
        "ultar-ril",
        "init",
        "wpa_supplicant",
        "ultar-audio",
        "propsvc",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// Zwischenablage: neuester Eintrag vorn, ohne Doppelte, höchstens 20, je höchstens 2000 Zeichen.
pub fn clip_push(list: &mut Vec<String>, s: &str) {
    let s: String = s.chars().take(2000).collect();
    if s.trim().is_empty() {
        return;
    }
    list.retain(|x| x != &s);
    list.insert(0, s);
    list.truncate(20);
}

pub fn fmt_hm(min: u16) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

pub fn fmt_dur(secs: u64) -> String {
    if secs >= 3600 {
        format!("{} h {:02} min", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{} min", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_math() {
        assert_eq!(weekday(2026, 11, 20), 4, "20.11.2026 ist ein Freitag");
        assert_eq!(days_between((2026, 11, 20), (2027, 1, 1)), 42);
        assert_eq!(add_days((2026, 12, 31), 1), (2027, 1, 1));
        assert_eq!(from_jdn(jdn(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(parse_date("20.11.2026"), Some((2026, 11, 20)));
        assert_eq!(parse_date("2026-11-20"), Some((2026, 11, 20)));
        assert_eq!(parse_date("31.02.2026"), None);
    }

    #[test]
    fn hijri_known_dates() {
        // rechnerischer Kalender: 1.1.2000 = 24 Ramadan 1420; 1 Muharram 1446 = 8.7.2024 (Sichtung: 7.7.)
        assert_eq!(hijri(2000, 1, 1), (1420, 9, 24));
        assert_eq!(hijri(2024, 7, 8), (1446, 1, 1));
        assert_eq!(hijri(2024, 7, 7), (1445, 12, 30));
    }

    #[test]
    fn prayer_times_damascus() {
        // Damaskus, 20.11.2026, UTC+3: Mittag ~12:21, Sonnenaufgang ~07:11, Untergang ~17:31
        let t = prayer_times(33.5138, 36.2765, 180, (2026, 11, 20), 0);
        let near =
            |v: Option<u16>, want: u16, tol: i32| (v.unwrap() as i32 - want as i32).abs() <= tol;
        assert!(near(t[2], 12 * 60 + 21, 3), "Dhuhr {:?}", t[2]);
        assert!(near(t[1], 7 * 60 + 11, 5), "Sonnenaufgang {:?}", t[1]);
        assert!(near(t[4], 17 * 60 + 31, 5), "Maghrib {:?}", t[4]);
        let v: Vec<u16> = t.iter().map(|x| x.unwrap()).collect();
        assert!(v.windows(2).all(|w| w[0] < w[1]), "aufsteigend: {v:?}");
        // Umm al-Qura: Isha 90 min nach Maghrib
        let u = prayer_times(21.4225, 39.8262, 180, (2026, 11, 20), 2);
        assert_eq!(u[5].unwrap(), u[4].unwrap() + 90);
        // hohe Breite im Sommer: Fajr-Winkel wird nicht erreicht
        assert!(prayer_times(65.0, 25.0, 180, (2026, 6, 21), 0)[0].is_none());
    }

    #[test]
    fn qibla_directions() {
        assert!(
            (qibla(33.5138, 36.2765) - 164.6).abs() < 1.5,
            "Damaskus {}",
            qibla(33.5138, 36.2765)
        );
        assert!(
            (qibla(52.52, 13.405) - 136.7).abs() < 1.5,
            "Berlin {}",
            qibla(52.52, 13.405)
        );
        assert!(
            (qibla(40.7128, -74.006) - 58.5).abs() < 1.5,
            "New York {}",
            qibla(40.7128, -74.006)
        );
    }

    #[test]
    fn compass_and_level() {
        assert!((heading(0.0, 30.0, 0.0) - 0.0).abs() < 0.1);
        assert!((heading(-30.0, 0.0, 0.0) - 90.0).abs() < 0.1);
        assert_eq!(cardinal(0.0), "N");
        assert_eq!(cardinal(92.0), "E");
        assert_eq!(cardinal(350.0), "N");
        let (a, b) = tilt(0.0, 0.0, 9.81);
        assert!(a.abs() < 0.01 && b.abs() < 0.01);
        assert!((tilt(9.81, 0.0, 0.0).0 - 90.0).abs() < 0.1);
    }

    #[test]
    fn calculators() {
        assert!((loan_payment(10000.0, 6.0, 24) - 443.21).abs() < 0.01);
        assert!((loan_payment(1200.0, 0.0, 12) - 100.0).abs() < 1e-9);
        assert_eq!(parse_base("ff", 16), Some(255));
        assert_eq!(to_base(255, 2), "11111111");
        assert_eq!(parse_base("12", 2), None);
        let r = vec![("USD".to_string(), 1.0), ("EUR".to_string(), 0.5)];
        assert!((convert_rate(&r, 1, 0, 10.0) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn passwords() {
        let rnd: Vec<u8> = (0..=255u8).cycle().take(600).collect();
        let p = password(&rnd, 16, true, true, true);
        assert_eq!(p.chars().count(), 16);
        assert!(!p.contains(['0', 'O', 'l', '1', 'I']));
        let q = password(&rnd, 12, false, false, false);
        assert!(q.chars().all(|c| c.is_ascii_lowercase()));
        let v = random_range(1, 6);
        assert!((1..=6).contains(&v));
    }

    #[test]
    fn morse_sos() {
        let m = morse("SOS");
        let on: Vec<u32> = m.iter().filter(|s| s.0).map(|s| s.1).collect();
        assert_eq!(on, vec![200, 200, 200, 600, 600, 600, 200, 200, 200]);
    }

    #[test]
    fn battery_estimates() {
        // 10 % in einer Stunde entladen, noch 50 % -> 5 h
        let h: Vec<(i64, u8, bool)> = (0..=6)
            .map(|i| (i * 600, 60 - (i as u8 * 10 / 6), false))
            .collect();
        let (chg, m) = battery_estimate(&h).unwrap();
        assert!(!chg && (280..=320).contains(&m), "{m}");
        // lädt 20 % in 30 min bei 80 % -> 30 min
        let c = vec![(0, 60, true), (900, 70, true), (1800, 80, true)];
        assert_eq!(battery_estimate(&c), Some((true, 30)));
        assert_eq!(battery_estimate(&[(0, 50, false), (60, 50, false)]), None);
    }

    #[test]
    fn health_and_clip() {
        let good = Health {
            storage_free_pct: 60.0,
            ram_free_pct: 50.0,
            batt_health: Some("Good".into()),
            temp_c: Some(30.0),
            backup_age_days: Some(3),
            uptime_days: 1.0,
            has_pin: true,
        };
        assert_eq!(health_score(&good), (100, vec![]));
        let bad = Health {
            storage_free_pct: 5.0,
            ram_free_pct: 5.0,
            batt_health: Some("Overheat".into()),
            temp_c: Some(65.0),
            backup_age_days: None,
            uptime_days: 20.0,
            has_pin: false,
        };
        let (s, tips) = health_score(&bad);
        assert_eq!(s, 10);
        assert_eq!(tips.len(), 7);
        let mut c = vec![];
        clip_push(&mut c, "a");
        clip_push(&mut c, "b");
        clip_push(&mut c, "a");
        clip_push(&mut c, "  ");
        assert_eq!(c, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn usage_stats() {
        let mut s = Stats::default();
        s.add_usage("2026-11-20", "Phone", 60);
        s.add_usage("2026-11-20", "Files", 600);
        s.add_usage("2026-11-20", "Phone", 30);
        assert_eq!(
            s.day_usage("2026-11-20"),
            vec![("Files".to_string(), 600), ("Phone".to_string(), 90)]
        );
        for d in 0..20 {
            s.add_usage(&format!("2026-10-{:02}", d + 1), "x", 1);
        }
        assert!(s.usage.len() <= 14);
        assert_eq!(fmt_dur(3725), "1 h 02 min");
    }
}
