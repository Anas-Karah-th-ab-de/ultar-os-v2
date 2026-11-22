//! Einstellungen des Fenstersystems und die App-Liste für das Startmenü.
//!
//! Quellen (spätere überschreiben frühere):
//! - `/etc/ultar/desktop.toml`, `/data/ultar/desktop.toml` (oder `ULTAR_WM_CONFIG`)
//! - installierte Programme mit `.desktop`-Datei unter `/usr/share/applications` und `~/.local/share/applications`
//!   (z. B. VS Code, Firefox, LibreOffice aus dem Ubuntu-Bereich)

use super::canvas::Img;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// "desktop", "tablet" oder "auto" (Tablet-Modus bei schmalen Bildschirmen)
    pub mode: String,
    /// Sprache: "ar", "en", "de" (sonst aus /data/ultar/settings.toml)
    pub lang: String,
    /// Terminal für Super+Enter
    pub terminal: String,
    /// Programme, die beim Start des Fenstersystems mitstarten
    pub autostart: Vec<String>,
    /// Eigene Einträge im Startmenü (stehen vorne)
    #[serde(rename = "app")]
    pub apps: Vec<AppCfg>,
    /// Nur diese Einträge (keine .desktop-Suche)
    pub only_config_apps: bool,
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct AppCfg {
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Clone)]
pub struct App {
    pub name: String,
    pub exec: String,
    pub icon: Option<Img>,
    pub keywords: String,
}

fn config_paths() -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("ULTAR_WM_CONFIG") {
        return vec![PathBuf::from(p)];
    }
    vec!["/etc/ultar/desktop.toml".into(), "/data/ultar/desktop.toml".into()]
}

pub fn load_config() -> Config {
    let mut cfg = Config { mode: "auto".into(), terminal: String::new(), ..Default::default() };
    for p in config_paths() {
        let Ok(s) = std::fs::read_to_string(&p) else { continue };
        match toml::from_str::<Config>(&s) {
            Ok(c) => {
                if !c.mode.is_empty() {
                    cfg.mode = c.mode;
                }
                if !c.lang.is_empty() {
                    cfg.lang = c.lang;
                }
                if !c.terminal.is_empty() {
                    cfg.terminal = c.terminal;
                }
                cfg.autostart.extend(c.autostart);
                cfg.apps.extend(c.apps);
                cfg.only_config_apps |= c.only_config_apps;
            }
            Err(e) => tracing::warn!("{}: {e}", p.display()),
        }
    }
    if cfg.lang.is_empty() {
        cfg.lang = system_lang();
    }
    if cfg.terminal.is_empty() {
        cfg.terminal = ["foot", "weston-terminal", "gnome-terminal", "xterm"]
            .iter()
            .find(|t| which(t).is_some())
            .unwrap_or(&"foot")
            .to_string();
    }
    cfg
}

/// Sprache der Ultar-Oberfläche übernehmen (gleiche Einstellung im ganzen System).
fn system_lang() -> String {
    for p in ["/data/ultar/settings.toml", "/compat-host/data/ultar/settings.toml"] {
        if let Ok(s) = std::fs::read_to_string(p) {
            for l in s.lines() {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("lang") {
                    let v = v.trim_start().trim_start_matches('=').trim().trim_matches('"');
                    if !v.is_empty() {
                        return v.to_string();
                    }
                }
            }
        }
    }
    match std::env::var("LANG").unwrap_or_default().get(..2) {
        Some("de") => "de".into(),
        Some("ar") => "ar".into(),
        _ => "en".into(),
    }
}

pub fn which(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        return Path::new(cmd).exists().then(|| PathBuf::from(cmd));
    }
    std::env::var("PATH")
        .unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into())
        .split(':')
        .map(|d| Path::new(d).join(cmd))
        .find(|p| p.exists())
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/usr/share/applications"), PathBuf::from("/usr/local/share/applications")];
    if let Ok(h) = std::env::var("HOME") {
        v.push(Path::new(&h).join(".local/share/applications"));
    }
    v.push("/var/lib/flatpak/exports/share/applications".into());
    // Von uapp installierte Apps (Windows-Programme, Android-Apps, AppImages): /data ist im Ubuntu-Bereich eingebunden
    v.push("/data/ultar/launchers".into());
    v
}

