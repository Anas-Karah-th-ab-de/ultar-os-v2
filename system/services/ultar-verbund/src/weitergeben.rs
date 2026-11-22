//! Weitergeben zwischen Geräten (lange drücken + wischen in der Oberfläche):
//! * **Datei/Bild:** wird übertragen, auf dem Ziel abgelegt und dort sofort geöffnet – weiterarbeiten am Ziel.
//! * **App/Bildschirm:** das Ziel öffnet die Fernansicht des Absenders – die App läuft weiter auf dem
//!   Ursprungsgerät, wird aber am Ziel gesehen und bedient (in beide Richtungen: Handy → PC und PC → Handy).

use std::path::{Path, PathBuf};

pub const MAX_DATEI: usize = 48 << 20;

/// Ablage für empfangene Dateien
pub fn empfang_ordner() -> PathBuf {
    if let Ok(d) = std::env::var("ULTAR_EMPFANG") {
        return d.into();
    }
    if Path::new("/data").is_dir() {
        return "/data/Empfangen".into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    Path::new(&home).join("Empfangen")
}

/// sicherer Dateiname (kein Pfad, keine Steuerzeichen) – verhindert ../-Angriffe
pub fn sicherer_name(n: &str) -> String {
    let basis = n.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = basis.chars().filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c)).take(120).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() {
        "datei".into()
    } else {
        s
    }
}

/// Datei ablegen ohne Vorhandenes zu überschreiben; liefert den Pfad
pub fn ablegen(name: &str, daten: &[u8]) -> Result<PathBuf, String> {
    if daten.len() > MAX_DATEI {
        return Err("Datei zu groß".into());
    }
    let dir = empfang_ordner();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = sicherer_name(name);
    let (stamm, endung) = match name.rsplit_once('.') {
        Some((a, b)) if !a.is_empty() => (a.to_string(), format!(".{b}")),
        _ => (name.clone(), String::new()),
    };
    let mut p = dir.join(&name);
    let mut i = 1;
    while p.exists() {
        i += 1;
        p = dir.join(format!("{stamm} ({i}){endung}"));
    }
    std::fs::write(&p, daten).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Hinweis in der Benachrichtigungsleiste (gleiches Format wie ultar-notify)
pub fn hinweis(titel: &str, text: &str) {
    use std::io::Write;
    let run = crate::bildschirm::run_dir();
    let _ = std::fs::create_dir_all(&run);
    let sauber = |s: &str| s.replace(['\t', '\n'], " ");
    // Format wie ultar-notify: Zeit, Dringlichkeit, Art, App, Titel, Text
    let zeile = format!("{}\tnormal\tapp\tverbund\t{}\t{}\n", crate::jetzt_ms() / 1000, sauber(titel), sauber(text));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(run.join("notify.log")) {
        let _ = f.write_all(zeile.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namen() {
        assert_eq!(sicherer_name("../../etc/passwd"), "passwd");
        assert_eq!(sicherer_name("C:\\x\\Bild 1.png"), "Bild 1.png");
        assert_eq!(sicherer_name("..."), "datei");
        assert_eq!(sicherer_name(".bashrc"), "bashrc");
    }
}
