//! Ultar Mind, Schicht 1: versteht Anweisungen sofort und ohne Sprachmodell – reines Rust, kein Ubuntu,
//! kein Netz. Deutsch, Englisch und Arabisch. Liefert einen Werkzeugaufruf (dieselben Werkzeuge wie der
//! Agent) oder eine kurze Antwort. Was hier nicht verstanden wird, geht an Schicht 2 (Sprachmodell), falls vorhanden.
//! Gesamtbild: docs/HELFER-KERN.md

use crate::agent::Call;
use serde_json::json;

#[derive(Debug, PartialEq)]
pub enum Intent {
    /// Werkzeug ausführen
    Tool(Call),
    /// Link prüfen
    Link(String),
    /// Dokumentenscanner öffnen
    Scan,
    /// Uhrzeit/Datum sagen
    Time,
    /// Helfer wegschicken
    Away,
    /// feste Antwort (Schlüssel für die Übersetzung)
    Say(&'static str),
    /// Erinnerung anlegen
    Remind(When, String),
    /// Erinnerungen aufzählen
    Reminders,
    /// alle Erinnerungen löschen
    ForgetReminders,
    /// sich vorstellen und durch das System führen
    Tour,
    /// Sprachsteuerung einrichten (Weckwort + Stimmprobe)
    VoiceSetup,
    /// geheimes Wort für den gesperrten Bildschirm einlernen
    VoiceSecret,
    VoiceOff,
    /// eigenen Sprachbefehl einlernen: der Text ist die Anweisung, die dann ausgeführt wird
    VoiceTeach(String),
}

/// Zeitpunkt einer Erinnerung
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum When {
    /// in so vielen Sekunden
    In(u64),
    /// zur Uhrzeit (Stunde, Minute) – heute, sonst morgen
    At(u32, u32),
}

fn call(tool: &str, args: serde_json::Value) -> Intent {
    Intent::Tool(Call {
        tool: tool.into(),
        args,
    })
}

fn has(t: &str, words: &[&str]) -> bool {
    words.iter().any(|w| t.contains(w))
}

/// Erste Zahl im Text (auch arabisch-indische Ziffern).
fn number(t: &str) -> Option<f32> {
    let norm: String = t
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            _ => c,
        })
        .collect();
    let mut cur = String::new();
    for c in norm.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            return cur.parse().ok();
        }
    }
    None
}

const ON: [&str; 14] = [
    " an",
    "ein",
    " on",
    "start",
    "aktiv",
    "enable",
    "شغل",
    "شغّل",
    "افتح",
    "فعل",
    "فعّل",
    "تشغيل",
    "anmachen",
    "turn on",
];
const OFF: [&str; 14] = [
    "aus",
    " off",
    "stop",
    "deaktiv",
    "disable",
    "اطفئ",
    "أطفئ",
    "اغلق",
    "أغلق",
    "اطفي",
    "طفي",
    "ايقاف",
    "إيقاف",
    "turn off",
];

fn on_off(t: &str) -> bool {
    // „aus" gewinnt, wenn beides vorkommt („mach das Licht wieder aus")
    !has(t, &OFF) || (has(t, &ON) && !has(t, &["aus", " off"]))
}

const APPS: [(&[&str], &str); 17] = [
    (&["kamera", "camera", "كاميرا"], "camera"),
    (
        &[
            "galerie",
            "gallery",
            "fotos",
            "photos",
            "bilder",
            "معرض",
            "الصور",
        ],
        "gallery",
    ),
    (&["telefon", "phone", "anruf", "هاتف", "الاتصال"], "phone"),
    (&["nachricht", "messages", "sms", "رسائل"], "messages"),
    (&["kontakt", "contacts", "جهات", "الأسماء"], "contacts"),
    (&["notiz", "notes", "ملاحظات"], "notes"),
    (&["uhr", "clock", "wecker", "الساعة", "ساعة"], "clock"),
    (&["rechner", "calculator", "حاسبة"], "calculator"),
    (&["datei", "files", "ملفات"], "files"),
    (&["browser", "internet", "متصفح"], "browser"),
    (&["terminal", "konsole", "طرفية"], "terminal"),
    (&["netzwerk", "network", "الشبكة"], "network"),
    (&["datenschutz", "privacy", "خصوصية"], "privacy"),
    (&["speicher", "storage", "تخزين"], "storage"),
    (&["anzeige", "display", "bildschirm", "الشاشة"], "display"),
    (
        &["einstellung", "settings", "إعدادات", "اعدادات"],
        "settings",
    ),
    (&["startseite", "home", "الرئيسية"], "home"),
];

