//! Windows programs (`.exe`, `.msi`) as apps of their own – stage P4 of
//! `docs/FREMD-APPS.md`.
//!
//! The service reads what the file says about itself ([`crate::pe`], [`crate::msi`]:
//! bounded readers, never executed), asks an administrator, keeps a copy of the file as the
//! app's **own installer** (`<root>/setup/<app id>/`) and registers the app with the
//! launcher. The program installs itself on its first start **in its own cell** (profile
//! `app.foreign`, the Linux area read-only, its own data, no network unless allowed): the
//! launcher binds the installer read-only at `/.setup` and the area's wrapper
//! `ultar-wine-app` makes the Wine prefix (only drive `C:`), runs the installer (MSI, NSIS
//! and Inno Setup without questions, any other installer with its own window) and starts the
//! installed program. Nothing of the file runs anywhere else.

use crate::detect::{self, Opened};
use crate::jobs::{confirm, question, register_one, step, Ctx, Request};
use crate::msi;
use crate::pe::{self, Installer};
use crate::state::{slug, AppRecord, State};
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use ultar_bus::messages::foreign::{Failure, Source, Step};
use ultar_bus::Client;

/// The wrapper in the Linux area that installs and starts a Windows program in its cell.
pub const WRAPPER: &str = "/usr/lib/ultar/wine/ultar-wine-app";
/// Wine in the Linux area (component „Windows programs“).
pub const WINE: &str = "/usr/bin/wine";

/// How the wrapper installs the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setup {
    /// Windows Installer database: `msiexec /i … /qb`.
    Msi,
    /// Nullsoft installer: `/S`.
    Nsis,
    /// Inno Setup: `/SILENT`.
    Inno,
    /// Another installer: with its own window.
    Interactive,
    /// A program without an installer: copied into `C:\Program Files`.
    Portable,
}

impl Setup {
    /// The wrapper's word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Msi => "msi",
            Self::Nsis => "nsis",
            Self::Inno => "inno",
            Self::Interactive => "setup",
            Self::Portable => "portable",
        }
    }
}

/// What a Windows file says about itself (all of it unverified).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Described {
    /// `.exe` or `.msi`.
    pub source: Source,
    /// Product name (or the file's stem).
    pub name: String,
    /// Version.
    pub version: String,
    /// Company or manufacturer.
    pub publisher: String,
    /// How it is installed.
    pub setup: Setup,
    /// Its PNG icon (programs only).
    pub icon: Option<Vec<u8>>,
}

/// Reads the opened file; `Err(Damaged)`: not a program or installer (or a library).
pub fn describe(opened: &Opened, path: &Path) -> Result<Described, Failure> {
    let stem = detect::stem(path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match opened.source {
        Source::Msi => {
            let info = msi::read(&opened.file).ok_or(Failure::Damaged)?;
            Ok(Described {
                source: Source::Msi,
                name: non_empty(info.product, &stem),
                version: info.version,
                publisher: info.manufacturer,
                setup: Setup::Msi,
                icon: None,
            })
        }
        Source::Exe => {
            let info = pe::read(&opened.file).ok_or(Failure::Damaged)?;
            if info.dll {
                return Err(Failure::Damaged);
            }
            let setup = match info.installer {
                Some(Installer::Nsis) => Setup::Nsis,
                Some(Installer::Inno) => Setup::Inno,
                _ if ["setup", "install"].iter().any(|w| file_name.contains(w)) => {
                    Setup::Interactive
                }
                _ => Setup::Portable,
            };
            Ok(Described {
                source: Source::Exe,
                name: non_empty(info.product, &stem),
                version: info.version,
                publisher: info.company,
                setup,
                icon: pe::png_icon(&opened.file),
            })
        }
        _ => Err(Failure::Damaged),
    }
}

fn non_empty(text: String, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_owned()
    } else {
        text
    }
}

/// The app id's own part from the product's name, without its version or remarks in
/// parentheses (an update keeps the id): `Notepad++` → `notepad`, `7-Zip 24.09 (x64 edition)`
/// → `7-zip`. Lowercase letters, digits and single `-`, at most 40.
pub fn id_base(name: &str) -> Option<String> {
    let mut words = Vec::new();
    let mut depth = 0usize;
    for w in name.split_whitespace() {
        if w.starts_with('(') {
            depth += 1;
        }
        let digits = w.trim_start_matches(['v', 'V']);
        let version = !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            && digits.chars().any(|c| c.is_ascii_digit());
        if depth == 0 && !version {
            words.push(w);
        }
        if w.ends_with(')') {
            depth = depth.saturating_sub(1);
        }
    }
    let mut out = String::new();
    for ch in words.join(" ").chars().flat_map(char::to_lowercase) {
        let c = if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            ch
        } else {
            '-'
        };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    let out: String = out.chars().take(40).collect();
    let out = out.trim_matches('-');
    (!out.is_empty()).then(|| out.to_owned())
}

