//! KM-Code-Craft-Apps: Katalog und Installation.
//!
//! Eine App ist ein .deb-Paket für den Ubuntu-Bereich. Quelle ist das neueste GitHub-Release
//! des Projekts (bei privaten Projekten mit Zugangs-Token) oder eine direkte Adresse.
//! Bringt das Paket Dienstdateien unter /usr/share/ultar/services/ mit, werden sie nach
//! /data/ultar/services übernommen und gestartet.
//! Katalog: /etc/ultar/apps.toml, ergänzt/überschrieben durch /data/ultar/apps.toml.

use crate::sys::{self, Bg, Msg, COMPAT};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Name des .deb-Pakets (für "installiert?")
    pub package: String,
    /// GitHub-Projekt "besitzer/name"; installiert wird das neueste Release
    #[serde(default)]
    pub github: String,
    /// Direkte Adresse einer .deb-Datei (hat Vorrang vor github)
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize, Default)]
struct Catalog {
    #[serde(default)]
    app: Vec<App>,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct AppsCfg {
    /// Zugangs-Token für private GitHub-Projekte
    pub github_token: String,
}

pub fn catalog() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for p in ["/etc/ultar/apps.toml".to_string(), format!("{}/apps.toml", sys::data_dir())] {
        if let Some(c) = std::fs::read_to_string(p).ok().and_then(|s| toml::from_str::<Catalog>(&s).ok()) {
            for a in c.app {
                out.retain(|o| o.id != a.id);
                out.push(a);
            }
        }
    }
    out
}

fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        a => a,
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// curl im Ubuntu-Bereich ausführen; Optionen über die Standardeingabe, damit Token nicht in der Prozessliste stehen.
pub fn curl(config: &str, extra: &[&str]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut a = vec!["run", "--", "/usr/bin/curl", "-sS", "-L", "--max-time", "300", "-K", "-"];
    a.extend_from_slice(extra);
    let mut ch = Command::new(COMPAT).args(&a).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    ch.stdin.take().unwrap().write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    let out = ch.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

pub fn curl_ready() -> bool {
    std::path::Path::new("/compat/usr/bin/curl").exists()
}

/// App im Hintergrund installieren; Fortschritt als JobLine, Ende als JobDone.
pub fn install(bg: &Bg, app: App, token: String) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let say = |s: String| bg.send(Msg::JobLine(s));
        let fail = |s: String| {
            bg.send(Msg::JobLine(s));
            bg.send(Msg::JobDone(1));
        };
        if !curl_ready() {
            say("Installing download tool (curl) …".into());
            bg.stream(COMPAT, &args(&["apt", "update"]), Msg::JobLine);
            if bg.stream(COMPAT, &args(&["apt", "install", "--no-install-recommends", "curl", "ca-certificates"]), Msg::JobLine) != 0 {
                return fail("curl could not be installed.".into());
            }
        }
        let auth = if token.is_empty() { String::new() } else { format!("header = \"Authorization: Bearer {token}\"\n") };
        // 1. Adresse der Paketdatei bestimmen
        let (url, accept) = if !app.url.is_empty() {
            (app.url.clone(), String::new())
        } else if !app.github.is_empty() {
            say(format!("Looking up latest release of {} …", app.github));
            let cfg = format!("url = \"https://api.github.com/repos/{}/releases/latest\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"User-Agent: ultar-os\"\n{auth}", app.github);
            let body = match curl(&cfg, &[]) {
                Ok(b) => b,
                Err(e) => return fail(format!("GitHub not reachable: {e}")),
            };
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let suffix = format!("_{}.deb", arch());
            let asset = v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.ends_with(&suffix) || n.ends_with("_all.deb"))));
            match asset.and_then(|a| a["url"].as_str()) {
                Some(u) => {
                    say(format!("Release {}: {}", v["tag_name"].as_str().unwrap_or("?"), asset.unwrap()["name"].as_str().unwrap_or("")));
                    (u.to_string(), "header = \"Accept: application/octet-stream\"\nheader = \"User-Agent: ultar-os\"\n".to_string())
                }
                None => {
                    let msg = v["message"].as_str().unwrap_or("no release with a matching .deb file");
                    return fail(format!("{}: {msg} (needed: *{suffix}). For private projects enter a GitHub token.", app.github));
                }
            }
        } else {
            return fail("Catalog entry has neither url nor github.".into());
        };
        // 2. Herunterladen (/tmp ist im Ubuntu-Bereich dasselbe Verzeichnis)
        let file = format!("/tmp/ultar-app-{}.deb", app.id);
        say(format!("Downloading {url} …"));
        let use_auth = if url.starts_with("https://api.github.com/") { auth.as_str() } else { "" };
        let cfg = format!("url = \"{url}\"\noutput = \"{file}\"\nfail\n{accept}{use_auth}");
        if let Err(e) = curl(&cfg, &[]) {
            return fail(format!("Download failed: {e}"));
        }
        // 3. Installieren, fehlende Abhängigkeiten nachziehen
        let mut code = bg.stream(COMPAT, &args(&["dpkg", "-i", &file]), Msg::JobLine);
        if code != 0 {
            say("Resolving dependencies …".into());
            bg.stream(COMPAT, &args(&["apt", "update"]), Msg::JobLine);
            code = bg.stream(COMPAT, &args(&["apt", "-f", "install"]), Msg::JobLine);
        }
        let _ = std::fs::remove_file(&file);
        if code != 0 {
            return fail(format!("Installation failed (code {code})."));
        }
        // 4. Dienste der App übernehmen und starten
        activate_services(&bg);
        say(format!("{} installed.", app.name));
        bg.send(Msg::AppsChanged);
        bg.send(Msg::JobDone(0));
    });
}

fn activate_services(bg: &Bg) {
    let src = "/compat/usr/share/ultar/services";
    let dst = format!("{}/services", sys::data_dir());
    let Ok(rd) = std::fs::read_dir(src) else { return };
    let _ = std::fs::create_dir_all(&dst);
    let mut names = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".toml") && std::fs::copy(e.path(), format!("{dst}/{name}")).is_ok() {
            names.push(name.trim_end_matches(".toml").to_string());
        }
    }
    // reload nimmt neue Dienste auf und startet sie (sofern nicht als manuell markiert)
    sys::ctl("reload");
    for n in names {
        bg.send(Msg::JobLine(format!("Service {n} registered.")));
    }
}

pub fn remove(bg: &Bg, app: App) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        // Dienste der App stoppen und abmelden
        if let Ok(out) = std::process::Command::new(COMPAT).args(["run", "--", "/usr/bin/dpkg", "-L", &app.package]).output() {
            for l in String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.starts_with("/usr/share/ultar/services/") && l.ends_with(".toml")) {
                let n = l.rsplit('/').next().unwrap_or("").trim_end_matches(".toml").to_string();
                sys::ctl(&format!("stop {n}"));
                let _ = std::fs::remove_file(format!("{}/services/{n}.toml", sys::data_dir()));
            }
        }
        let code = bg.stream(COMPAT, &args(&["apt", "remove", &app.package]), Msg::JobLine);
        bg.send(Msg::AppsChanged);
        bg.send(Msg::JobDone(code));
    });
}