/// Eintrag einer .desktop-Datei in der passenden Sprache.
fn parse_desktop(text: &str, lang: &str) -> Option<(String, String, String, String)> {
    let mut in_entry = false;
    let (mut name, mut name_l, mut exec, mut icon, mut kw) = (None, None, None, String::new(), String::new());
    let mut hidden = false;
    let mut is_app = false;
    let mut ultar_id = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Name" => name = Some(v.to_string()),
            "Exec" => exec = Some(v.to_string()),
            "Icon" => icon = v.to_string(),
            "Keywords" | "GenericName" | "Comment" => {
                kw.push(' ');
                kw.push_str(v);
            }
            "NoDisplay" | "Hidden" if v == "true" => hidden = true,
            "Terminal" if v == "true" => hidden = true,
            "Type" => is_app = v == "Application",
            // von uapp installiert: Start über „uapp run“ (Wine-Bereich, Android-Paket, Umgebung)
            "X-Ultar-App" => ultar_id = v.to_string(),
            "OnlyShowIn" if !v.contains("Ultar") => hidden = true,
            _ => {
                if k == format!("Name[{lang}]") {
                    name_l = Some(v.to_string());
                }
            }
        }
    }
    if hidden || !is_app {
        return None;
    }
    let mut exec = exec?;
    if !ultar_id.is_empty() && ultar_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
        exec = format!("uapp run {ultar_id}");
    }
    // Feldcodes (%u, %F, …) entfernen – das Startmenü öffnet Programme ohne Datei
    let exec = exec
        .split_whitespace()
        .filter(|t| !(t.starts_with('%') && t.len() == 2))
        .collect::<Vec<_>>()
        .join(" ");
    Some((name_l.or(name)?, exec, icon, kw))
}

/// Symbol suchen (nur PNG; SVG-Symbole bekommen ein gezeichnetes Ersatzsymbol).
pub fn find_icon(name: &str) -> Option<Img> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Img::load(Path::new(name));
    }
    let mut roots = vec![PathBuf::from("/usr/share/icons/hicolor"), PathBuf::from("/usr/share/icons/Yaru")];
    if let Ok(h) = std::env::var("HOME") {
        roots.push(Path::new(&h).join(".local/share/icons/hicolor"));
    }
    for size in ["128x128", "96x96", "64x64", "256x256", "48x48", "512x512"] {
        for r in &roots {
            for sub in ["apps", "categories"] {
                let p = r.join(size).join(sub).join(format!("{name}.png"));
                if p.exists() {
                    if let Some(i) = Img::load(&p) {
                        return Some(i);
                    }
                }
            }
        }
    }
    for p in [format!("/usr/share/pixmaps/{name}.png"), format!("/usr/share/icons/{name}.png")] {
        if let Some(i) = Img::load(Path::new(&p)) {
            return Some(i);
        }
    }
    None
}

pub fn load_apps(cfg: &Config) -> Vec<App> {
    let mut out: Vec<App> = cfg
        .apps
        .iter()
        .map(|a| App { name: a.name.clone(), exec: a.exec.clone(), icon: find_icon(&a.icon), keywords: String::new() })
        .collect();
    if cfg.only_config_apps {
        return out;
    }
    let mut found: Vec<App> = Vec::new();
    for dir in desktop_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(&p) else { continue };
            if let Some((name, exec, icon, kw)) = parse_desktop(&t, &cfg.lang) {
                if out.iter().chain(found.iter()).any(|a| a.name == name) {
                    continue;
                }
                found.push(App { name, exec, icon: find_icon(&icon), keywords: kw.to_lowercase() });
            }
        }
    }
    found.sort_by_key(|a| a.name.to_lowercase());
    out.extend(found);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_entry() {
        let t = "[Desktop Entry]\nName=Visual Studio Code\nName[de]=VS Code\nExec=/usr/share/code/code --unity-launch %F\nIcon=vscode\nType=Application\n\n[Desktop Action new]\nName=Neues Fenster\nExec=code --new-window\n";
        let (n, e, i, _) = parse_desktop(t, "de").unwrap();
        assert_eq!(n, "VS Code");
        assert_eq!(e, "/usr/share/code/code --unity-launch");
        assert_eq!(i, "vscode");
        assert!(parse_desktop("[Desktop Entry]\nName=x\nExec=y\nType=Application\nNoDisplay=true\n", "en").is_none());
    }
}
