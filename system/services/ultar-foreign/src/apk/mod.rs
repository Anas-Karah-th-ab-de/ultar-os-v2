//! Android apps (`.apk`, stage P5 of `docs/FREMD-APPS.md`): what an APK says about
//! itself and whether its signature holds – read by the service's own code **inside the
//! inspection cell** (no network, `nobody`, every right dropped), never in the service
//! itself; the cell answers with a few bounded lines ([`Report::to_lines`]) that the
//! service checks again ([`Report::from_lines`]).
//!
//! In the installation cell the same code puts the APK into the Linux area
//! (`/opt/ultar-android/apps/<package>/base.apk`, read-only for the app) and its
//! x86-64 native libraries next to it (`lib/`), so that nothing executable ever lives in
//! the app's own (writable, `noexec`) data.

pub mod arsc;
pub mod axml;
pub mod sig;
#[cfg(any(test, feature = "test-sign"))]
pub mod testsign;
pub mod zip;

use std::io::Write;
use std::path::Path;

/// Where the Android apps live in the Linux area.
pub const APPS_DIR: &str = "/opt/ultar-android/apps";
/// The start program of the Android runtime in the area (`make-android-runtime.sh`).
pub const RUN: &str = "/opt/ultar-android/bin/ultar-apk-run";
/// First line of the cell's answer.
const HEADER: &str = "ultar-apk 1";
/// Largest symbol taken.
const MAX_ICON: usize = 256 << 10;
/// Largest native library and all of them together.
const MAX_LIB: usize = 256 << 20;
const MAX_LIBS_TOTAL: u64 = 1 << 30;
/// Most native libraries.
const MAX_LIBS: usize = 512;
/// Longest text of a report line.
const MAX_TEXT: usize = 128;

/// Which processors the app's native code is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    /// No native code: runs anywhere.
    Java,
    /// Has x86-64 code.
    X86_64,
    /// Only ARM code.
    Arm,
    /// Only code for other processors (32-bit x86, MIPS …).
    Other,
}

impl Abi {
    fn word(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::X86_64 => "x86_64",
            Self::Arm => "arm",
            Self::Other => "other",
        }
    }

    fn from_word(w: &str) -> Option<Self> {
        [Self::Java, Self::X86_64, Self::Arm, Self::Other]
            .into_iter()
            .find(|a| a.word() == w)
    }
}

/// The signature in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sig {
    /// APK Signature Scheme v2 or v3, verified.
    Valid(u8),
    /// JAR signing only.
    OldOnly,
    /// None.
    Unsigned,
    /// Does not verify.
    Invalid,
}

impl Sig {
    fn word(self) -> String {
        match self {
            Self::Valid(n) => format!("v{n}"),
            Self::OldOnly => "v1".into(),
            Self::Unsigned => "none".into(),
            Self::Invalid => "bad".into(),
        }
    }

    fn from_word(w: &str) -> Option<Self> {
        Some(match w {
            "v2" => Self::Valid(2),
            "v3" => Self::Valid(3),
            "v1" => Self::OldOnly,
            "none" => Self::Unsigned,
            "bad" => Self::Invalid,
            _ => return None,
        })
    }
}

/// What an APK says about itself (unverified, except for the signature).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Package name (`org.example.app`).
    pub package: String,
    /// Name in the default language.
    pub label: String,
    /// Names by language (`ar`, `en`, `de`).
    pub labels: Vec<(String, String)>,
    /// Version as the app shows it.
    pub version: String,
    /// Android's version number (updates never go back).
    pub version_code: u32,
    /// Lowest Android version it runs on (API level).
    pub min_sdk: u32,
    /// The launcher activity in the form the runtime takes (`org/example/Main`).
    pub activity: String,
    /// Native code.
    pub abi: Abi,
    /// Signature.
    pub sig: Sig,
    /// SHA-256 fingerprints of the signers' certificates with their common names.
    pub signers: Vec<(String, String)>,
    /// Android permissions it asks for.
    pub permissions: Vec<String>,
    /// Its symbol (PNG), if one could be found.
    pub icon: Option<Vec<u8>>,
}

