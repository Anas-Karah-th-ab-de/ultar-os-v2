//! Sprache der Befehlszeilen-Werkzeuge: Deutsch (Quelle), Arabisch, Englisch.
//!
//! Die Werkzeuge schreiben weiter Deutsch; `println!`/`eprintln!` aus diesem Crate übersetzen die Ausgabe
//! anhand der Tabellen (längste Wendung zuerst). Einbinden: `use ultar_sprache::{println, eprintln};`
//!
//! Sprache: `ULTAR_LANG`, sonst `LC_ALL`/`LC_MESSAGES`/`LANG` (ar…, en…, de…), sonst die Spracheinstellung der
//! Oberfläche (`/data/ultar/settings.toml`, `lang = "ar"`), sonst `/etc/ultar/sprache`, sonst Deutsch.

pub mod tabellen;

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sprache {
    De,
    Ar,
    En,
}

fn aus_text(s: &str) -> Option<Sprache> {
    let s = s.trim().trim_matches('"').to_ascii_lowercase();
    if s.starts_with("ar") {
        Some(Sprache::Ar)
    } else if s.starts_with("en") {
        Some(Sprache::En)
    } else if s.starts_with("de") {
        Some(Sprache::De)
    } else {
        None
    }
}

/// Sprache ermitteln (ohne Zwischenspeicher – für Tests und Wechsel zur Laufzeit)
pub fn ermitteln() -> Sprache {
    for k in ["ULTAR_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(s) = std::env::var(k).ok().as_deref().and_then(aus_text) {
            return s;
        }
    }
    let data = std::env::var("ULTAR_DATA").unwrap_or_else(|_| "/data/ultar".into());
    if let Ok(t) = std::fs::read_to_string(format!("{data}/settings.toml")) {
        for l in t.lines() {
            if let Some(v) = l
                .trim()
                .strip_prefix("lang")
                .map(str::trim)
                .and_then(|r| r.strip_prefix('='))
            {
                if let Some(s) = aus_text(v) {
                    return s;
                }
            }
        }
    }
    std::fs::read_to_string("/etc/ultar/sprache")
        .ok()
        .as_deref()
        .and_then(aus_text)
        .unwrap_or(Sprache::De)
}

pub fn sprache() -> Sprache {
    static S: OnceLock<Sprache> = OnceLock::new();
    *S.get_or_init(ermitteln)
}

/// Kürzel für Umgebungsvariablen (LANG für Kindprozesse)
pub fn kuerzel() -> &'static str {
    match sprache() {
        Sprache::Ar => "ar",
        Sprache::En => "en",
        Sprache::De => "de",
    }
}

fn tabelle(s: Sprache) -> &'static [(&'static str, &'static str)] {
    static AR: OnceLock<Vec<(&str, &str)>> = OnceLock::new();
    static EN: OnceLock<Vec<(&str, &str)>> = OnceLock::new();
    let bauen = |ar: bool| {
        let mut v: Vec<(&str, &str)> = tabellen::ALLE
            .iter()
            .flat_map(|t| t.iter())
            .map(|(de, en, a)| (*de, if ar { *a } else { *en }))
            .filter(|(de, _)| !de.is_empty())
            .collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        v
    };
    match s {
        Sprache::Ar => AR.get_or_init(|| bauen(true)),
        Sprache::En => EN.get_or_init(|| bauen(false)),
        Sprache::De => &[],
    }
}

/// Buchstaben, Ziffern und was zu Namen/Pfaden/Feldern gehört (`netz.art`, `--uploads`, `/data`)
fn wortzeichen(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '=' | ':' | '<')
}

