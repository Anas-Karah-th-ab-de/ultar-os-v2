//! Benachrichtigungs-Schnittstelle. Jede App/jeder Dienst meldet Ereignisse hierüber; die
//! Oberfläche (ultar-shell) liest sie, zeigt sie im Wisch-von-oben-Panel und alarmiert
//! (Ton/Vibration), auch wenn das Handy in der Tasche ist.
//!
//! Transport: eine Zeile je Benachrichtigung, angehängt an `/run/ultar/notify.log` (flüchtig).
//! Format (TAB-getrennt, in Feldern werden TAB/Zeilenumbruch durch Leerzeichen ersetzt):
//! ```text
//! <ts>\t<urgency>\t<kind>\t<app>\t<title>\t<body>
//! ```
//! urgency: low | normal | high   (high = z. B. eingehender Anruf → sofort alarmieren/aufwecken)
//! kind:    call | sms | message | system | app

pub const LOG: &str = "/run/ultar/notify.log";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Urgency {
    Low,
    Normal,
    High,
}

impl Urgency {
    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::High => "high",
        }
    }
    pub fn parse(s: &str) -> Urgency {
        match s {
            "high" => Urgency::High,
            "low" => Urgency::Low,
            _ => Urgency::Normal,
        }
    }
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Eine Benachrichtigung melden. Non-blocking, best-effort.
pub fn post(kind: &str, app: &str, title: &str, body: &str, urgency: Urgency) {
    use std::io::Write;
    let _ = std::fs::create_dir_all("/run/ultar");
    let line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        now(),
        urgency.as_str(),
        clean(kind),
        clean(app),
        clean(title),
        clean(body)
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Kurzform für einfache App-Hinweise.
pub fn info(app: &str, title: &str, body: &str) {
    post("app", app, title, body, Urgency::Normal);
}

/// Eine gelesene Benachrichtigung.
#[derive(Clone, Debug)]
pub struct Note {
    pub ts: i64,
    pub urgency: Urgency,
    pub kind: String,
    pub app: String,
    pub title: String,
    pub body: String,
}

/// Eine Zeile parsen.
pub fn parse_line(line: &str) -> Option<Note> {
    let f: Vec<&str> = line.splitn(6, '\t').collect();
    if f.len() < 6 {
        // Rückfall: ältere Zeilen "Titel\tText" oder nur "Titel"
        let (title, body) = line.split_once('\t').unwrap_or((line, ""));
        if title.trim().is_empty() {
            return None;
        }
        return Some(Note {
            ts: 0,
            urgency: Urgency::Normal,
            kind: "app".into(),
            app: String::new(),
            title: title.trim().to_string(),
            body: body.trim().to_string(),
        });
    }
    Some(Note {
        ts: f[0].parse().unwrap_or(0),
        urgency: Urgency::parse(f[1]),
        kind: f[2].to_string(),
        app: f[3].to_string(),
        title: f[4].to_string(),
        body: f[5].trim_end().to_string(),
    })
}