/// A Java package name: dotted parts of letters, digits and `_`, at least two parts.
pub fn package_ok(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    name.len() <= MAX_TEXT
        && parts.len() >= 2
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// A class name in the runtime's form (`org/example/Main$Inner`).
fn activity_ok(class: &str) -> bool {
    class.len() <= MAX_TEXT
        && class.contains('/')
        && class.split('/').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

fn abi_of(names: &[String]) -> Abi {
    let mut abis: Vec<&str> = names
        .iter()
        .filter_map(|n| n.strip_prefix("lib/")?.split_once('/'))
        .filter(|(_, file)| file.ends_with(".so"))
        .map(|(abi, _)| abi)
        .collect();
    abis.sort_unstable();
    abis.dedup();
    if abis.is_empty() {
        Abi::Java
    } else if abis.contains(&"x86_64") {
        Abi::X86_64
    } else if abis.iter().any(|a| a.starts_with("arm")) {
        Abi::Arm
    } else {
        Abi::Other
    }
}

/// The best PNG among the icon's files: highest density first (`anydpi` files are XML).
fn icon<R: std::io::Read + std::io::Seek>(
    a: &mut zip::Archive<R>,
    files: Vec<(u16, String)>,
) -> Option<Vec<u8>> {
    let mut pngs: Vec<(u16, String)> = files
        .into_iter()
        .filter(|(d, p)| p.ends_with(".png") && *d < 0xfffe)
        .map(|(d, p)| (if d == 0 { 160 } else { d }, p))
        .collect();
    pngs.sort_by_key(|p| std::cmp::Reverse(p.0));
    for (_, path) in pngs {
        let Some(entry) = a.entry(&path).cloned() else {
            continue;
        };
        if let Ok(data) = a.read(&entry, MAX_ICON) {
            if data.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Some(data);
            }
        }
    }
    None
}

fn names(table: Option<&arsc::Table<'_>>, value: Option<&axml::Value>) -> Vec<(String, String)> {
    match (value, table) {
        (Some(axml::Value::Text(t)), _) => vec![(String::new(), t.clone())],
        (Some(axml::Value::Ref(r)), Some(t)) => t.strings(*r),
        _ => Vec::new(),
    }
}

/// Reads an APK.
pub fn analyze(file: std::fs::File) -> Option<Report> {
    let mut a = zip::Archive::open(std::io::BufReader::new(file)).ok()?;
    let manifest = a.entry("AndroidManifest.xml")?.clone();
    let m = axml::parse(&a.read(&manifest, axml::MAX_MANIFEST).ok()?).ok()?;
    if !package_ok(&m.package) {
        return None;
    }
    let arsc_bytes = a
        .entry("resources.arsc")
        .cloned()
        .and_then(|e| a.read(&e, arsc::MAX_TABLE).ok());
    let table = arsc_bytes
        .as_deref()
        .and_then(|b| arsc::Table::parse(b).ok());
    let labels = names(table.as_ref(), m.label.as_ref());
    let default = |list: &[(String, String)]| {
        list.iter()
            .find(|(l, _)| l.is_empty())
            .or(list.first())
            .map(|(_, s)| clean(s))
    };
    let label = default(&labels)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| m.package.clone());
    let version = default(&names(table.as_ref(), m.version_name.as_ref()))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| m.version_code.to_string());
    let icon_files = match (m.icon, table.as_ref()) {
        (Some(id), Some(t)) => t.files(id),
        _ => Vec::new(),
    };
    let entry_names: Vec<String> = a.entries.iter().map(|e| e.name.clone()).collect();
    let (sig, signers) = match sig::check(&mut a) {
        sig::Signed::Valid { scheme, signers } => (Sig::Valid(scheme), signers),
        sig::Signed::OldOnly => (Sig::OldOnly, Vec::new()),
        sig::Signed::Unsigned => (Sig::Unsigned, Vec::new()),
        sig::Signed::Invalid => (Sig::Invalid, Vec::new()),
    };
    Some(Report {
        labels: labels
            .iter()
            .filter(|(l, _)| ["ar", "en", "de"].contains(&l.as_str()))
            .map(|(l, s)| (l.clone(), clean(s)))
            .collect(),
        label,
        version: clean(&version),
        version_code: m.version_code,
        min_sdk: m.min_sdk,
        activity: m.launcher.map(|c| c.replace('.', "/")).unwrap_or_default(),
        abi: abi_of(&entry_names),
        sig,
        signers,
        permissions: m.permissions.iter().map(|p| clean(p)).collect(),
        icon: icon(&mut a, icon_files),
        package: m.package,
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    if !text.len().is_multiple_of(4) {
        return None;
    }
    for c in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &b in c {
            let v = match b {
                b'=' => {
                    pad += 1;
                    0
                }
                _ if pad > 0 => return None,
                _ => B64.iter().position(|&x| x == b)? as u32,
            };
            n = (n << 6) | v;
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad.min(2)]);
    }
    Some(out)
}