/// Text in eine Sprache übersetzen (Deutsch bleibt unverändert)
pub fn in_sprache(text: &str, s: Sprache) -> String {
    let t = tabelle(s);
    if t.is_empty() || text.is_empty() {
        return text.to_string();
    }
    // von links nach rechts: an jeder Stelle die längste passende Wendung ersetzen
    let mut aus = String::with_capacity(text.len() + text.len() / 2);
    let mut i = 0;
    let b = text.as_bytes();
    'aussen: while i < b.len() {
        // Wendungen nur an Wortanfängen (nicht mitten in einem Wort)
        let wortanfang = i == 0
            || !text[..i]
                .chars()
                .next_back()
                .map(wortzeichen)
                .unwrap_or(false);
        {
            for (de, ziel) in t {
                if (wortanfang
                    || !de
                        .chars()
                        .next()
                        .map(|c| c.is_alphanumeric())
                        .unwrap_or(false))
                    && text[i..].starts_with(de)
                {
                    let ende = i + de.len();
                    let wortende = ende >= b.len()
                        || !text[ende..]
                            .chars()
                            .next()
                            .map(wortzeichen)
                            .unwrap_or(false)
                        || !de
                            .chars()
                            .last()
                            .map(|c| c.is_alphanumeric())
                            .unwrap_or(false);
                    if wortende {
                        aus.push_str(ziel);
                        i = ende;
                        continue 'aussen;
                    }
                }
            }
        }
        let c = text[i..].chars().next().unwrap();
        aus.push(c);
        i += c.len_utf8();
    }
    aus
}

/// Text in die eingestellte Sprache übersetzen – JSON (für Programme) bleibt unverändert
pub fn tr(text: &str) -> String {
    let t = text.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        return text.to_string();
    }
    in_sprache(text, sprache())
}

/// Befehlswort (auch englisch/arabisch eingegeben) auf das deutsche Befehlswort abbilden
pub fn befehl<'a>(wort: &'a str) -> &'a str {
    tabellen::BEFEHLE
        .iter()
        .find(|(_, en, ar)| *en == wort || *ar == wort)
        .map(|(de, _, _)| *de)
        .unwrap_or(wort)
}

/// Argumente mit übersetzten Befehlswörtern (für `match` in den Werkzeugen): nur Befehl und Unterbefehl,
/// nie Namen dahinter oder etwas nach `--` (Befehle für andere Geräte bleiben unverändert)
pub fn argumente() -> Vec<String> {
    let mut v: Vec<String> = std::env::args().skip(1).collect();
    for i in 0..v.len().min(3) {
        if v[i] == "--" {
            break;
        }
        v[i] = befehl(&v[i]).to_string();
    }
    v
}

#[macro_export]
macro_rules! println {
    () => { ::std::println!() };
    ($($t:tt)*) => { ::std::println!("{}", $crate::tr(&::std::format!($($t)*))) };
}

#[macro_export]
macro_rules! eprintln {
    () => { ::std::eprintln!() };
    ($($t:tt)*) => { ::std::eprintln!("{}", $crate::tr(&::std::format!($($t)*))) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uebersetzt() {
        let z =
            "Zelle „a“ angelegt: Netz frei, Protokoll aus, Standort gesperrt, Geräteinfo verborgen";
        assert_eq!(
            in_sprache(z, Sprache::En),
            "Cell „a“ created: network open, log off, location blocked, device info hidden"
        );
        assert_eq!(
            in_sprache(z, Sprache::Ar),
            "الخلية „a“ أُنشئت: الشبكة مفتوح، السجل مطفأ، الموقع محجوب، معلومات الجهاز مخفي"
        );
        assert_eq!(in_sprache("Zellenwand", Sprache::En), "Zellenwand"); // nicht mitten im Wort
        assert_eq!(in_sprache("gestoppt", Sprache::De), "gestoppt");
        assert_eq!(
            in_sprache("zelle setze x protokoll.uploads true", Sprache::En),
            "zelle setze x protokoll.uploads true"
        );
        assert_eq!(befehl("list"), "liste");
        std::env::set_var("ULTAR_LANG", "ar");
        assert_eq!(tr("{\"netz\":\"frei\"}"), "{\"netz\":\"frei\"}");
        assert_eq!(befehl("قائمة"), "liste");
    }

    #[test]
    fn hilfe_vollstaendig() {
        // jede Hilfe hat eine Übersetzung in beiden Sprachen
        for t in tabellen::ALLE {
            for (de, en, ar) in t.iter() {
                assert!(!en.is_empty() && !ar.is_empty(), "fehlt: {de}");
            }
        }
    }
}