/// Anweisung verstehen. `None` = nicht erkannt.
pub fn understand(text: &str) -> Option<Intent> {
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(u) = crate::linkcheck::find_urls(raw).into_iter().next() {
        return Some(Intent::Link(u));
    }
    let t = format!(" {} ", raw.to_lowercase());
    let t = t.as_str();

    if has(
        t,
        &[
            "geh weg",
            "verschwinde",
            "go away",
            "hide",
            "اذهب",
            "اختف",
            "اختفي",
        ],
    ) {
        return Some(Intent::Away);
    }
    // Sprache: vor allem anderen, weil der Befehlstext beliebige Wörter enthalten darf
    for pre in [
        "lerne sprachbefehl ",
        "sprachbefehl lernen ",
        "sprachbefehl ",
        "teach voice command ",
        "voice command ",
        "علمني أمر صوتي ",
        "أمر صوتي ",
    ] {
        let low = raw.to_lowercase();
        if let Some(i) = low.find(pre) {
            let rest = raw
                .chars()
                .skip(low[..i + pre.len()].chars().count())
                .collect::<String>()
                .trim()
                .trim_matches(['"', '„', '“', ':'])
                .trim()
                .to_string();
            if !rest.is_empty() {
                return Some(Intent::VoiceTeach(rest));
            }
        }
    }
    if has(
        t,
        &[
            "geheimes wort",
            "geheimwort",
            "secret word",
            "كلمة سرية",
            "الكلمة السرية",
        ],
    ) {
        return Some(Intent::VoiceSecret);
    }
    if has(
        t,
        &[
            "sprache aus",
            "stimme aus",
            "nicht mehr zuhören",
            "voice off",
            "stop listening",
            "أوقف الاستماع",
            "أوقف الصوت",
        ],
    ) {
        return Some(Intent::VoiceOff);
    }
    if has(
        t,
        &[
            "sprache einrichten",
            "stimme einrichten",
            "sprachsteuerung",
            "hey km",
            "voice setup",
            "set up voice",
            "voice control",
            "إعداد الصوت",
            "الأوامر الصوتية",
            "التحكم الصوتي",
        ],
    ) {
        return Some(Intent::VoiceSetup);
    }
    // Erinnerungen und Termine – vor allem anderen, weil der Text beliebige Wörter enthalten darf
    if has(
        t,
        &[
            "meine erinnerungen",
            "welche erinnerungen",
            "meine termine",
            "my reminders",
            "list reminders",
            "تذكيراتي",
            "مواعيدي",
        ],
    ) {
        return Some(Intent::Reminders);
    }
    if has(t, &["erinner", "remind", "ذكرني", "ذكّرني", "تذكير"])
        && has(
            t,
            &[
                "lösch", "losch", "delete", "clear", "vergiss", "forget", "احذف", "امسح",
            ],
        )
    {
        return Some(Intent::ForgetReminders);
    }
    if has(
        t,
        &[
            "erinner",
            "remind",
            "termin ",
            "appointment",
            "aufgabe ",
            "ذكرني",
            "ذكّرني",
            "موعد",
            "مهمة",
        ],
    ) {
        if let Some((when, what)) = remind(raw) {
            return Some(Intent::Remind(when, what));
        }
        // ohne Zeitangabe nur nachfragen, wenn wirklich eine Erinnerung gemeint war
        if has(t, &["erinner", "remind", "ذكرني", "ذكّرني"]) {
            return Some(Intent::Say("Tell me when, for example: remind me in 10 minutes to call mum – or: remind me at 17:30 to leave."));
        }
    }
    // Schalter
    let toggles: [(&[&str], &str); 6] = [
        (
            &[
                "taschenlampe",
                "lampe",
                "licht",
                "flashlight",
                "torch",
                "كشاف",
                "مصباح",
                "ضوء",
                "الفلاش",
            ],
            "flashlight",
        ),
        (&["wlan", "wifi", "wi-fi", "واي فاي", "وايفاي"], "wifi"),
        (&["bluetooth", "بلوتوث"], "bluetooth"),
        (
            &["flugmodus", "flugzeug", "airplane", "flight mode", "طيران"],
            "airplane",
        ),
        (
            &["mobile daten", "mobile data", "datenverbindung", "بيانات"],
            "mobile_data",
        ),
        (&["vpn"], "vpn"),
    ];
    for (words, name) in toggles {
        if has(t, words) {
            return Some(call("toggle", json!({"name": name, "on": on_off(t)})));
        }
    }
    // Einstellungen mit Wert
    if has(t, &["helligkeit", "brightness", "سطوع"])
        || has(t, &[" heller", " dunkler", "brighter", "dimmer"])
    {
        let v = number(t).unwrap_or(if has(t, &["max", "voll", "heller", "brighter", "أقصى"]) {
            100.0
        } else if has(t, &["min", "dunkler", "dimmer", "أدنى"]) {
            15.0
        } else {
            60.0
        });
        return Some(call(
            "set_setting",
            json!({"name": "brightness", "value": format!("{}", v.clamp(10.0, 100.0) as u32)}),
        ));
    }
    if has(t, &["lautstärke", "volume", "الصوت", "صوت"])
        || has(t, &["stumm", "mute", "leise", "صامت", "كتم"])
    {
        let v = number(t).unwrap_or(if has(t, &["stumm", "mute", "صامت", "كتم"]) {
            0.0
        } else if has(t, &["leise"]) {
            25.0
        } else if has(t, &["laut", "max"]) {
            100.0
        } else {
            60.0
        });
        return Some(call(
            "set_setting",
            json!({"name": "volume", "value": format!("{}", v.clamp(0.0, 100.0) as u32)}),
        ));
    }
    if has(
        t,
        &[
            "dunkelmodus",
            "dunkler modus",
            "dark mode",
            "nachtmodus",
            "الوضع الداكن",
            "الوضع الليلي",
        ],
    ) || has(t, &["heller modus", "light mode", "الوضع الفاتح"])
    {
        let dark = !has(t, &["heller modus", "light mode", "الوضع الفاتح"]) && on_off(t);
        return Some(call(
            "set_setting",
            json!({"name": "dark", "value": if dark { "true" } else { "false" }}),
        ));
    }
    if has(t, &["sprache", "language", "اللغة", "لغة"]) {
        let l = if has(t, &["deutsch", "german", "ألمان"]) {
            Some("de")
        } else if has(t, &["englisch", "english", "إنجليز", "انجليز"]) {
            Some("en")
        } else if has(t, &["arabisch", "arabic", "عربي"]) {
            Some("ar")
        } else {
            None
        };
        if let Some(l) = l {
            return Some(call("set_setting", json!({"name": "language", "value": l})));
        }
    }
    // Scanner / Foto
    if has(t, &["scan", "dokument", "document", "مسح", "مستند"]) {
        return Some(Intent::Scan);
    }
    if has(
        t,
        &[
            "foto",
            "photo",
            "selfie",
            "bild mach",
            "picture",
            "صورة",
            "صور لي",
        ],
    ) && !has(t, &["galerie", "gallery", "zeig", "show", "معرض"])
    {
        return Some(call("take_photo", json!({})));
    }
    // Zeitgeber / Wecker
    if has(t, &["timer", "stoppuhr", "countdown", "مؤقت", "تايمر"]) {
        if let Some(n) = number(t) {
            let min = if has(t, &["stunde", "hour", "ساعة"]) {
                n * 60.0
            } else if has(t, &["sekunde", "second", "ثانية"]) {
                (n / 60.0).max(1.0)
            } else {
                n
            };
            return Some(call("start_timer", json!({"minutes": min as u32})));
        }
    }
    if has(
        t,
        &[
            "wecker",
            "alarm",
            "weck mich",
            "wake me",
            "منبه",
            "أيقظني",
            "ايقظني",
        ],
    ) {
        let digits: String = t
            .chars()
            .map(|c| {
                if c.is_ascii_digit() || c == ':' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let tok = digits.split_whitespace().next().unwrap_or("");
        let (h, m) = match tok.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok(), m.parse::<u32>().unwrap_or(0)),
            None => (tok.parse::<u32>().ok(), 0),
        };
        if let Some(h) = h.filter(|h| *h < 24) {
            return Some(call("set_alarm", json!({"hour": h, "minute": m.min(59)})));
        }
    }
    // Notiz: „Notiz: Milch kaufen" / „note buy milk"
    for key in ["notiz", "notiere", "note", "merke", "ملاحظة", "دوّن", "دون"] {
        if let Some(i) = t.find(key) {
            let rest = raw[raw
                .char_indices()
                .nth(t[..i].chars().count().saturating_sub(1) + key.chars().count())
                .map(|x| x.0)
                .unwrap_or(raw.len())..]
                .trim_start_matches([':', ' ', ',', 'r', 'e'])
                .trim();
            if rest.chars().count() >= 2 {
                return Some(call("add_note", json!({"text": rest})));
            }
        }
    }
    // Auskunft
    if has(
        t,
        &[
            "wie spät",
            "uhrzeit",
            "what time",
            "datum",
            "date",
            "كم الساعة",
            "التاريخ",
            "الوقت",
        ],
    ) {
        return Some(Intent::Time);
    }
    if has(
        t,
        &[
            "status",
            "akku",
            "batterie",
            "battery",
            "speicherplatz",
            "arbeitsspeicher",
            "بطارية",
            "الحالة",
            "wie geht es dem",
        ],
    ) {
        return Some(call("get_status", json!({})));
    }
    // App öffnen
    let wants_open = has(
        t,
        &[
            "öffne",
            "open",
            "starte",
            "start ",
            "zeig",
            "show",
            "geh zu",
            "go to",
            "افتح",
            "اذهب إلى",
            "شغل",
        ],
    );
    for (words, name) in APPS {
        if has(t, words) && (wants_open || t.split_whitespace().count() <= 2) {
            return Some(call("open_app", json!({"name": name})));
        }
    }
    if has(
        t,
        &[
            "stell dich vor",
            "wer bist du",
            "who are you",
            "introduce yourself",
            "rundgang",
            "führung",
            "tour",
            "من أنت",
            "من انت",
            "عرف بنفسك",
            "عرّف بنفسك",
            "جولة",
        ],
    ) {
        return Some(Intent::Tour);
    }
    if has(
        t,
        &[
            "hilfe",
            "help",
            "was kannst du",
            "what can you",
            "مساعدة",
            "ماذا تستطيع",
            "ماذا يمكنك",
        ],
    ) {
        return Some(Intent::Say("I can: check links, switch flashlight, Wi-Fi, Bluetooth and flight mode, set brightness and volume, open apps, scan documents, take photos, set timers and alarms, save notes and show the device status."));
    }
    if has(
        t,
        &[
            " hallo",
            " hi ",
            " hey",
            "hello",
            "guten ",
            "مرحبا",
            "السلام",
            "اهلا",
            "أهلا",
        ],
    ) {
        return Some(Intent::Say(
            "Hello! Tell me what to do, or give me a link to check.",
        ));
    }
    if has(t, &["danke", "thanks", "thank you", "شكرا", "شكراً"]) {
        return Some(Intent::Say("You're welcome."));
    }
    None
}