impl Report {
    /// The cell's answer.
    pub fn to_lines(&self) -> String {
        let mut out = format!("{HEADER}\npackage={}\nlabel={}\n", self.package, self.label);
        for (l, s) in &self.labels {
            out.push_str(&format!("label.{l}={s}\n"));
        }
        out.push_str(&format!(
            "version={}\ncode={}\nmin_sdk={}\nactivity={}\nabi={}\nsig={}\n",
            self.version,
            self.version_code,
            self.min_sdk,
            self.activity,
            self.abi.word(),
            self.sig.word()
        ));
        for (fp, cn) in &self.signers {
            out.push_str(&format!("signer={fp} {cn}\n"));
        }
        for p in &self.permissions {
            out.push_str(&format!("perm={p}\n"));
        }
        if let Some(icon) = &self.icon {
            out.push_str(&format!("icon={}\n", base64(icon)));
        }
        out.push_str("end\n");
        out
    }

    /// Reads (and checks again) the cell's answer.
    pub fn from_lines(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut r = Report {
            package: String::new(),
            label: String::new(),
            labels: Vec::new(),
            version: String::new(),
            version_code: 0,
            min_sdk: 0,
            activity: String::new(),
            abi: Abi::Other,
            sig: Sig::Invalid,
            signers: Vec::new(),
            permissions: Vec::new(),
            icon: None,
        };
        let mut ended = false;
        for line in lines {
            if line == "end" {
                ended = true;
                break;
            }
            let (key, value) = line.split_once('=')?;
            if key != "icon" && (value.len() > 2 * MAX_TEXT || value.chars().any(char::is_control))
            {
                return None;
            }
            match key {
                "package" => r.package = value.to_owned(),
                "label" => r.label = value.to_owned(),
                "label.ar" | "label.en" | "label.de" if r.labels.len() < 3 => {
                    r.labels.push((key[6..].to_owned(), value.to_owned()));
                }
                "version" => r.version = value.to_owned(),
                "code" => r.version_code = value.parse().ok()?,
                "min_sdk" => r.min_sdk = value.parse().ok()?,
                "activity" => r.activity = value.to_owned(),
                "abi" => r.abi = Abi::from_word(value)?,
                "sig" => r.sig = Sig::from_word(value)?,
                "signer" if r.signers.len() < 8 => {
                    let (fp, cn) = value.split_once(' ').unwrap_or((value, ""));
                    let fp_ok = fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit());
                    if !fp_ok {
                        return None;
                    }
                    r.signers.push((fp.to_owned(), cn.to_owned()));
                }
                "perm" if r.permissions.len() < 128 => r.permissions.push(value.to_owned()),
                "icon" if value.len() <= MAX_ICON * 4 / 3 + 4 => {
                    r.icon = Some(unbase64(value)?).filter(|i| i.starts_with(b"\x89PNG\r\n\x1a\n"));
                }
                _ => return None,
            }
        }
        let activity_fine = r.activity.is_empty() || activity_ok(&r.activity);
        let sig_fine = matches!(r.sig, Sig::Valid(_)) == !r.signers.is_empty();
        (ended && package_ok(&r.package) && activity_fine && sig_fine).then_some(r)
    }

    /// Why this app cannot be installed here (`None`: it can).
    pub fn refusal(&self) -> Option<ultar_bus::messages::foreign::Refusal> {
        use ultar_bus::messages::foreign::Refusal as R;
        match (self.sig, self.abi) {
            (_, Abi::Arm) => Some(R::NeedsArm),
            (_, Abi::Other) => Some(R::NeedsOtherProcessor),
            (Sig::OldOnly, _) => Some(R::OldSignature),
            (Sig::Unsigned, _) => Some(R::Unsigned),
            (Sig::Invalid, _) => Some(R::BadSignature),
            (Sig::Valid(_), _) if self.activity.is_empty() => Some(R::NoStartScreen),
            (Sig::Valid(_), _) => None,
        }
    }

    /// The fingerprints of its signers.
    pub fn fingerprints(&self) -> Vec<String> {
        self.signers.iter().map(|(fp, _)| fp.clone()).collect()
    }
}

/// Programs of the service that run in-process inside a cell (after the cell confined
/// itself), never a program of the area.
pub mod internal {
    /// Prefix of their names in a cell job.
    pub const PREFIX: &str = "/.ultar-internal/";
    /// Reads an APK: `apk-inspect <file>`.
    pub const INSPECT: &str = "/.ultar-internal/apk-inspect";
    /// Puts an APK into the area: `apk-place <file> <package>`.
    pub const PLACE: &str = "/.ultar-internal/apk-place";
    /// Takes an app out of the area: `apk-remove <package>`.
    pub const REMOVE: &str = "/.ultar-internal/apk-remove";
}