/// The installer's name in its directory: the original's letters, digits and `._+-`, the
/// right ending, at most 64 bytes (the wrapper finds it at `/.setup/<name>`).
pub fn setup_file_name(path: &Path, source: Source) -> String {
    let ending = if source == Source::Msi {
        ".msi"
    } else {
        ".exe"
    };
    let stem: String = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._+-".contains(*c))
        .take(56)
        .collect();
    let stem = stem.trim_start_matches('.');
    if stem.is_empty() {
        format!("setup{ending}")
    } else {
        format!("{stem}{ending}")
    }
}

/// Copies the opened file into a fresh `dir` (`0755`, the file `0644`: the app's user reads
/// it through the read-only bind; only the service writes there).
fn keep_installer(source: &mut std::fs::File, dir: &Path, name: &str) -> std::io::Result<()> {
    use std::io::Seek;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
    source.seek(std::io::SeekFrom::Start(0))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(dir.join(name))?;
    // readable by the app's own user in its cell (the service's umask is strict)
    out.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    let copied = std::io::copy(&mut source.take(detect::MAX_FILE + 1), &mut out)?;
    if copied > detect::MAX_FILE {
        return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
    }
    out.sync_all()
}

/// Writes the program's icon (`<icons>/<id>.png`); empty when there is none.
fn keep_icon(icons: &Path, id: &str, png: Option<&[u8]>) -> String {
    let Some(png) = png else {
        return String::new();
    };
    let path = icons.join(format!("{id}.png"));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&path)
        .and_then(|mut f| {
            // readable by the launcher and the desktop (the service's umask is strict)
            f.set_permissions(std::fs::Permissions::from_mode(0o644))?;
            std::io::Write::write_all(&mut f, png)
        });
    match written {
        Ok(()) => path.to_string_lossy().into_owned(),
        Err(_) => String::new(),
    }
}

/// The app record of a Windows program.
fn record(id: String, d: &Described, file: &str, stem: &str, icon: String) -> AppRecord {
    AppRecord {
        id,
        name: d.name.clone(),
        names: Vec::new(),
        source: d.source.word().into(),
        package: d.name.clone(),
        entry: stem.to_owned(),
        version: d.version.clone(),
        icon,
        exec: WRAPPER.into(),
        args: vec![d.setup.word().into(), file.to_owned(), d.name.clone()],
        network: false,
        lan: false,
        folders: Vec::new(),
    }
}

/// Removes what the service keeps of a Windows program (its installer, its icon).
pub fn forget(ctx: &Ctx, record: &AppRecord) {
    let _ = std::fs::remove_dir_all(ctx.layout.setup().join(&record.id));
    if !record.icon.is_empty() {
        let _ = std::fs::remove_file(&record.icon);
    }
}