/// Zeitpunkt in Unix-Sekunden: Uhrzeit heute, falls noch nicht vorbei, sonst morgen.
pub fn due(when: When, now: i64, tz_min: i32) -> i64 {
    match when {
        When::In(s) => now + s as i64,
        When::At(h, m) => {
            let mut d = crate::sys::from_unix(now, tz_min);
            d.h = h as i64;
            d.mi = m as i64;
            d.s = 0;
            let t = crate::sys::to_unix(&d, tz_min);
            if t <= now {
                t + 86400
            } else {
                t
            }
        }
    }
}

/// „erinnere mich in 10 Minuten an X" / „… um 17:30 an X" / „remind me at 9 to X" / „ذكرني بعد ٥ دقائق X"
fn remind(raw: &str) -> Option<(When, String)> {
    let norm: String = raw
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            _ => c,
        })
        .collect();
    let words: Vec<&str> = norm.split_whitespace().collect();
    let low: Vec<String> = words
        .iter()
        .map(|w| {
            w.to_lowercase()
                .trim_matches(|c: char| matches!(c, ',' | '.' | '!' | '?'))
                .to_string()
        })
        .collect();
    let mut when = None;
    let mut skip = vec![false; words.len()];
    for i in 0..words.len() {
        let w = low[i].as_str();
        // Uhrzeit: "17:30", "17.30", "um 9", "at 9", "الساعة 5"
        let clock = |tok: &str| -> Option<(u32, u32)> {
            let tok = tok.trim_end_matches("uhr");
            let (h, m) = match tok.split_once([':', '.']) {
                Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
                None => (tok.parse::<u32>().ok()?, 0),
            };
            (h < 24 && m < 60).then_some((h, m))
        };
        if matches!(w, "um" | "at" | "الساعة" | "عند") && i + 1 < words.len() {
            if let Some((h, m)) = clock(&low[i + 1]) {
                when = Some(When::At(h, m));
                skip[i] = true;
                skip[i + 1] = true;
                if low.get(i + 2).is_some_and(|x| x == "uhr") {
                    skip[i + 2] = true;
                }
                break;
            }
        }
        if w.contains(':') {
            if let Some((h, m)) = clock(w) {
                when = Some(When::At(h, m));
                skip[i] = true;
                break;
            }
        }
        // Dauer: "in 10 minuten", "in 2 hours", "بعد 5 دقائق"
        if matches!(w, "in" | "nach" | "بعد") && i + 2 < words.len() {
            if let Ok(n) = low[i + 1].parse::<u64>() {
                let u = low[i + 2].as_str();
                let secs = if u.starts_with("sek")
                    || u.starts_with("sec")
                    || u.starts_with("ثان")
                    || u.starts_with("ثوان")
                {
                    n
                } else if u.starts_with("min") || u.starts_with("دقيق") || u.starts_with("دقائق")
                {
                    n * 60
                } else if u.starts_with("stund")
                    || u.starts_with("hour")
                    || u == "h"
                    || u.starts_with("ساع")
                {
                    n * 3600
                } else {
                    continue;
                };
                when = Some(When::In(secs.max(1)));
                skip[i] = true;
                skip[i + 1] = true;
                skip[i + 2] = true;
                break;
            }
        }
    }
    let when = when?;
    // Füllwörter am Anfang entfernen, der Rest ist der Text der Erinnerung
    const LEAD: [&str; 26] = [
        "erinnere",
        "erinner",
        "erinnerung",
        "mich",
        "bitte",
        "daran",
        "an",
        "ans",
        "dass",
        "remind",
        "me",
        "to",
        "that",
        "please",
        "about",
        "termin",
        "appointment",
        "aufgabe",
        "task",
        "ذكرني",
        "ذكّرني",
        "موعد",
        "مهمة",
        "ب",
        "أن",
        "ان",
    ];
    let rest: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip[*i])
        .map(|(_, w)| *w)
        .collect();
    let mut a = 0;
    while a < rest.len()
        && LEAD.contains(
            &rest[a]
                .to_lowercase()
                .trim_matches(|c: char| matches!(c, ',' | ':' | '.'))
                .to_string()
                .as_str(),
        )
    {
        a += 1;
    }
    let what = rest[a..]
        .join(" ")
        .trim_matches(|c: char| matches!(c, ',' | ':' | '.' | ' '))
        .to_string();
    Some((
        when,
        if what.is_empty() {
            raw.trim().to_string()
        } else {
            what
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(text: &str) -> (String, String) {
        match understand(text) {
            Some(Intent::Tool(c)) => (c.tool.clone(), c.args.to_string()),
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn schalter_in_drei_sprachen() {
        assert_eq!(
            tool("Taschenlampe an"),
            ("toggle".into(), r#"{"name":"flashlight","on":true}"#.into())
        );
        assert_eq!(
            tool("mach das Licht wieder aus"),
            (
                "toggle".into(),
                r#"{"name":"flashlight","on":false}"#.into()
            )
        );
        assert_eq!(
            tool("turn off wifi"),
            ("toggle".into(), r#"{"name":"wifi","on":false}"#.into())
        );
        assert_eq!(
            tool("شغل الكشاف"),
            ("toggle".into(), r#"{"name":"flashlight","on":true}"#.into())
        );
        assert_eq!(
            tool("اطفئ البلوتوث"),
            ("toggle".into(), r#"{"name":"bluetooth","on":false}"#.into())
        );
        assert_eq!(
            tool("Flugmodus ein"),
            ("toggle".into(), r#"{"name":"airplane","on":true}"#.into())
        );
    }

    #[test]
    fn werte_und_apps() {
        assert_eq!(
            tool("Helligkeit 40"),
            (
                "set_setting".into(),
                r#"{"name":"brightness","value":"40"}"#.into()
            )
        );
        assert_eq!(
            tool("السطوع ٧٠"),
            (
                "set_setting".into(),
                r#"{"name":"brightness","value":"70"}"#.into()
            )
        );
        assert_eq!(
            tool("stumm"),
            (
                "set_setting".into(),
                r#"{"name":"volume","value":"0"}"#.into()
            )
        );
        assert_eq!(
            tool("öffne die Kamera"),
            ("open_app".into(), r#"{"name":"camera"}"#.into())
        );
        assert_eq!(
            tool("افتح الإعدادات"),
            ("open_app".into(), r#"{"name":"settings"}"#.into())
        );
        assert_eq!(
            tool("Timer 5 Minuten"),
            ("start_timer".into(), r#"{"minutes":5}"#.into())
        );
        assert_eq!(
            tool("Wecker um 7:30"),
            ("set_alarm".into(), r#"{"hour":7,"minute":30}"#.into())
        );
        assert_eq!(
            tool("Notiz: Milch kaufen"),
            ("add_note".into(), r#"{"text":"Milch kaufen"}"#.into())
        );
        assert_eq!(tool("Akku?"), ("get_status".into(), "{}".into()));
    }

    #[test]
    fn erinnerungen() {
        assert_eq!(
            understand("Erinnere mich in 10 Minuten an Mama anrufen"),
            Some(Intent::Remind(When::In(600), "Mama anrufen".into()))
        );
        assert_eq!(
            understand("erinnere mich um 17:30 an den Zahnarzt"),
            Some(Intent::Remind(When::At(17, 30), "den Zahnarzt".into()))
        );
        assert_eq!(
            understand("remind me at 9 to take my pills"),
            Some(Intent::Remind(When::At(9, 0), "take my pills".into()))
        );
        assert_eq!(
            understand("remind me in 2 hours to leave"),
            Some(Intent::Remind(When::In(7200), "leave".into()))
        );
        assert_eq!(
            understand("Termin um 14 Uhr Besprechung"),
            Some(Intent::Remind(When::At(14, 0), "Besprechung".into()))
        );
        assert_eq!(
            understand("ذكرني بعد ٥ دقائق الاتصال بأحمد"),
            Some(Intent::Remind(When::In(300), "الاتصال بأحمد".into()))
        );
        assert!(matches!(
            understand("erinnere mich an Brot"),
            Some(Intent::Say(_))
        ));
        assert_eq!(understand("meine Erinnerungen"), Some(Intent::Reminders));
        assert_eq!(understand("Wer bist du?"), Some(Intent::Tour));
        assert_eq!(understand("Sprache einrichten"), Some(Intent::VoiceSetup));
        assert_eq!(
            understand("Lerne Sprachbefehl Taschenlampe an"),
            Some(Intent::VoiceTeach("Taschenlampe an".into()))
        );
        assert_eq!(
            understand("geheimes Wort festlegen"),
            Some(Intent::VoiceSecret)
        );
        assert_eq!(understand("من أنت"), Some(Intent::Tour));
        assert_eq!(
            understand("lösche alle Erinnerungen"),
            Some(Intent::ForgetReminders)
        );
        assert!(!matches!(
            understand("öffne terminal"),
            Some(Intent::Remind(..)) | Some(Intent::Say(_))
        ));
        // 12:00 UTC, Zone +60: 13:00 Ortszeit
        let now = 1_800_000_000 - 1_800_000_000 % 86400 + 12 * 3600;
        assert_eq!(due(When::In(90), now, 60), now + 90);
        assert_eq!(due(When::At(17, 30), now, 60), now + 4 * 3600 + 1800);
        assert_eq!(due(When::At(9, 0), now, 60), now + 20 * 3600);
    }

    #[test]
    fn sonderfaelle() {
        assert_eq!(
            understand("ist bit.ly/abc sicher?"),
            Some(Intent::Link("bit.ly/abc".into()))
        );
        assert_eq!(understand("Dokument scannen"), Some(Intent::Scan));
        assert_eq!(understand("wie spät ist es"), Some(Intent::Time));
        assert_eq!(understand("geh weg"), Some(Intent::Away));
        assert!(matches!(understand("was kannst du"), Some(Intent::Say(_))));
        assert_eq!(understand("erkläre mir die Relativitätstheorie"), None);
    }
}
