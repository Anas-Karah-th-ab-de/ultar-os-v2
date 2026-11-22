//! Systemupdate aus den GitHub-Releases des Projekts.
//!
//! Ablauf: neuestes Release abfragen -> Kernsystem-Abbild (ultar-initramfs-<arch>.cpio.zst) und
//! SHA256SUMS laden -> Prüfsumme vergleichen -> unter /data/ultar/update bereitlegen.
//! Ist in der Konfiguration ein Boot-Ziel eingetragen (boot_target), wird das geprüfte Abbild dorthin
//! übernommen (vorherige Fassung bleibt als .alt erhalten) und gilt ab dem nächsten Start.
//! Konfiguration: /etc/ultar/update.toml, überschrieben durch /data/ultar/update.toml.

use crate::apps::curl;
use crate::sys::{self, Bg, Msg, COMPAT};
use serde::Deserialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UpdCfg {
    /// Adresse der GitHub-Schnittstelle
    pub api: String,
    /// Projekt "besitzer/name", dessen Releases die Updates liefern
    pub repo: String,
    /// Datei, aus der der Bootloader das Kernsystem lädt. Leer = Update wird nur bereitgelegt.
    pub boot_target: String,
}

impl Default for UpdCfg {
    fn default() -> Self {
        Self { api: "https://api.github.com".into(), repo: "Anas-Karah-th-ab-de/ultar-os".into(), boot_target: String::new() }
    }
}

pub fn cfg() -> UpdCfg {
    for p in [format!("{}/update.toml", sys::data_dir()), "/etc/ultar/update.toml".to_string()] {
        if let Some(c) = std::fs::read_to_string(p).ok().and_then(|s| toml::from_str(&s).ok()) {
            return c;
        }
    }
    UpdCfg::default()
}

#[derive(Clone, Debug, Default)]
pub struct Release {
    pub tag: String,
    pub newer: bool,
    pub image_name: String,
    pub image_url: String,
    pub sums_url: String,
    pub error: String,
}

fn ver(s: &str) -> Vec<u32> {
    s.trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect()
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    ver(tag) > ver(current)
}

fn auth(c: &UpdCfg, url: &str, token: &str) -> String {
    // Token nur an die GitHub-Schnittstelle selbst senden
    if !token.is_empty() && url.starts_with(&c.api) { format!("header = \"Authorization: Bearer {token}\"\n") } else { String::new() }
}

pub fn check(bg: &Bg, token: String) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let c = cfg();
        let mut r = Release::default();
        let url = format!("{}/repos/{}/releases/latest", c.api, c.repo);
        let conf = format!("url = \"{url}\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"User-Agent: ultar-os\"\n{}", auth(&c, &url, &token));
        match curl(&conf, &[]) {
            Err(e) => r.error = e,
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                match v["tag_name"].as_str() {
                    None => r.error = v["message"].as_str().unwrap_or("no release found").to_string(),
                    Some(tag) => {
                        r.tag = tag.to_string();
                        r.newer = is_newer(tag, VERSION);
                        let want = format!("ultar-initramfs-{}.cpio.zst", std::env::consts::ARCH);
                        for a in v["assets"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                            let (name, url) = (a["name"].as_str().unwrap_or(""), a["url"].as_str().unwrap_or(""));
                            if name == want {
                                r.image_name = name.into();
                                r.image_url = url.into();
                            } else if name == "SHA256SUMS" {
                                r.sums_url = url.into();
                            }
                        }
                    }
                }
            }
        }
        bg.send(Msg::Update(r));
    });
}

/// Laden, prüfen, bereitlegen und – wenn ein Boot-Ziel eingetragen ist – einspielen.
pub fn install(bg: &Bg, r: Release, token: String) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let c = cfg();
        let say = |s: String| bg.send(Msg::JobLine(s));
        let fail = |s: String| {
            bg.send(Msg::JobLine(s));
            bg.send(Msg::JobDone(1));
        };
        if r.image_url.is_empty() {
            return fail(format!("Release {} has no system image for this device ({}).", r.tag, std::env::consts::ARCH));
        }
        if r.sums_url.is_empty() {
            return fail("Release has no SHA256SUMS – update refused.".into());
        }
        let dir = format!("{}/update", sys::data_dir());
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return fail(format!("{dir}: {e}"));
        }
        let (img, sums) = (format!("{dir}/{}.neu", r.image_name), format!("{dir}/SHA256SUMS"));
        for (url, file) in [(&r.sums_url, &sums), (&r.image_url, &img)] {
            say(format!("Downloading {} …", file.rsplit('/').next().unwrap_or("")));
            let conf = format!("url = \"{url}\"\noutput = \"{file}\"\nfail\nheader = \"Accept: application/octet-stream\"\nheader = \"User-Agent: ultar-os\"\n{}", auth(&c, url, &token));
            if let Err(e) = curl(&conf, &[]) {
                return fail(format!("Download failed: {e}"));
            }
        }
        let want = std::fs::read_to_string(&sums).unwrap_or_default().lines().find_map(|l| {
            let mut p = l.split_whitespace();
            let (h, n) = (p.next()?, p.next()?);
            (n.trim_start_matches('*') == r.image_name).then(|| h.to_lowercase())
        });
        let have = sys::capture(COMPAT, &["run", "--", "/usr/bin/sha256sum", &img]).split_whitespace().next().unwrap_or("").to_lowercase();
        match want {
            Some(w) if !have.is_empty() && w == have => say("Checksum matches.".into()),
            Some(_) => {
                let _ = std::fs::remove_file(&img);
                return fail("Checksum does not match – update refused.".into());
            }
            None => {
                let _ = std::fs::remove_file(&img);
                return fail(format!("SHA256SUMS has no entry for {}.", r.image_name));
            }
        }
        let staged = format!("{dir}/{}", r.image_name);
        if let Err(e) = std::fs::rename(&img, &staged) {
            return fail(format!("{staged}: {e}"));
        }
        let _ = std::fs::write(format!("{dir}/version"), &r.tag);
        if c.boot_target.is_empty() {
            say(format!("Update {} is verified and stored in {dir}.", r.tag));
            say("No boot target configured (boot_target in update.toml) – it is not activated automatically.".into());
            return bg.send(Msg::JobDone(0));
        }
        // Einspielen: neben das Ziel kopieren, auf den Datenträger zwingen, alte Fassung aufheben, umbenennen
        let tmp = format!("{}.neu", c.boot_target);
        let res = std::fs::copy(&staged, &tmp)
            .and_then(|_| std::fs::File::open(&tmp)?.sync_all())
            .and_then(|_| {
                if std::path::Path::new(&c.boot_target).exists() {
                    std::fs::copy(&c.boot_target, format!("{}.alt", c.boot_target))?;
                }
                std::fs::rename(&tmp, &c.boot_target)
            });
        match res {
            Ok(()) => {
                nix::unistd::sync();
                say(format!("Update {} installed to {}. Previous version kept as .alt. Restart to use it.", r.tag, c.boot_target));
                bg.send(Msg::JobDone(0));
            }
            Err(e) => fail(format!("Could not write {}: {e}", c.boot_target)),
        }
    });
}