/// Runs an internal program (inside the cell). Returns the exit code.
pub fn run_internal(program: &[String]) -> i32 {
    match (
        program.first().map(String::as_str),
        program.get(1),
        program.get(2),
    ) {
        (Some(internal::INSPECT), Some(file), None) => inspect_here(Path::new(file)),
        (Some(internal::PLACE), Some(file), Some(pkg)) if program.len() == 3 => {
            place_here(Path::new(file), pkg)
        }
        (Some(internal::REMOVE), Some(pkg), None) => remove_here(pkg),
        _ => 2,
    }
}

fn inspect_here(file: &Path) -> i32 {
    let Some(report) = std::fs::File::open(file).ok().and_then(analyze) else {
        return 3;
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(report.to_lines().as_bytes()).is_err() {
        return 4;
    }
    0
}

/// `lib/x86_64/<name>.so` → `<name>.so`, when the name is a plain file name.
fn lib_name(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix("lib/x86_64/")?;
    let fine = name.ends_with(".so")
        && name.len() <= MAX_TEXT
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c));
    fine.then_some(name)
}

fn place_here(file: &Path, package: &str) -> i32 {
    use std::os::unix::fs::PermissionsExt;
    if !package_ok(package) {
        return 2;
    }
    let Ok(f) = std::fs::File::open(file) else {
        return 3;
    };
    // the copy must be the app that was confirmed: same package, verified signature
    match analyze(f) {
        Some(r) if r.package == package && r.refusal().is_none() => {}
        _ => return 5,
    }
    let dir = Path::new(APPS_DIR).join(package);
    let _ = std::fs::remove_dir_all(&dir);
    let lib = dir.join("lib");
    if std::fs::create_dir_all(&lib).is_err() || std::fs::copy(file, dir.join("base.apk")).is_err()
    {
        return 6;
    }
    let Ok(mut a) = std::fs::File::open(file)
        .map(std::io::BufReader::new)
        .map_err(|_| ())
        .and_then(|r| zip::Archive::open(r).map_err(|_| ()))
    else {
        return 6;
    };
    let libs: Vec<zip::Entry> = a
        .entries
        .iter()
        .filter(|e| lib_name(&e.name).is_some())
        .cloned()
        .collect();
    if libs.len() > MAX_LIBS || libs.iter().map(|e| e.size).sum::<u64>() > MAX_LIBS_TOTAL {
        let _ = std::fs::remove_dir_all(&dir);
        return 7;
    }
    for e in libs {
        let Some(name) = lib_name(&e.name) else {
            continue;
        };
        let ok = a
            .read(&e, MAX_LIB)
            .ok()
            .is_some_and(|data| std::fs::write(lib.join(name), data).is_ok());
        if !ok {
            let _ = std::fs::remove_dir_all(&dir);
            return 7;
        }
    }
    for (path, mode) in [
        (dir.clone(), 0o755),
        (lib, 0o755),
        (dir.join("base.apk"), 0o644),
    ] {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    0
}

fn remove_here(package: &str) -> i32 {
    if !package_ok(package) {
        return 2;
    }
    match std::fs::remove_dir_all(Path::new(APPS_DIR).join(package)) {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            package: "org.example.calc".into(),
            label: "Calculator".into(),
            labels: vec![("ar".into(), "آلة حاسبة".into())],
            version: "1.4.2".into(),
            version_code: 42,
            min_sdk: 21,
            activity: "org/example/calc/Main".into(),
            abi: Abi::Java,
            sig: Sig::Valid(2),
            signers: vec![("ab".repeat(32), "Jane".into())],
            permissions: vec!["android.permission.INTERNET".into()],
            icon: Some(b"\x89PNG\r\n\x1a\nrest".to_vec()),
        }
    }

    #[test]
    fn a_report_survives_its_lines_and_odd_answers_are_refused() {
        let r = report();
        assert_eq!(Report::from_lines(&r.to_lines()), Some(r.clone()));
        let lines = r.to_lines();
        for (from, to) in [
            ("package=org.example.calc", "package=../etc"),
            ("activity=org/example/calc/Main", "activity=../../x"),
            ("sig=v2", "sig=v9"),
            ("\nend\n", "\n"),
            ("abi=java", "abi=sparc"),
            ("code=42", "code=x"),
        ] {
            assert!(
                Report::from_lines(&lines.replacen(from, to, 1)).is_none(),
                "{to}"
            );
        }
        let unsigned = lines.replacen("sig=v2", "sig=none", 1);
        assert!(
            Report::from_lines(&unsigned).is_none(),
            "signers without a signature"
        );
    }

    #[test]
    fn refusals_follow_processor_and_signature() {
        use ultar_bus::messages::foreign::Refusal as R;
        let mut r = report();
        assert_eq!(r.refusal(), None);
        r.abi = Abi::Arm;
        assert_eq!(r.refusal(), Some(R::NeedsArm));
        r.abi = Abi::X86_64;
        r.sig = Sig::OldOnly;
        assert_eq!(r.refusal(), Some(R::OldSignature));
        r.sig = Sig::Valid(3);
        r.activity.clear();
        assert_eq!(r.refusal(), Some(R::NoStartScreen));
    }

    #[test]
    fn names_and_libraries() {
        assert!(package_ok("org.happysanta.gd"));
        for bad in ["org", "org..x", "1org.x", "org.x/y", "org.x-y", ""] {
            assert!(!package_ok(bad), "{bad}");
        }
        assert_eq!(lib_name("lib/x86_64/libgame.so"), Some("libgame.so"));
        for bad in [
            "lib/arm64-v8a/libx.so",
            "lib/x86_64/../x.so",
            "lib/x86_64/a/b.so",
            "lib/x86_64/.so",
            "lib/x86_64/x.txt",
        ] {
            assert_eq!(lib_name(bad), None, "{bad}");
        }
        let names = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(abi_of(&names(&["classes.dex"])), Abi::Java);
        assert_eq!(
            abi_of(&names(&["lib/arm64-v8a/a.so", "lib/x86_64/a.so"])),
            Abi::X86_64
        );
        assert_eq!(
            abi_of(&names(&["lib/arm64-v8a/a.so", "lib/armeabi-v7a/a.so"])),
            Abi::Arm
        );
        assert_eq!(abi_of(&names(&["lib/x86/a.so"])), Abi::Other);
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..20 {
            let data: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
            assert_eq!(unbase64(&base64(&data)), Some(data));
        }
        assert_eq!(unbase64("abc"), None);
        assert_eq!(unbase64("a=bc"), None);
    }

    #[test]
    fn a_whole_apk_is_read() {
        use crate::apk::zip::tests::build;
        let manifest = axml::tests::sample();
        let table = arsc::tests::sample(
            "Calculator",
            "آلة حاسبة",
            &[
                (240, "res/mipmap-hdpi/ic.png"),
                (640, "res/mipmap-xxxhdpi/ic.png"),
            ],
        );
        let big_icon = b"\x89PNG\r\n\x1a\nbig".to_vec();
        let apk = build(
            &[
                ("AndroidManifest.xml", &manifest),
                ("resources.arsc", &table),
                ("res/mipmap-hdpi/ic.png", b"\x89PNG\r\n\x1a\nsmall"),
                ("res/mipmap-xxxhdpi/ic.png", &big_icon),
                ("lib/x86_64/libx.so", b"\x7fELF"),
                ("classes.dex", b"dex"),
            ],
            true,
        );
        let path = std::env::temp_dir().join(format!("apk-{}.apk", std::process::id()));
        std::fs::write(&path, &apk).unwrap();
        let r = analyze(std::fs::File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(r.package, "org.example.calc");
        assert_eq!(r.label, "Calculator");
        assert_eq!(r.labels, vec![("ar".to_owned(), "آلة حاسبة".to_owned())]);
        assert_eq!(r.version, "1.4.2");
        assert_eq!(r.activity, "org/example/calc/Main");
        assert_eq!(r.abi, Abi::X86_64);
        assert_eq!(r.sig, Sig::Unsigned);
        assert_eq!(r.icon, Some(big_icon));
        assert_eq!(
            r.refusal(),
            Some(ultar_bus::messages::foreign::Refusal::Unsigned)
        );
        assert_eq!(Report::from_lines(&r.to_lines()), Some(r));
        // signed: nothing refused, the signer is known
        let signed = crate::apk::testsign::resign(&apk, "Ultar Test A").unwrap();
        std::fs::write(&path, &signed).unwrap();
        let r = analyze(std::fs::File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(r.sig, Sig::Valid(2));
        assert_eq!(r.refusal(), None);
        assert_eq!(r.signers.len(), 1);
        assert_eq!(r.signers[0].1, "Ultar Test A");
        assert_eq!(Report::from_lines(&r.to_lines()), Some(r));
    }
}
