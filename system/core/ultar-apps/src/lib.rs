//! ultar-apps – ein Installer für alle App-Formate auf Ultar OS (Handy, Desktop/Laptop, Server).
//!
//! | Format                     | Weg                                                                 |
//! |----------------------------|---------------------------------------------------------------------|
//! | `.uap` (KM Code Craft)     | `ultar-pkg`: Signaturpflicht gegen eingebrannte Schlüssel           |
//! | `.deb`                     | Ubuntu-Bereich: `apt-get install` (löst Abhängigkeiten)             |
//! | `.apk`                     | PC: Waydroid im Ubuntu-Bereich · Handy: abgeschottete Android-Umgebung |
//! | `.exe`, `.msi`             | Wine im Ubuntu-Bereich, **eigener Wine-Bereich je App**, ohne Laufwerk Z: |
//! | AppImage                   | eigener Ordner, Start mit `--appimage-extract-and-run` (kein FUSE nötig) |
//! | `.flatpak`, `.flatpakref`  | Flatpak im Ubuntu-Bereich                                           |
//!
//! Alles Installierte steht in `/data/ultar/apps.d/<id>.toml`; Startmenü-Einträge für Oberfläche und
//! Desktop liegen in `/data/ultar/launchers/*.desktop`. Fremde (nicht signierte) Formate erlaubt die
//! Richtlinie `/etc/ultar/apps-policy.toml` (überschreibbar in `/data/ultar/apps-policy.toml`).
//!
//! Läuft `uapp` selbst im Ubuntu-Bereich (z. B. aus dem Desktop), merkt es das an `ULTAR_IN_COMPAT=1`
//! und ruft die Programme dort direkt auf, statt über `ultar-compat`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------- Format erkennen

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Uap,
    Deb,
    Apk,
    /// Android-Split-Paket (.xapk/.apks/.apkm)
    ApkSplit,
    Exe,
    Msi,
    AppImage,
    Flatpak,
    FlatpakRef,
    Rpm,
    Snap,
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Uap => "Ultar-Paket (.uap)",
            Format::Deb => "Ubuntu/Debian-Paket (.deb)",
            Format::Apk => "Android-App (.apk)",
            Format::ApkSplit => "Android-Split-Paket (.xapk/.apks)",
            Format::Exe => "Windows-Programm (.exe)",
            Format::Msi => "Windows-Installationspaket (.msi)",
            Format::AppImage => "AppImage",
            Format::Flatpak => "Flatpak-Paket",
            Format::FlatpakRef => "Flatpak-Verweis (.flatpakref)",
            Format::Rpm => "RPM-Paket",
            Format::Snap => "Snap-Paket",
            Format::Unknown => "unbekannt",
        }
    }
    /// Von KM Code Craft signiert und geprüft – alle anderen gelten als „fremd“.
    pub fn is_signed_native(self) -> bool {
        self == Format::Uap
    }
    pub fn supported(self) -> bool {
        !matches!(self, Format::Unknown | Format::Snap | Format::ApkSplit)
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Format an Inhalt (Magic) und Endung erkennen. Der Inhalt hat Vorrang: eine umbenannte Datei wird nicht
/// fälschlich als etwas anderes ausgeführt.
pub fn detect(p: &Path) -> std::io::Result<Format> {
    let mut f = std::fs::File::open(p)?;
    let mut head = vec![0u8; 4096];
    let n = f.read(&mut head)?;
    head.truncate(n);
    Ok(detect_bytes(&head, &ext_of(p), p))
}

fn detect_bytes(h: &[u8], ext: &str, p: &Path) -> Format {
    let starts = |m: &[u8]| h.len() >= m.len() && &h[..m.len()] == m;
    if starts(b"!<arch>\ndebian-binary") {
        return Format::Deb;
    }
    if starts(b"\x7fELF") {
        // AppImage Typ 1/2: "AI" + Version an Position 8
        if h.len() > 10 && &h[8..10] == b"AI" {
            return Format::AppImage;
        }
        return if ext == "appimage" {
            Format::AppImage
        } else {
            Format::Unknown
        };
    }
    if starts(b"MZ") {
        return Format::Exe;
    }
    if starts(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        // OLE-Verbunddatei: MSI (Word-Dokumente haben dasselbe Magic)
        return if ext == "msi" || ext == "msp" {
            Format::Msi
        } else {
            Format::Unknown
        };
    }
    if starts(&[0xED, 0xAB, 0xEE, 0xDB]) {
        return Format::Rpm;
    }
    if starts(b"hsqs") && ext == "snap" {
        return Format::Snap;
    }
    if starts(b"flatpak") || (ext == "flatpak" && starts(b"\x89")) {
        return Format::Flatpak;
    }
    if ext == "flatpakref"
        && h.windows(14)
            .any(|w| w == b"[Flatpak Ref]\n" || w == b"[Flatpak Ref]\r")
    {
        return Format::FlatpakRef;
    }
    if starts(b"PK\x03\x04") {
        if let Ok(f) = std::fs::File::open(p) {
            if let Ok(z) = zip::ZipArchive::new(f) {
                let names: Vec<String> = z.file_names().map(String::from).collect();
                if names.iter().any(|n| n == "AndroidManifest.xml") {
                    return Format::Apk;
                }
                if names.iter().any(|n| n.ends_with(".apk")) {
                    return Format::ApkSplit;
                }
            }
        }
        return Format::Unknown;
    }
    // .uap = tar mit ultar-manifest.toml (ustar-Magic an Position 257)
    if h.len() > 262
        && &h[257..262] == b"ustar"
        && (ext == "uap"
            || h.windows(ultar_pkg::M_MANIFEST.len())
                .any(|w| w == ultar_pkg::M_MANIFEST.as_bytes()))
    {
        return Format::Uap;
    }
    Format::Unknown
}

// ---------------------------------------------------------------- Pfade, Richtlinie

#[derive(Clone, Debug)]
pub struct Paths {
    /// Datenwurzel, Standard /data/ultar (Test: ULTAR_APPS_ROOT)
    pub data: PathBuf,
    /// Ubuntu-Bereich aus Sicht dieses Prozesses (/compat; im Ubuntu-Bereich selbst /)
    pub compat_root: PathBuf,
}

impl Paths {
    pub fn system() -> Paths {
        let data = std::env::var("ULTAR_APPS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| "/data/ultar".into());
        let compat_root = if in_compat() {
            "/".into()
        } else {
            std::env::var("ULTAR_COMPAT_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| "/compat".into())
        };
        Paths { data, compat_root }
    }
    pub fn registry(&self) -> PathBuf {
        self.data.join("apps.d")
    }
    pub fn launchers(&self) -> PathBuf {
        self.data.join("launchers")
    }
    pub fn wine(&self, id: &str) -> PathBuf {
        self.data.join("wine").join(id)
    }
    pub fn app_dir(&self, id: &str) -> PathBuf {
        self.data.join("apps").join(id)
    }
    pub fn incoming(&self) -> PathBuf {
        self.data.join("incoming")
    }
    /// Austauschordner für Windows-/Android-Apps (in Wine Laufwerk D:)
    pub fn share(&self) -> PathBuf {
        self.data.join("share")
    }
    pub fn android_pending(&self) -> PathBuf {
        self.data.join("android-share").join("pending")
    }
    fn compat_has(&self, bin: &str) -> bool {
        let rel = bin.trim_start_matches('/');
        self.compat_root.join(rel).exists()
    }
}

pub fn in_compat() -> bool {
    std::env::var("ULTAR_IN_COMPAT").is_ok_and(|v| v == "1")
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// Nicht von KM Code Craft signierte Formate (apk, exe, msi, AppImage, Flatpak, deb) zulassen
    pub allow_foreign: bool,
    /// Windows-Programme ohne Laufwerk Z: (= ganzes Dateisystem); nur D: = /data/ultar/share
    pub wine_isolate: bool,
    /// Einzelne Formate sperren, z. B. ["exe", "msi"]
    pub deny_formats: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            allow_foreign: true,
            wine_isolate: true,
            deny_formats: Vec::new(),
        }
    }
}

impl Policy {
    pub fn load(p: &Paths) -> Policy {
        let user = p.data.join("apps-policy.toml");
        let sys = std::env::var("ULTAR_APPS_POLICY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| "/etc/ultar/apps-policy.toml".into());
        for f in [user, sys] {
            if let Ok(s) = std::fs::read_to_string(&f) {
                if let Ok(pol) = toml::from_str(&s) {
                    return pol;
                }
            }
        }
        Policy::default()
    }
    fn format_key(f: Format) -> &'static str {
        match f {
            Format::Uap => "uap",
            Format::Deb => "deb",
            Format::Apk | Format::ApkSplit => "apk",
            Format::Exe => "exe",
            Format::Msi => "msi",
            Format::AppImage => "appimage",
            Format::Flatpak | Format::FlatpakRef => "flatpak",
            Format::Rpm => "rpm",
            Format::Snap => "snap",
            Format::Unknown => "unknown",
        }
    }
    pub fn check(&self, f: Format, force: bool) -> Result<(), String> {
        if self
            .deny_formats
            .iter()
            .any(|d| d.eq_ignore_ascii_case(Self::format_key(f)))
        {
            return Err(format!(
                "{} ist auf diesem Gerät gesperrt (Richtlinie)",
                f.label()
            ));
        }
        if !f.is_signed_native() && !self.allow_foreign && !force {
            return Err(format!("{} ist nicht von KM Code Craft signiert; fremde Formate sind gesperrt (Einstellungen → Apps oder --allow-foreign)", f.label()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Verzeichnis der Apps

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub format: Option<Format>,
    /// Ursprünglicher Dateiname
    pub source: String,
    pub sha256: String,
    /// Unix-Zeit der Installation
    pub installed: u64,
    /// "ok" | "pending" (wartet z. B. auf die Android-Umgebung)
    pub status: String,
    /// Paket-/App-Kennung im jeweiligen System (deb-Paket, Android-Paket, Flatpak-ID)
    pub package: String,
    /// Startbefehl (argv) – im Ubuntu-Bereich ausgeführt, außer bei `.uap` (nativ)
    pub run: Vec<String>,
    /// Umgebungsvariablen für den Start
    pub env: Vec<(String, String)>,
    /// Startmenü-Einträge (Dateinamen unter launchers/)
    pub launchers: Vec<String>,
    /// Hinweis für Nutzer (z. B. „nur ARM-Code: braucht Übersetzungsschicht“)
    pub note: String,
}

pub fn list(p: &Paths) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(p.registry())
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|s| toml::from_str(&s).ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

pub fn get(p: &Paths, id: &str) -> Option<Entry> {
    let s = std::fs::read_to_string(p.registry().join(format!("{}.toml", sanitize(id)))).ok()?;
    toml::from_str(&s).ok()
}

fn save(p: &Paths, e: &Entry) -> Result<(), String> {
    std::fs::create_dir_all(p.registry()).map_err(|e| e.to_string())?;
    let f = p.registry().join(format!("{}.toml", e.id));
    let tmp = f.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(e).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &f).map_err(|e| e.to_string())
}

pub fn sanitize(s: &str) -> String {
    let mut o: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while o.contains("--") {
        o = o.replace("--", "-");
    }
    let o = o.trim_matches(|c| c == '-' || c == '.').to_string();
    if o.is_empty() {
        "app".into()
    } else {
        o.chars().take(48).collect()
    }
}

fn unique_id(p: &Paths, base: &str) -> String {
    let base = sanitize(base);
    if get(p, &base).is_none() {
        return base;
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|c| get(p, c).is_none())
        .unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

// ---------------------------------------------------------------- Programme im Ubuntu-Bereich

fn compat_bin() -> String {
    std::env::var("ULTAR_COMPAT_BIN").unwrap_or_else(|_| "/usr/bin/ultar-compat".into())
}

/// Befehl im Ubuntu-Bereich (oder direkt, wenn wir schon dort laufen)
pub fn compat_cmd(argv: &[String], env: &[(String, String)]) -> Command {
    if in_compat() {
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        for (k, v) in env {
            c.env(k, v);
        }
        c
    } else {
        let mut c = Command::new(compat_bin());
        c.args(["run", "--", "/usr/bin/env"]);
        for (k, v) in env {
            c.arg(format!("{k}={v}"));
        }
        c.args(argv);
        c
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn run_ok(
    argv: &[String],
    env: &[(String, String)],
    log: &mut dyn FnMut(&str),
) -> Result<String, String> {
    log(&format!("$ {}", argv.join(" ")));
    let out = compat_cmd(argv, env)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    let se = String::from_utf8_lossy(&out.stderr).to_string();
    for l in so
        .lines()
        .chain(se.lines())
        .filter(|l| !l.trim().is_empty())
        .take(40)
    {
        log(&format!("  {l}"));
    }
    if out.status.success() {
        Ok(so)
    } else {
        Err(format!(
            "{} fehlgeschlagen ({}): {}",
            argv[0],
            out.status,
            se.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
        ))
    }
}

fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

// ---------------------------------------------------------------- Installation

#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// fremde Formate trotz Richtlinie zulassen (nur Verwalter)
    pub allow_foreign: bool,
    /// Windows-Installer ohne Rückfragen (MSI: /qn)
    pub silent: bool,
    /// Anzeigename statt des Dateinamens
    pub name: Option<String>,
}

pub fn install(
    p: &Paths,
    file: &Path,
    o: &Opts,
    log: &mut dyn FnMut(&str),
) -> Result<Entry, String> {
    let file = file
        .canonicalize()
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let fmt = detect(&file).map_err(|e| e.to_string())?;
    log(&format!("Format: {}", fmt.label()));
    if !fmt.supported() {
        return Err(match fmt {
            Format::Snap => "Snap-Pakete brauchen snapd mit systemd – auf Ultar nicht vorgesehen. Bitte die .deb-, Flatpak- oder AppImage-Fassung nehmen.".into(),
            Format::ApkSplit => "Android-Split-Pakete (.xapk/.apks) werden noch nicht unterstützt – bitte die einzelne .apk nehmen.".into(),
            _ => format!("{}: Format nicht erkannt", file.display()),
        });
    }
    Policy::load(p).check(fmt, o.allow_foreign)?;
    let sha = sha256_file(&file).map_err(|e| e.to_string())?;
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();
    let source = file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    // Kopie unter /data, damit der Ubuntu-Bereich (sieht /data, /media, /tmp) die Datei sicher erreicht
    std::fs::create_dir_all(p.incoming()).map_err(|e| e.to_string())?;
    let staged = p
        .incoming()
        .join(format!("{}.{}", &sha[..16], ext_of(&file)));
    if !staged.exists() {
        std::fs::copy(&file, &staged).map_err(|e| format!("Kopie: {e}"))?;
    }
    let mut e = Entry {
        name: o.name.clone().unwrap_or_else(|| pretty_name(&stem)),
        format: Some(fmt),
        source,
        sha256: sha,
        installed: now(),
        status: "ok".into(),
        ..Default::default()
    };
    let res = match fmt {
        Format::Uap => install_uap(p, &staged, &mut e, log),
        Format::Deb => install_deb(p, &staged, &mut e, log),
        Format::Rpm => install_rpm(p, &staged, &mut e, log),
        Format::Apk => install_apk(p, &staged, &stem, &mut e, log),
        Format::Exe | Format::Msi => install_wine(p, &staged, fmt, &stem, o, &mut e, log),
        Format::AppImage => install_appimage(p, &staged, &stem, &mut e, log),
        Format::Flatpak | Format::FlatpakRef => {
            install_flatpak(p, &staged, fmt, &stem, &mut e, log)
        }
        _ => unreachable!(),
    };
    let _ = std::fs::remove_file(&staged);
    res?;
    save(p, &e)?;
    log(&format!("Installiert: {} ({})", e.name, e.id));
    Ok(e)
}

fn pretty_name(stem: &str) -> String {
    let base = stem
        .split(['_', '-'])
        .filter(|w| {
            !w.is_empty()
                && !w.chars().next().unwrap().is_ascii_digit()
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "setup"
                        | "installer"
                        | "install"
                        | "x64"
                        | "x86"
                        | "amd64"
                        | "arm64"
                        | "win64"
                        | "win32"
                        | "release"
                )
        })
        .collect::<Vec<_>>()
        .join(" ");
    if base.is_empty() {
        stem.to_string()
    } else {
        base
    }
}

fn install_uap(
    _p: &Paths,
    f: &Path,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let trusted = std::env::var("ULTAR_TRUSTED").unwrap_or_else(|_| ultar_pkg::TRUSTED_DIR.into());
    let m = ultar_pkg::install(f, Path::new(&trusted))?;
    log("Signatur geprüft (Ed25519) und installiert");
    e.id = sanitize(&m.id);
    e.name = m.name.clone();
    e.package = m.id.clone();
    Ok(())
}

fn deb_field(f: &Path, field: &str, log: &mut dyn FnMut(&str)) -> Result<String, String> {
    Ok(run_ok(
        &s(&["dpkg-deb", "-f", &f.to_string_lossy(), field]),
        &[],
        log,
    )?
    .trim()
    .to_string())
}

fn install_deb(
    p: &Paths,
    f: &Path,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    if !p.compat_has("/usr/bin/apt-get") {
        return Err("Ubuntu-Bereich fehlt (/compat) – .deb-Pakete brauchen ihn".into());
    }
    let pkg = deb_field(f, "Package", log)?;
    let arch = deb_field(f, "Architecture", log).unwrap_or_default();
    let ours = if host_arch() == "aarch64" {
        "arm64"
    } else {
        "amd64"
    };
    if !arch.is_empty() && arch != "all" && arch != ours {
        return Err(format!("Paket ist für {arch}, dieses Gerät braucht {ours}"));
    }
    let env = vec![("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())];
    run_ok(
        &s(&[
            "apt-get",
            "install",
            "-y",
            "--no-install-recommends",
            &f.to_string_lossy(),
        ]),
        &env,
        log,
    )?;
    e.id = unique_id(p, &pkg);
    e.package = pkg.clone();
    if let Ok(n) = deb_field(f, "Description", log) {
        if let Some(first) = n.lines().next() {
            e.note = first.to_string();
        }
    }
    e.name = pretty_name(&pkg);
    // Programme des Pakets mit .desktop-Datei erscheinen von selbst im Startmenü (/usr/share/applications)
    if let Ok(files) = run_ok(&s(&["dpkg", "-L", &pkg]), &[], &mut |_| {}) {
        if let Some(d) = files
            .lines()
            .find(|l| l.starts_with("/usr/share/applications/") && l.ends_with(".desktop"))
        {
            if let Some(de) = parse_desktop(
                &std::fs::read_to_string(p.compat_root.join(d.trim_start_matches('/')))
                    .unwrap_or_default(),
            ) {
                e.name = de.name;
                e.run = de.exec;
            }
        }
    }
    Ok(())
}

fn install_rpm(
    p: &Paths,
    f: &Path,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    if !p.compat_has("/usr/bin/alien") {
        return Err("RPM-Pakete: im Ubuntu-Bereich fehlt „alien“ (ultar-compat apt install alien) – oder die .deb-Fassung nehmen".into());
    }
    let dir = p.incoming().join(format!("rpm-{}", &e.sha256[..12]));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    run_ok(
        &s(&[
            "sh",
            "-c",
            &format!(
                "cd '{}' && alien --to-deb --scripts '{}'",
                dir.display(),
                f.display()
            ),
        ]),
        &[],
        log,
    )?;
    let deb = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|x| x.path())
        .find(|x| x.extension().is_some_and(|x| x == "deb"))
        .ok_or("alien hat kein .deb erzeugt")?;
    let r = install_deb(p, &deb, e, log);
    let _ = std::fs::remove_dir_all(&dir);
    e.format = Some(Format::Rpm);
    r
}

// ---- Android

/// Paketname und ABIs aus einer APK lesen
pub fn apk_info(f: &Path) -> Result<(String, Vec<String>), String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(f).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut abis: Vec<String> = z
        .file_names()
        .filter_map(|n| n.strip_prefix("lib/"))
        .filter_map(|r| r.split('/').next())
        .filter(|a| !a.is_empty())
        .map(String::from)
        .collect();
    abis.sort();
    abis.dedup();
    let mut buf = Vec::new();
    z.by_name("AndroidManifest.xml")
        .map_err(|e| e.to_string())?
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    let pkg = axml_manifest_package(&buf).ok_or("AndroidManifest.xml: Paketname nicht lesbar")?;
    Ok((pkg, abis))
}

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn axml_strings(b: &[u8], off: usize) -> Option<Vec<String>> {
    let hsize = u16le(b, off + 2)? as usize;
    let count = u32le(b, off + 8)? as usize;
    let flags = u32le(b, off + 16)?;
    let start = u32le(b, off + 20)? as usize;
    let utf8 = flags & 0x100 != 0;
    let mut out = Vec::with_capacity(count.min(100_000));
    for i in 0..count.min(100_000) {
        let so = off + start + u32le(b, off + hsize + i * 4)? as usize;
        if utf8 {
            let mut o = so;
            let l1 = *b.get(o)? as usize; // Zeichenzahl
            o += if l1 & 0x80 != 0 { 2 } else { 1 };
            let mut bl = *b.get(o)? as usize;
            if bl & 0x80 != 0 {
                bl = ((bl & 0x7f) << 8) | *b.get(o + 1)? as usize;
                o += 2;
            } else {
                o += 1;
            }
            out.push(String::from_utf8_lossy(b.get(o..o + bl)?).into_owned());
        } else {
            let mut o = so;
            let mut l = u16le(b, o)? as usize;
            if l & 0x8000 != 0 {
                l = ((l & 0x7fff) << 16) | u16le(b, o + 2)? as usize;
                o += 4;
            } else {
                o += 2;
            }
            let units: Vec<u16> = (0..l).map(|k| u16le(b, o + k * 2)).collect::<Option<_>>()?;
            out.push(String::from_utf16_lossy(&units));
        }
    }
    Some(out)
}

/// Attribut `package` des Elements `<manifest>` aus binärem Android-XML (AXML)
pub fn axml_manifest_package(b: &[u8]) -> Option<String> {
    if u16le(b, 0)? != 0x0003 {
        return None;
    }
    let mut off = u16le(b, 2)? as usize;
    let mut strings: Vec<String> = Vec::new();
    while off + 8 <= b.len() {
        let ty = u16le(b, off)?;
        let size = u32le(b, off + 4)? as usize;
        if size < 8 {
            return None;
        }
        match ty {
            0x0001 => strings = axml_strings(b, off)?,
            0x0102 => {
                let hsize = u16le(b, off + 2)? as usize;
                let ext = off + hsize;
                let name = u32le(b, ext + 4)? as usize;
                if strings.get(name).map(String::as_str) == Some("manifest") {
                    let astart = u16le(b, ext + 8)? as usize;
                    let asize = u16le(b, ext + 10)? as usize;
                    let acount = u16le(b, ext + 12)? as usize;
                    for i in 0..acount {
                        let a = ext + astart + i * asize;
                        let an = u32le(b, a + 4)? as usize;
                        if strings.get(an).map(String::as_str) == Some("package") {
                            let raw = u32le(b, a + 8)?;
                            let data = u32le(b, a + 16)?;
                            let idx = if raw != u32::MAX { raw } else { data } as usize;
                            return strings.get(idx).cloned();
                        }
                    }
                    return None;
                }
            }
            _ => {}
        }
        off += size;
    }
    None
}

fn install_apk(
    p: &Paths,
    f: &Path,
    stem: &str,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let (pkg, abis) = apk_info(f)?;
    log(&format!(
        "Android-Paket {pkg}, Bibliotheken: {}",
        if abis.is_empty() {
            "keine (läuft überall)".into()
        } else {
            abis.join(", ")
        }
    ));
    e.id = unique_id(p, &pkg);
    e.package = pkg.clone();
    e.name = pretty_name(stem);
    let native: &[&str] = if host_arch() == "aarch64" {
        &["arm64-v8a", "armeabi-v7a", "armeabi"]
    } else {
        &["x86_64", "x86"]
    };
    if !abis.is_empty() && !abis.iter().any(|a| native.contains(&a.as_str())) {
        e.note = format!("Nur Code für {} – braucht eine Übersetzungsschicht (libndk/libhoudini) in der Android-Umgebung", abis.join("/"));
        log(&e.note.clone());
    }
    // Kopie behalten: Neuinstallation nach Zurücksetzen der Android-Umgebung, Installation später
    let keep = p.android_pending().join(format!("{pkg}.apk"));
    std::fs::create_dir_all(p.android_pending()).map_err(|e| e.to_string())?;
    std::fs::copy(f, &keep).map_err(|e| e.to_string())?;
    if p.compat_has("/usr/bin/waydroid") {
        // PC/Laptop: Waydroid (Android in Linux-Containern, gleicher Kernel)
        e.run = s(&["waydroid", "app", "launch", &pkg]);
        let st = run_ok(&s(&["waydroid", "status"]), &[], &mut |_| {}).unwrap_or_default();
        if st.contains("RUNNING")
            && st
                .lines()
                .any(|l| l.starts_with("Session") && l.contains("RUNNING"))
        {
            run_ok(
                &s(&["waydroid", "app", "install", &keep.to_string_lossy()]),
                &[],
                log,
            )?;
            let _ = std::fs::remove_file(&keep);
        } else {
            e.status = "pending".into();
            log("Android-Umgebung (Waydroid) läuft nicht – die App wird beim nächsten Start installiert (uapp android-sync)");
        }
    } else if Path::new("/usr/lib/ultar/android/ultar-android.sh").exists()
        || ultar_hal_system::current_edition() == ultar_profile::Edition::Phone
    {
        // Handy: abgeschottete Android-Umgebung (Standard AUS) installiert ausstehende APKs beim Start
        e.run = s(&["/usr/lib/ultar/android/ultar-android.sh", "launch", &pkg]);
        e.status = "pending".into();
        log("Wird in der abgeschotteten Android-Umgebung installiert, sobald sie gestartet ist");
    } else {
        return Err("Keine Android-Umgebung: im Ubuntu-Bereich „waydroid“ installieren (Desktop-Ausgabe hat es dabei)".into());
    }
    write_launcher(
        p,
        &e.id,
        &e.name,
        &format!("uapp run {}", e.id),
        "android",
        &mut e.launchers,
    )?;
    Ok(())
}

/// Ausstehende APKs installieren (Aufruf, sobald die Android-Umgebung läuft)
pub fn android_sync(p: &Paths, log: &mut dyn FnMut(&str)) -> Result<usize, String> {
    let mut n = 0;
    for mut e in list(p)
        .into_iter()
        .filter(|e| e.format == Some(Format::Apk) && e.status == "pending")
    {
        let apk = p.android_pending().join(format!("{}.apk", e.package));
        if !apk.exists() {
            continue;
        }
        if p.compat_has("/usr/bin/waydroid") {
            run_ok(
                &s(&["waydroid", "app", "install", &apk.to_string_lossy()]),
                &[],
                log,
            )?;
            let _ = std::fs::remove_file(&apk);
            e.status = "ok".into();
            save(p, &e)?;
            n += 1;
        }
    }
    Ok(n)
}

// ---- Windows (Wine)

fn wine_env(p: &Paths, id: &str) -> Vec<(String, String)> {
    let pre = p.wine(id);
    vec![
        ("WINEPREFIX".into(), pre.to_string_lossy().into()),
        ("HOME".into(), pre.join("home").to_string_lossy().into()),
        (
            "XDG_DATA_HOME".into(),
            pre.join("home/.local/share").to_string_lossy().into(),
        ),
        ("WINEDEBUG".into(), "-all".into()),
        // keine Nachfrage nach Mono/Gecko beim Einrichten (offline-fähig)
        ("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into()),
    ]
}

/// Windows-Programm abschotten: Laufwerk Z: (= ganzes Dateisystem) entfernen, nur D: = Austauschordner
pub fn wine_isolate(p: &Paths, id: &str) -> std::io::Result<()> {
    let dd = p.wine(id).join("dosdevices");
    std::fs::create_dir_all(&dd)?;
    let z = dd.join("z:");
    if std::fs::symlink_metadata(&z).is_ok() {
        std::fs::remove_file(&z)?;
    }
    std::fs::create_dir_all(p.share())?;
    let d = dd.join("d:");
    if std::fs::symlink_metadata(&d).is_err() {
        std::os::unix::fs::symlink(p.share(), &d)?;
    }
    Ok(())
}

/// Wine-Befehl im Ordner C: starten (ohne Laufwerk Z: findet Wine sonst kein Arbeitsverzeichnis)
fn in_drive_c(argv: Vec<String>) -> Vec<String> {
    let mut v = s(&[
        "sh",
        "-c",
        "cd \"$WINEPREFIX/drive_c\" 2>/dev/null; exec \"$@\"",
        "sh",
    ]);
    v.extend(argv);
    v
}

fn install_wine(
    p: &Paths,
    f: &Path,
    fmt: Format,
    stem: &str,
    o: &Opts,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let wine = ["/usr/bin/wine", "/usr/lib/wine/wine64", "/usr/bin/wine64"]
        .into_iter()
        .find(|w| p.compat_has(w));
    let Some(wine) = wine else {
        return Err(if host_arch() == "aarch64" {
            "Windows-Programme auf ARM brauchen Wine mit Übersetzung (Hangover/FEX) im Ubuntu-Bereich – noch nicht eingerichtet".into()
        } else {
            "Wine fehlt im Ubuntu-Bereich (Desktop-Ausgabe hat es dabei; sonst: ultar-compat apt install wine64)".into()
        });
    };
    let wine = if wine == "/usr/lib/wine/wine64" {
        "/usr/lib/wine/wine64"
    } else {
        "wine"
    };
    e.id = unique_id(p, stem);
    e.name = pretty_name(stem);
    let env = wine_env(p, &e.id);
    std::fs::create_dir_all(p.wine(&e.id).join("home")).map_err(|e| e.to_string())?;
    log("Eigener Wine-Bereich wird eingerichtet …");
    run_ok(&in_drive_c(s(&[wine, "wineboot", "--init"])), &env, log)?;
    if Policy::load(p).wine_isolate {
        wine_isolate(p, &e.id).map_err(|e| e.to_string())?;
        log("Abgeschottet: kein Laufwerk Z:, Austausch nur über D: (/data/ultar/share)");
    }
    // Installer bzw. Programm über einen Pfad in C: starten (Z: gibt es nicht mehr)
    let inst_dir = p.wine(&e.id).join("drive_c/ultar-setup");
    std::fs::create_dir_all(&inst_dir).map_err(|e| e.to_string())?;
    let fname = format!(
        "{}.{}",
        sanitize(stem),
        if fmt == Format::Msi { "msi" } else { "exe" }
    );
    std::fs::copy(f, inst_dir.join(&fname)).map_err(|e| e.to_string())?;
    let winpath = format!("C:\\ultar-setup\\{fname}");
    let before = wine_desktop_files(p, &e.id);
    let mut argv = if fmt == Format::Msi {
        s(&[wine, "msiexec", "/i", &winpath])
    } else {
        s(&[wine, &winpath])
    };
    if o.silent && fmt == Format::Msi {
        argv.push("/qn".into());
    }
    log("Installation läuft …");
    let r = run_ok(&in_drive_c(argv), &env, log);
    // Wine legt für Programme mit Startmenü-Verknüpfung .desktop-Dateien an
    let mut new: Vec<PathBuf> = wine_desktop_files(p, &e.id)
        .into_iter()
        .filter(|x| !before.contains(x))
        .collect();
    new.sort();
    if let Err(err) = r {
        if new.is_empty() {
            return Err(err);
        }
        log(&format!(
            "Installer meldete einen Fehler, hat aber Programme angelegt: {err}"
        ));
    }
    let skip = |n: &str| {
        let l = n.to_lowercase();
        l.contains("uninstall")
            || l.contains("deinstall")
            || l.contains("readme")
            || l.contains("website")
    };
    for (i, d) in new.iter().enumerate() {
        let Some(de) = parse_desktop(&std::fs::read_to_string(d).unwrap_or_default()) else {
            continue;
        };
        if skip(&de.name) {
            continue;
        }
        if e.run.is_empty() {
            e.name = de.name.clone();
            e.run = de.exec.clone();
        }
        let fname = format!("ultar-{}-{i}.desktop", e.id);
        let txt = std::fs::read_to_string(d).unwrap_or_default();
        std::fs::create_dir_all(p.launchers()).map_err(|e| e.to_string())?;
        std::fs::write(
            p.launchers().join(&fname),
            add_desktop_key(&txt, "X-Ultar-App", &e.id),
        )
        .map_err(|e| e.to_string())?;
        e.launchers.push(fname);
    }
    if e.run.is_empty() {
        if fmt == Format::Msi {
            e.note = "MSI ohne Startmenü-Eintrag installiert (z. B. Laufzeitbibliothek)".into();
        } else {
            // Kein Installer, sondern ein eigenständiges Programm: dauerhaft in C:\Programme\Ultar ablegen
            let pdir = p.wine(&e.id).join("drive_c/Program Files/Ultar");
            std::fs::create_dir_all(&pdir).map_err(|e| e.to_string())?;
            let exe = format!("{}.exe", sanitize(stem));
            std::fs::copy(f, pdir.join(&exe)).map_err(|e| e.to_string())?;
            e.run = s(&[wine, &format!("C:\\Program Files\\Ultar\\{exe}")]);
            e.note = "Eigenständiges Programm (ohne Installer)".into();
        }
    }
    let _ = std::fs::remove_dir_all(&inst_dir);
    e.env = env;
    if e.launchers.is_empty() && !e.run.is_empty() {
        write_launcher(
            p,
            &e.id,
            &e.name,
            &format!("uapp run {}", e.id),
            "wine",
            &mut e.launchers,
        )?;
    }
    Ok(())
}

fn wine_desktop_files(p: &Paths, id: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let root = p.wine(id).join("home/.local/share/applications");
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for x in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let pa = x.path();
            if pa.is_dir() {
                stack.push(pa);
            } else if pa.extension().is_some_and(|e| e == "desktop") {
                out.push(pa);
            }
        }
    }
    out
}

// ---- AppImage, Flatpak

fn elf_machine(f: &Path) -> Option<u16> {
    let mut h = [0u8; 20];
    std::fs::File::open(f).ok()?.read_exact(&mut h).ok()?;
    Some(u16::from_le_bytes([h[18], h[19]]))
}

fn install_appimage(
    p: &Paths,
    f: &Path,
    stem: &str,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let want = if host_arch() == "aarch64" { 183 } else { 62 };
    if let Some(m) = elf_machine(f) {
        if m != want {
            return Err(format!(
                "AppImage ist für eine andere Prozessor-Architektur (ELF {m}), dieses Gerät: {}",
                host_arch()
            ));
        }
    }
    e.id = unique_id(p, stem);
    e.name = pretty_name(stem);
    let dir = p.app_dir(&e.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join(format!("{}.AppImage", sanitize(stem)));
    std::fs::copy(f, &dst).map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    // ohne FUSE lauffähig: entpacken und starten
    e.env = vec![("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into())];
    e.run = vec![dst.to_string_lossy().into()];
    log("AppImage abgelegt (Start ohne FUSE über Entpacken)");
    write_launcher(
        p,
        &e.id,
        &e.name,
        &format!("uapp run {}", e.id),
        "appimage",
        &mut e.launchers,
    )
}

fn flatpak_apps(log: &mut dyn FnMut(&str)) -> Vec<String> {
    run_ok(
        &s(&["flatpak", "list", "--app", "--columns=application"]),
        &[],
        log,
    )
    .unwrap_or_default()
    .lines()
    .map(|l| l.trim().to_string())
    .filter(|l| !l.is_empty())
    .collect()
}

fn install_flatpak(
    p: &Paths,
    f: &Path,
    fmt: Format,
    stem: &str,
    e: &mut Entry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    if !p.compat_has("/usr/bin/flatpak") {
        return Err("Flatpak fehlt im Ubuntu-Bereich (Desktop-Ausgabe hat es dabei; sonst: ultar-compat apt install flatpak)".into());
    }
    // Laufzeiten (GNOME/KDE/Freedesktop) kommen von Flathub
    let _ = run_ok(
        &s(&[
            "flatpak",
            "remote-add",
            "--if-not-exists",
            "--system",
            "flathub",
            "https://dl.flathub.org/repo/flathub.flatpakrepo",
        ]),
        &[],
        log,
    );
    let before = flatpak_apps(&mut |_| {});
    let fs = f.to_string_lossy().to_string();
    let argv = if fmt == Format::FlatpakRef {
        s(&[
            "flatpak",
            "install",
            "-y",
            "--noninteractive",
            "--system",
            "--from",
            &fs,
        ])
    } else {
        s(&[
            "flatpak",
            "install",
            "-y",
            "--noninteractive",
            "--system",
            "--bundle",
            &fs,
        ])
    };
    run_ok(&argv, &[], log)?;
    let after = flatpak_apps(&mut |_| {});
    let appid = after
        .iter()
        .find(|a| !before.contains(a))
        .cloned()
        .or_else(|| {
            std::fs::read_to_string(f).ok().and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("Name=").map(|x| x.trim().to_string()))
            })
        });
    let appid = appid.ok_or("Flatpak installiert, App-Kennung aber nicht gefunden")?;
    e.id = unique_id(p, &appid);
    e.package = appid.clone();
    e.name = pretty_name(appid.rsplit('.').next().unwrap_or(stem));
    e.run = s(&["flatpak", "run", &appid]);
    Ok(())
}

// ---------------------------------------------------------------- Starten, Entfernen

pub fn run(p: &Paths, id: &str) -> Result<std::process::Child, String> {
    let e = get(p, id).ok_or_else(|| format!("{id}: nicht installiert"))?;
    if e.status == "pending" {
        return Err(format!("{}: wartet noch auf die Android-Umgebung", e.name));
    }
    if e.run.is_empty() {
        return Err(format!("{}: kein Startbefehl", e.name));
    }
    if e.format == Some(Format::Uap) {
        let mut c = Command::new(&e.run[0]);
        c.args(&e.run[1..]);
        return c.spawn().map_err(|x| x.to_string());
    }
    let argv = if matches!(e.format, Some(Format::Exe) | Some(Format::Msi)) {
        in_drive_c(e.run.clone())
    } else {
        e.run.clone()
    };
    compat_cmd(&argv, &e.env).spawn().map_err(|x| x.to_string())
}

pub fn remove(p: &Paths, id: &str, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let e = get(p, id).ok_or_else(|| format!("{id}: nicht installiert"))?;
    match e.format {
        Some(Format::Deb) | Some(Format::Rpm) => {
            let env = vec![("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())];
            run_ok(
                &s(&["apt-get", "remove", "-y", "--autoremove", &e.package]),
                &env,
                log,
            )?;
        }
        Some(Format::Apk) => {
            let _ = std::fs::remove_file(p.android_pending().join(format!("{}.apk", e.package)));
            if e.status == "ok" && p.compat_has("/usr/bin/waydroid") {
                run_ok(&s(&["waydroid", "app", "remove", &e.package]), &[], log)?;
            }
        }
        Some(Format::Exe) | Some(Format::Msi) => {
            let _ = std::fs::remove_dir_all(p.wine(&e.id));
        }
        Some(Format::AppImage) => {
            let _ = std::fs::remove_dir_all(p.app_dir(&e.id));
        }
        Some(Format::Flatpak) | Some(Format::FlatpakRef) => {
            run_ok(
                &s(&[
                    "flatpak",
                    "uninstall",
                    "-y",
                    "--noninteractive",
                    "--system",
                    &e.package,
                ]),
                &[],
                log,
            )?;
        }
        Some(Format::Uap) => {
            // native Pakete: Dateien der Nutzlast liegen unter /data/ultar/apps/<id> bzw. im System
            let _ = std::fs::remove_dir_all(p.app_dir(&e.id));
        }
        _ => {}
    }
    for l in &e.launchers {
        let _ = std::fs::remove_file(p.launchers().join(l));
    }
    std::fs::remove_file(p.registry().join(format!("{}.toml", e.id))).map_err(|x| x.to_string())?;
    log(&format!("Entfernt: {}", e.name));
    Ok(())
}

// ---------------------------------------------------------------- .desktop

pub struct DesktopEntry {
    pub name: String,
    pub exec: Vec<String>,
    pub icon: String,
}

/// Exec-Zeile einer .desktop-Datei in argv zerlegen (Freedesktop-Regeln: erst String-Escapes, dann Anführungszeichen)
pub fn split_exec(raw: &str) -> Vec<String> {
    // Ebene 1: Escapes des Werts (\s \n \t \r \\)
    let mut v = String::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('s') => v.push(' '),
                Some('n') => v.push('\n'),
                Some('t') => v.push('\t'),
                Some('r') => v.push('\r'),
                Some('\\') => v.push('\\'),
                Some(o) => {
                    v.push('\\');
                    v.push(o)
                }
                None => v.push('\\'),
            }
        } else {
            v.push(c);
        }
    }
    // Ebene 2: Argumente mit Anführungszeichen; Feldcodes %f %u … entfallen
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut q = false;
    let mut it = v.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                q = !q;
                has = true
            }
            '\\' => {
                if let Some(n) = it.next() {
                    cur.push(n);
                    has = true;
                }
            }
            c if c.is_whitespace() && !q => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true
            }
        }
    }
    if has {
        out.push(cur);
    }
    out.retain(|a| !(a.len() == 2 && a.starts_with('%')));
    out
}

