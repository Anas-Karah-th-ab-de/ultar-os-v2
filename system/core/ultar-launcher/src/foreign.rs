//! Apps from other systems (`docs/FREMD-APPS.md`): their manifest and their cell.
//!
//! Such an app is a program of the shared **Linux area** (later Wine and the Android
//! layer inside it) that nobody here checked. The service for such apps
//! (`ultar-foreign`) writes its manifest after the person confirmed the
//! installation with an administrator's password and registers it through
//! `launcher.install_foreign` – never by a signature. The launcher gives it the
//! profile `app.foreign` and a cell that differs from a native one only in what it
//! sees:
//!
//! * `/` is the Linux area **read-only** (`nosuid`, `nodev`) instead of the system's
//!   directories; nothing of the Ultar system is visible but the compositor's socket
//!   directory (for the Wayland bridge);
//! * no bus socket, no service role, no kernel right, no publish directory;
//! * devices: only the harmless nodes, each bound alone (never the kernel's device
//!   tree);
//! * network: without the permission a network namespace (own loopback only); with
//!   it the kernel's barrier "internet only" (never the device, its loopback or – unless
//!   the person allowed it – the local network);
//! * a start without any of these features is refused, whatever the operator's
//!   `accept_missing` says: there is no unsafe mode for apps from other systems.

use crate::appid::AppId;
use crate::manifest::{normalised_absolute, Manifest, ManifestError, Program, Requested, MAX_TEXT};
use crate::perm::{Permission, PermissionSet};
use serde::Deserialize;

/// Prefix of the ids of apps from other systems (no store key may be named so).
pub const PREFIX: &str = "foreign";
/// The kinds this version starts: Linux programs, Windows programs (Wine in the Linux area);
/// `compat-apk` follows in a later stage.
pub const KINDS: [&str; 2] = ["compat-deb", "compat-exe"];
/// What the cell sees of the Linux area: these entries of its root, read-only;
/// a top-level link (merged `/usr`: `bin -> usr/bin`) is made again as a link.
pub const RUNTIME_ENTRIES: [&str; 10] = [
    "usr", "bin", "sbin", "lib", "lib32", "lib64", "libx32", "etc", "opt", "var",
];
/// Where the bridge directory appears in the cell.
pub const BRIDGE_IN_CELL: &str = "/.ultar";
/// Where a Windows program's own installer directory appears in its cell (read-only).
pub const SETUP_IN_CELL: &str = "/.setup";
/// The Wayland bridge's program name in the bridge directory.
pub const BRIDGE_PROGRAM: &str = "ultar-wayland";
/// `XDG_RUNTIME_DIR` of the app (in its private `/tmp`; the bridge makes it `0700`).
pub const RUNTIME_DIR: &str = "/tmp/.run";
/// Name of the Wayland socket the bridge offers.
pub const WAYLAND_DISPLAY: &str = "wayland-0";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    id: String,
    version: String,
    kind: String,
    exec: String,
    #[serde(default)]
    args: Vec<String>,
    /// Name of its entry (data of others; the service lists it, the launcher does not).
    #[serde(default)]
    name: String,
    /// The package that brought it.
    #[serde(default)]
    package: String,
    #[serde(default)]
    permissions: RawPermissions,
    #[serde(default)]
    limits: RawLimits,
    #[serde(default)]
    foreign: RawForeign,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
    network: Option<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    memory_mb: Option<u32>,
    pids: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawForeign {
    #[serde(default)]
    lan: bool,
}

/// The installer directory of the foreign app `id` below `root` (`ultar-foreign --root`/setup):
/// a plain directory named exactly like the app – never a link, never another app's.
pub fn setup_dir(root: &std::path::Path, id: &AppId) -> Option<std::path::PathBuf> {
    if !is_foreign_id(id) {
        return None;
    }
    let dir = root.join(id.as_str());
    let meta = std::fs::symlink_metadata(&dir).ok()?;
    (meta.file_type().is_dir()).then_some(dir)
}

/// Whether `id` is one of an app from another system: `foreign.<rest>`.
pub fn is_foreign_id(id: &AppId) -> bool {
    id.as_str()
        .split_once('.')
        .is_some_and(|(prefix, rest)| prefix == PREFIX && !rest.is_empty())
}

fn text_ok(text: &str) -> bool {
    text.len() <= MAX_TEXT && !text.chars().any(char::is_control)
}

fn network(word: Option<&str>) -> Result<bool, ManifestError> {
    match word {
        None | Some("none") => Ok(false),
        Some("allowed") => Ok(true),
        Some(_) => Err(ManifestError::Permission),
    }
}

