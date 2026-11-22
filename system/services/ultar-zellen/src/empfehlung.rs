//! Vor einer Installation: gibt es das schon? Dann verknüpfen statt doppelt installieren.
//!
//! Erkannt werden Dienste und Programmfamilien (Browser, Datenbanken, Webserver, Laufzeiten …) in installierten Apps
//! (uapp), Zellen und Kiri-Containern. Beispiel: ein zweiter Browser „für Offline-Daten“ muss nicht neu installiert
//! werden – eine neue Zelle nutzt den vorhandenen mit eigenem Profil, eigenem Speicher und eigenem Netz.
//! Die Entscheidung bleibt beim Nutzer: „trotzdem doppelt installieren“ geht immer.

use serde::Serialize;

/// (Familie, Merkmale in Datei-/Paket-/Abbildnamen)
const FAMILIEN: &[(&str, &[&str])] = &[
    (
        "Browser",
        &[
            "firefox",
            "chrome",
            "chromium",
            "brave",
            "edge",
            "opera",
            "vivaldi",
            "librewolf",
            "tor-browser",
            "epiphany",
        ],
    ),
    ("PostgreSQL", &["postgres", "postgresql", "pgsql"]),
    ("MySQL/MariaDB", &["mysql", "mariadb"]),
    ("Redis", &["redis", "valkey", "keydb"]),
    ("MongoDB", &["mongo"]),
    (
        "Webserver",
        &["nginx", "caddy", "apache2", "httpd", "lighttpd", "traefik"],
    ),
    ("Python", &["python3", "python"]),
    ("Node.js", &["nodejs", "node"]),
    ("Java", &["openjdk", "temurin", "java", "jre", "jdk"]),
    ("Wine", &["wine"]),
    ("Office", &["libreoffice", "onlyoffice", "wps"]),
    ("Medienspieler", &["vlc", "mpv"]),
    ("Messenger", &["telegram", "signal", "whatsapp", "element"]),
    ("Code-Editor", &["code", "vscode", "vscodium", "sublime"]),
    (
        "KI-Modell-Dienst",
        &["ollama", "llama", "llamacpp", "localai"],
    ),
];

/// Familie eines Namens (Datei, Paket, Container-Abbild)
pub fn familie(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    // Pfad, Endung, Version, Registry abschneiden: „docker.io/library/postgres:16“ → „postgres“
    let n = n.rsplit('/').next().unwrap_or(&n);
    let n = n.split([':', '@']).next().unwrap_or(n);
    let woerter: Vec<&str> = n
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    FAMILIEN
        .iter()
        .find(|(_, m)| {
            m.iter().any(|x| {
                woerter
                    .iter()
                    .any(|w| w == x || (w.len() > x.len() && w.starts_with(x) && x.len() >= 5))
            })
        })
        .map(|(f, _)| *f)
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Vorhanden {
    /// "app" | "zelle" | "container"
    pub art: String,
    pub kennung: String,
    pub name: String,
    pub familie: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct Empfehlung {
    pub neu: String,
    pub familie: String,
    pub vorhanden: Vec<Vorhanden>,
    pub vorschlag: String,
}

/// installierte Apps (uapp), Zellen und Container einsammeln
pub fn bestand() -> Vec<Vorhanden> {
    let mut v = Vec::new();
    let data = std::env::var("ULTAR_DATA").unwrap_or_else(|_| "/data/ultar".into());
    for e in std::fs::read_dir(format!("{data}/apps.d"))
        .map(|d| d.flatten().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let Ok(s) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Ok(t) = toml::from_str::<toml::Value>(&s) else {
            continue;
        };
        let id = t
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let name = t
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&id)
            .to_string();
        let paket = t
            .get("package")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let quelle = t
            .get("source")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        if let Some(f) = familie(&name)
            .or_else(|| familie(&paket))
            .or_else(|| familie(&quelle))
        {
            v.push(Vorhanden {
                art: "app".into(),
                kennung: id,
                name,
                familie: f.into(),
            });
        }
    }
    for p in crate::profil::Profil::alle() {
        let ziel = if p.app.is_empty() {
            p.befehl.first().cloned().unwrap_or_default()
        } else {
            p.app.clone()
        };
        if let Some(f) = familie(&ziel).or_else(|| familie(&p.name)) {
            v.push(Vorhanden {
                art: "zelle".into(),
                kennung: p.name.clone(),
                name: p.name.clone(),
                familie: f.into(),
            });
        }
    }
    if let Ok(cs) = crate::kiri::Docker::standard().container() {
        for c in cs {
            if let Some(f) = familie(&c.abbild) {
                v.push(Vorhanden {
                    art: "container".into(),
                    kennung: c.id.chars().take(12).collect(),
                    name: c.name,
                    familie: f.into(),
                });
            }
        }
    }
    v
}

pub fn pruefen(neu: &str, bestand: &[Vorhanden]) -> Option<Empfehlung> {
    let f = familie(neu)?;
    let gleiche: Vec<Vorhanden> = bestand.iter().filter(|b| b.familie == f).cloned().collect();
    if gleiche.is_empty() {
        return None;
    }
    let erstes = &gleiche[0];
    let vorschlag = match erstes.art.as_str() {
        "app" => format!("„{}“ ist schon installiert. Statt doppelt: neue Zelle mit eigenem Profil, Speicher und Netz – `zelle neu <name> --app {}`.", erstes.name, erstes.kennung),
        "zelle" => format!("Die Zelle „{}“ hat das schon. Verknüpfen (gemeinsamer Ordner, gegenseitig erreichbar): `zelle verknuepfen <neu> {}`.", erstes.name, erstes.kennung),
        _ => format!("Der Container „{}“ läuft schon. Verknüpfen statt einen zweiten: `zelle verknuepfen-container <neu> {}`.", erstes.name, erstes.name),
    };
    Some(Empfehlung {
        neu: neu.into(),
        familie: f.into(),
        vorhanden: gleiche,
        vorschlag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn familien() {
        assert_eq!(familie("Firefox Setup 128.0.exe"), Some("Browser"));
        assert_eq!(
            familie("docker.io/library/postgres:16-alpine"),
            Some("PostgreSQL")
        );
        assert_eq!(familie("mariadb:11"), Some("MySQL/MariaDB"));
        assert_eq!(familie("brave-browser_1.2_amd64.deb"), Some("Browser"));
        assert_eq!(familie("taschenrechner.uap"), None);
        assert_eq!(
            familie("codecraft-notes.deb"),
            None,
            "„code“ nur als ganzes Wort"
        );
        let b = vec![Vorhanden {
            art: "app".into(),
            kennung: "firefox".into(),
            name: "Firefox".into(),
            familie: "Browser".into(),
        }];
        let e = pruefen("ChromeSetup.exe", &b).unwrap();
        assert_eq!(e.familie, "Browser");
        assert!(e.vorschlag.contains("--app firefox"));
        assert!(pruefen("redis:7", &b).is_none());
    }
}