pub fn parse_desktop(txt: &str) -> Option<DesktopEntry> {
    let mut in_main = false;
    let (mut name, mut exec, mut icon) = (None, None, String::new());
    for l in txt.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some(v) = l.strip_prefix("Name=") {
            name.get_or_insert(v.to_string());
        } else if let Some(v) = l.strip_prefix("Exec=") {
            exec = Some(split_exec(v));
        } else if let Some(v) = l.strip_prefix("Icon=") {
            icon = v.to_string();
        }
    }
    Some(DesktopEntry {
        name: name?,
        exec: exec?,
        icon,
    })
}

fn add_desktop_key(txt: &str, k: &str, v: &str) -> String {
    let mut o = String::new();
    let mut done = false;
    for l in txt.lines() {
        o.push_str(l);
        o.push('\n');
        if !done && l.trim() == "[Desktop Entry]" {
            o.push_str(&format!("{k}={v}\n"));
            done = true;
        }
    }
    o
}

fn write_launcher(
    p: &Paths,
    id: &str,
    name: &str,
    exec: &str,
    kind: &str,
    list: &mut Vec<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(p.launchers()).map_err(|e| e.to_string())?;
    let fname = format!("ultar-{id}.desktop");
    let icon = match kind {
        "android" => "android",
        "wine" => "wine",
        _ => "application-x-executable",
    };
    let txt = format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nIcon={icon}\nCategories=Ultar;\nX-Ultar-App={id}\nX-Ultar-Kind={kind}\n");
    std::fs::write(p.launchers().join(&fname), txt).map_err(|e| e.to_string())?;
    if !list.contains(&fname) {
        list.push(fname);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("uapp-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detect_magic() {
        let d = tmp("magic");
        let cases: Vec<(&str, Vec<u8>, Format)> = vec![
            ("a.deb", b"!<arch>\ndebian-binary   ".to_vec(), Format::Deb),
            ("a.exe", b"MZ\x90\x00".to_vec(), Format::Exe),
            ("renamed.txt", b"MZ\x90\x00".to_vec(), Format::Exe),
            (
                "a.msi",
                vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0],
                Format::Msi,
            ),
            (
                "a.doc",
                vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0],
                Format::Unknown,
            ),
            (
                "x.AppImage",
                {
                    let mut v = b"\x7fELF\x02\x01\x01\x00AI\x02".to_vec();
                    v.resize(64, 0);
                    v
                },
                Format::AppImage,
            ),
            (
                "plain",
                {
                    let mut v = b"\x7fELF\x02\x01\x01\x00\x00\x00".to_vec();
                    v.resize(64, 0);
                    v
                },
                Format::Unknown,
            ),
            ("a.rpm", vec![0xED, 0xAB, 0xEE, 0xDB, 3], Format::Rpm),
            (
                "a.flatpakref",
                b"[Flatpak Ref]\nName=org.x.Y\n".to_vec(),
                Format::FlatpakRef,
            ),
        ];
        for (n, b, want) in cases {
            let f = d.join(n);
            std::fs::write(&f, &b).unwrap();
            assert_eq!(detect(&f).unwrap(), want, "{n}");
        }
    }

    /// Minimales binäres AndroidManifest (UTF-16-Stringpool) mit <manifest package="...">
    fn axml(pkg: &str) -> Vec<u8> {
        let strs = ["manifest", "package", pkg];
        let mut data = Vec::new();
        let mut offs = Vec::new();
        for s in strs {
            offs.push(data.len() as u32);
            let u: Vec<u16> = s.encode_utf16().collect();
            data.extend((u.len() as u16).to_le_bytes());
            for c in &u {
                data.extend(c.to_le_bytes());
            }
            data.extend([0, 0]);
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let hdr = 28u32;
        let start = hdr + 4 * strs.len() as u32;
        let mut sp = Vec::new();
        sp.extend(1u16.to_le_bytes());
        sp.extend((hdr as u16).to_le_bytes());
        sp.extend((start + data.len() as u32).to_le_bytes());
        sp.extend((strs.len() as u32).to_le_bytes());
        sp.extend(0u32.to_le_bytes());
        sp.extend(0u32.to_le_bytes());
        sp.extend(start.to_le_bytes());
        sp.extend(0u32.to_le_bytes());
        for o in offs {
            sp.extend(o.to_le_bytes());
        }
        sp.extend(&data);
        let mut el = Vec::new();
        el.extend(0x0102u16.to_le_bytes());
        el.extend(16u16.to_le_bytes());
        el.extend((16u32 + 20 + 20).to_le_bytes());
        el.extend(1u32.to_le_bytes());
        el.extend(u32::MAX.to_le_bytes());
        el.extend(u32::MAX.to_le_bytes()); // ns
        el.extend(0u32.to_le_bytes()); // name = manifest
        el.extend(20u16.to_le_bytes());
        el.extend(20u16.to_le_bytes());
        el.extend(1u16.to_le_bytes());
        el.extend([0u8; 6]);
        el.extend(u32::MAX.to_le_bytes()); // attr ns
        el.extend(1u32.to_le_bytes()); // "package"
        el.extend(2u32.to_le_bytes()); // raw = pkg
        el.extend(8u16.to_le_bytes());
        el.push(0);
        el.push(3);
        el.extend(2u32.to_le_bytes());
        let mut out = Vec::new();
        out.extend(3u16.to_le_bytes());
        out.extend(8u16.to_le_bytes());
        out.extend((8 + sp.len() as u32 + el.len() as u32).to_le_bytes());
        out.extend(sp);
        out.extend(el);
        out
    }

    #[test]
    fn apk_package_name() {
        assert_eq!(
            axml_manifest_package(&axml("tech.codecraft.demo")).as_deref(),
            Some("tech.codecraft.demo")
        );
        let d = tmp("apk");
        let f = d.join("demo.apk");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&f).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("AndroidManifest.xml", o).unwrap();
            std::io::Write::write_all(&mut z, &axml("org.example.app")).unwrap();
            z.start_file("lib/arm64-v8a/libx.so", o).unwrap();
            z.start_file("classes.dex", o).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(detect(&f).unwrap(), Format::Apk);
        let (pkg, abis) = apk_info(&f).unwrap();
        assert_eq!(pkg, "org.example.app");
        assert_eq!(abis, vec!["arm64-v8a"]);
    }

    #[test]
    fn exec_split() {
        assert_eq!(
            split_exec(
                r#"env WINEPREFIX="/data/ultar/wine/x y" wine C:\\\\ProgramData\\\\A\\\\B.lnk"#
            ),
            vec![
                "env",
                "WINEPREFIX=/data/ultar/wine/x y",
                "wine",
                r"C:\ProgramData\A\B.lnk"
            ]
        );
        assert_eq!(
            split_exec("code --new-window %F"),
            vec!["code", "--new-window"]
        );
    }

    #[test]
    fn policy_and_ids() {
        let d = tmp("pol");
        let p = Paths {
            data: d.clone(),
            compat_root: d.join("nocompat"),
        };
        assert!(Policy::default().check(Format::Exe, false).is_ok());
        let deny = Policy {
            allow_foreign: false,
            ..Default::default()
        };
        assert!(deny.check(Format::Apk, false).is_err());
        assert!(deny.check(Format::Uap, false).is_ok());
        assert!(deny.check(Format::Apk, true).is_ok());
        let pol = Policy {
            deny_formats: vec!["exe".into()],
            ..Default::default()
        };
        assert!(pol.check(Format::Exe, true).is_err());
        assert_eq!(sanitize("My App_Setup x64!"), "my-app-setup-x64");
        assert_eq!(pretty_name("Notepad++_8.6_Installer_x64"), "Notepad++");
        let e = Entry {
            id: "my-app".into(),
            name: "My App".into(),
            status: "ok".into(),
            ..Default::default()
        };
        save(&p, &e).unwrap();
        assert_eq!(unique_id(&p, "My App"), "my-app-2");
        assert_eq!(list(&p).len(), 1);
    }
}