/// The person's folders the manifest names (`documents:rw` …, each known once).
fn folders(files: Vec<String>) -> Result<Vec<String>, ManifestError> {
    let known = crate::areas::declared(&files);
    if files.len() > 8 || known.len() != files.len() {
        return Err(ManifestError::Permission);
    }
    Ok(files)
}

fn requested(raw: &RawLimits) -> Result<Requested, ManifestError> {
    let in_range = |v: Option<u32>, max: u32| v.is_none_or(|v| (1..=max).contains(&v));
    if !in_range(raw.memory_mb, 1 << 20) || !in_range(raw.pids, 1 << 16) {
        return Err(ManifestError::Limit);
    }
    Ok(Requested {
        memory_mb: raw.memory_mb,
        pids: raw.pids,
    })
}

fn program(exec: String, args: Vec<String>) -> Result<Program, ManifestError> {
    let arg_ok = |a: &String| a.len() <= MAX_TEXT && !a.contains('\0');
    let below_bridge = exec == BRIDGE_IN_CELL
        || exec.starts_with("/.ultar/")
        || exec == SETUP_IN_CELL
        || exec.starts_with("/.setup/");
    if !normalised_absolute(&exec) || below_bridge || args.len() > 32 || !args.iter().all(arg_ok) {
        return Err(ManifestError::Program);
    }
    Ok(Program { exec, args })
}

/// Reads and checks the manifest of an app from another system.
pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    if bytes.len() > ultar_bus::messages::launcher::MAX_FOREIGN_MANIFEST {
        return Err(ManifestError::Size);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ManifestError::Size)?;
    let raw: Raw = toml::from_str(text).map_err(|_| ManifestError::Syntax)?;
    let id = AppId::new(&raw.id).map_err(|_| ManifestError::Id)?;
    if !is_foreign_id(&id) {
        return Err(ManifestError::Id);
    }
    if !KINDS.contains(&raw.kind.as_str()) {
        return Err(ManifestError::Unsupported);
    }
    let version_ok = !raw.version.is_empty()
        && raw.version.len() <= 32
        && raw
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b));
    if !version_ok {
        return Err(ManifestError::Version);
    }
    if !text_ok(&raw.name) || !text_ok(&raw.package) {
        return Err(ManifestError::Syntax);
    }
    let allowed = network(raw.permissions.network.as_deref())?;
    if raw.foreign.lan && !allowed {
        return Err(ManifestError::Permission);
    }
    let declared = if allowed {
        PermissionSet::EMPTY.with(Permission::Network)
    } else {
        PermissionSet::EMPTY
    };
    Ok(Manifest {
        id,
        version: raw.version,
        ui: program(raw.exec, raw.args)?,
        service: None,
        declared,
        network_allowed: allowed,
        bus: Vec::new(),
        files: folders(raw.permissions.files)?,
        kernel_rights: 0,
        limits: requested(&raw.limits)?,
        publisher: None,
        open_schemes: Vec::new(),
        multi_window: false,
        open_page_arg: None,
        lan: raw.foreign.lan,
    })
}

/// Sets the kernel's barrier "internet only" for this process and everything it starts
/// (`lan`: the local network as well). Stage P0 (`docs/FREMD-APPS.md`) provides the
/// kernel call; until then this refuses, and `plan.rs` never asks for it because the
/// platform reports the barrier absent (`Platform::net_barrier`).
pub fn set_net_barrier(lan: bool) -> Result<(), ultar_sys::Errno> {
    let _ = lan;
    Err(ultar_sys::Errno::ENOSYS)
}