/// Installs a `.exe` or `.msi` of the person: confirmed by an administrator, kept as the
/// app's own installer, registered as an `app.foreign` app (`Ok(None)`: not confirmed).
pub fn install(
    ctx: &Ctx,
    client: &Client,
    state: &State,
    req: &Request,
) -> Result<Option<Vec<AppRecord>>, Failure> {
    step(client, req.job, Step::Prepare, 5, Vec::new());
    let path = PathBuf::from(&req.path);
    let mut opened = detect::open_source(&ctx.sources, &req.path).map_err(|_| Failure::Damaged)?;
    if opened.size > detect::MAX_FILE {
        return Err(Failure::Damaged);
    }
    let described = describe(&opened, &path)?;
    step(client, req.job, Step::Confirm, 15, Vec::new());
    let text = question(&described.name, described.source, &req.choices, false);
    if !confirm(client, &req.caller, text)? {
        return Ok(None);
    }
    let stem = id_base(&described.name)
        .or_else(|| slug(&detect::stem(&path).replace('.', "-"), 40))
        .unwrap_or_else(|| "program".into());
    let id = state
        .id_for(described.source, &described.name, &stem)
        .ok_or(Failure::Internal)?;
    step(client, req.job, Step::Install, 40, Vec::new());
    let file = setup_file_name(&path, described.source);
    let dir = ctx.layout.setup().join(&id);
    if keep_installer(&mut opened.file, &dir, &file).is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Failure::Space);
    }
    let icon = keep_icon(&ctx.layout.icons(), &id, described.icon.as_deref());
    let mut app = record(id, &described, &file, &stem, icon);
    app.set_choices(&req.choices);
    step(client, req.job, Step::Register, 85, Vec::new());
    if register_one(client, &app) {
        Ok(Some(vec![app]))
    } else {
        forget(ctx, &app);
        Err(Failure::Launcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn opened(bytes: &[u8], name: &str) -> (Opened, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ufwin-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        let o = detect::open_source(&[dir], path.to_str().unwrap()).unwrap();
        (o, path)
    }

    #[test]
    fn programs_and_installers_are_described() {
        let mut overlay = vec![0u8; 64];
        overlay.extend_from_slice(b"\xEF\xBE\xAD\xDENullsoftInst");
        let nsis = pe::tests::sample(
            &[
                ("ProductName", "Notepad++"),
                ("CompanyName", "Don Ho"),
                ("ProductVersion", "8.7"),
            ],
            &overlay,
        );
        let (o, p) = opened(&nsis, "npp.8.7.Installer.x64.exe");
        let d = describe(&o, &p).unwrap();
        assert_eq!((d.source, d.setup), (Source::Exe, Setup::Nsis));
        assert_eq!(
            (d.name.as_str(), d.publisher.as_str()),
            ("Notepad++", "Don Ho")
        );
        let plain = pe::tests::sample(&[], &[]);
        let (o, p) = opened(&plain, "tool.exe");
        let d = describe(&o, &p).unwrap();
        assert_eq!((d.setup, d.name.as_str()), (Setup::Portable, "tool"));
        let (o, p) = opened(&plain, "MySetup.exe");
        assert_eq!(describe(&o, &p).unwrap().setup, Setup::Interactive);
        let mut dll = pe::tests::sample(&[], &[]);
        dll[0x84 + 18] = 0x22;
        dll[0x84 + 19] = 0x20;
        let (o, p) = opened(&dll, "library.exe");
        assert_eq!(describe(&o, &p), Err(Failure::Damaged));
    }

    #[test]
    fn ids_come_from_the_product_name_without_its_version() {
        assert_eq!(id_base("Notepad++").as_deref(), Some("notepad"));
        assert_eq!(
            id_base("7-Zip 24.09 (x64 edition)").as_deref(),
            Some("7-zip")
        );
        assert_eq!(id_base("GIMP v2.10.38").as_deref(), Some("gimp"));
        assert_eq!(id_base("Paint.NET").as_deref(), Some("paint-net"));
        assert_eq!(id_base("ملف"), None);
        assert_eq!(id_base("1.0 (beta)"), None);
    }

    #[test]
    fn the_installers_name_is_safe() {
        let n = |p: &str, s| setup_file_name(Path::new(p), s);
        assert_eq!(
            n("/d/npp.8.7.Installer.x64.exe", Source::Exe),
            "npp.8.7.Installer.x64.exe"
        );
        assert_eq!(n("/d/7z 24 x64.MSI", Source::Msi), "7z24x64.msi");
        assert_eq!(n("/d/..hidden.exe", Source::Exe), "hidden.exe");
        assert_eq!(n("/d/مثبت.exe", Source::Exe), "setup.exe");
        assert!(!n(&format!("/d/{}.exe", "a".repeat(300)), Source::Exe).contains('/'));
        assert!(n(&format!("/d/{}.exe", "a".repeat(300)), Source::Exe).len() <= 64);
    }

    #[test]
    fn the_kept_installer_and_icon_are_readable_by_the_app() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ultar-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source_path = dir.join("source.exe");
        std::fs::write(&source_path, b"MZ installer").unwrap();
        // the service's strict umask
        let old = rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));
        let mut source = std::fs::File::open(&source_path).unwrap();
        keep_installer(&mut source, &dir.join("setup"), "setup.exe").unwrap();
        let icon = keep_icon(&dir, "foreign.exe-x", Some(b"\x89PNG"));
        rustix::process::umask(old);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.join("setup")), 0o755);
        assert_eq!(mode(&dir.join("setup/setup.exe")), 0o644);
        assert_eq!(
            std::fs::read(dir.join("setup/setup.exe")).unwrap(),
            b"MZ installer"
        );
        assert_eq!(mode(Path::new(&icon)), 0o644);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_record_starts_the_wrapper_with_the_installer() {
        let d = Described {
            source: Source::Msi,
            name: "7-Zip".into(),
            version: "24.09".into(),
            publisher: "Igor Pavlov".into(),
            setup: Setup::Msi,
            icon: None,
        };
        let r = record("foreign.msi-7z".into(), &d, "7z.msi", "a7z", String::new());
        assert_eq!(r.exec, WRAPPER);
        assert_eq!(r.args, ["msi", "7z.msi", "7-Zip"]);
        let t: toml::Table = toml::from_str(&r.manifest()).unwrap();
        assert_eq!(t["kind"].as_str(), Some("compat-exe"));
        assert_eq!(t["permissions"]["network"].as_str(), Some("none"));
    }
}