/// The environment of an app from another system (added to the launcher's own:
/// `PATH`, `HOME=/data`, `TMPDIR`, the app id and the person's look).
pub fn environment(lang: &str) -> Vec<(String, String)> {
    let locale = match lang {
        "en" => "en_US.UTF-8",
        "de" => "de_DE.UTF-8",
        _ => "ar_SY.UTF-8",
    };
    [
        ("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/games"),
        ("XDG_RUNTIME_DIR", RUNTIME_DIR),
        ("WAYLAND_DISPLAY", WAYLAND_DISPLAY),
        ("XDG_SESSION_TYPE", "wayland"),
        ("GDK_BACKEND", "wayland"),
        ("QT_QPA_PLATFORM", "wayland"),
        ("SDL_VIDEODRIVER", "wayland"),
        ("MOZ_ENABLE_WAYLAND", "1"),
        ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ("NO_AT_BRIDGE", "1"),
        ("LANG", locale),
        ("LC_ALL", locale),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

/// The program a cell starts: the bridge with the app's program after `--`, or the
/// program alone when the system has no bridge (`[paths] foreign_bridge` unset: the app
/// then has no window, which is no risk – the bridge is about display only).
pub fn bridged(program: &Program, bridge: bool) -> Program {
    if !bridge {
        return program.clone();
    }
    let mut args = vec!["--".to_owned(), program.exec.clone()];
    args.extend(program.args.iter().cloned());
    Program {
        exec: format!("{BRIDGE_IN_CELL}/{BRIDGE_PROGRAM}"),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "id = \"foreign.deb-mousepad\"\nversion = \"0.6.2-1.u1\"\n\
        kind = \"compat-deb\"\nexec = \"/usr/bin/mousepad\"\nargs = [\"--new\"]\n\
        name = \"Mousepad\"\npackage = \"mousepad\"\n\
        [permissions]\nnetwork = \"allowed\"\nfiles = [\"documents:rw\"]\n\
        [foreign]\nlan = false\n";

    #[test]
    fn the_installer_directory_is_the_apps_own_and_never_a_link() {
        let root = std::env::temp_dir().join(format!("ulf-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("foreign.exe-notepad")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere"), root.join("foreign.exe-linked"))
            .unwrap();
        let id = |s: &str| AppId::new(s).unwrap();
        assert_eq!(
            setup_dir(&root, &id("foreign.exe-notepad")),
            Some(root.join("foreign.exe-notepad"))
        );
        assert_eq!(setup_dir(&root, &id("foreign.exe-linked")), None);
        assert_eq!(setup_dir(&root, &id("foreign.exe-missing")), None);
        assert_eq!(setup_dir(&root, &id("files")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_windows_program_is_a_kind_and_cannot_start_its_installer_directly() {
        let exe = GOOD.replace("compat-deb", "compat-exe");
        assert!(parse(exe.as_bytes()).is_ok());
        let direct = exe.replace("/usr/bin/mousepad", "/.setup/setup.exe");
        assert!(matches!(
            parse(direct.as_bytes()),
            Err(ManifestError::Program)
        ));
    }

    #[test]
    fn a_foreign_manifest_is_read() {
        let m = parse(GOOD.as_bytes()).unwrap();
        assert_eq!(m.id.as_str(), "foreign.deb-mousepad");
        assert!(m.network_allowed && m.declared.has(Permission::Network));
        assert!(m.bus.is_empty() && m.service.is_none() && m.kernel_rights == 0);
        assert_eq!(m.files, vec!["documents:rw".to_owned()]);
        assert!(!m.lan);
        let b = bridged(&m.ui, true);
        assert_eq!(b.exec, "/.ultar/ultar-wayland");
        assert_eq!(b.args, vec!["--", "/usr/bin/mousepad", "--new"]);
        assert_eq!(bridged(&m.ui, false), m.ui);
    }

    #[test]
    fn everything_else_is_refused() {
        let cases = [
            ("foreign.deb-mousepad", "acme.mousepad"),
            ("foreign.deb-mousepad", "mousepad"),
            ("compat-deb", "native"),
            ("compat-deb", "compat-apk"),
            ("/usr/bin/mousepad", "mousepad"),
            ("/usr/bin/mousepad", "/.ultar/ultar-wayland"),
            ("/usr/bin/mousepad", "/usr/../bin/sh"),
            ("network = \"allowed\"", "network = \"ask\""),
            ("documents:rw", "/etc:rw"),
            ("lan = false", "lan = false\nroot = true"),
            ("[foreign]", "bus = [\"x\"]\n[foreign]"),
        ];
        for (from, to) in cases {
            let text = GOOD.replacen(from, to, 1);
            assert!(parse(text.as_bytes()).is_err(), "{to}");
        }
        let lan_alone = GOOD
            .replace("network = \"allowed\"", "network = \"none\"")
            .replace("lan = false", "lan = true");
        assert_eq!(parse(lan_alone.as_bytes()), Err(ManifestError::Permission));
        let huge = format!("{GOOD}# {}\n", "x".repeat(20_000));
        assert_eq!(parse(huge.as_bytes()), Err(ManifestError::Size));
    }

    #[test]
    fn environment_names_the_locale() {
        let env = environment("de");
        assert!(env.contains(&("LANG".into(), "de_DE.UTF-8".into())));
        assert!(env.contains(&("WAYLAND_DISPLAY".into(), "wayland-0".into())));
        assert!(environment("xx").contains(&("LANG".into(), "ar_SY.UTF-8".into())));
    }
}
