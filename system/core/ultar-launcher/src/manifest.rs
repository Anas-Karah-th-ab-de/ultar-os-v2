//! The app manifest (`docs/APP-RULES.md` §3) as the launcher reads it from
//! the **signed** bytes of a package: what to run, which permissions the
//! app declares, which limits it asks for.
//!
//! Hostile input: the size is bounded, every table refuses unknown keys
//! (a typo must not silently drop a restriction), every list and string is
//! bounded, program paths must be absolute and normalised.

use crate::appid::AppId;
use crate::perm::{kernel_rights, Permission, PermissionSet};
use serde::Deserialize;
use std::fmt;

/// Largest manifest in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// Most arguments of a program.
pub const MAX_ARGS: usize = 32;
/// Longest argument or path.
pub const MAX_TEXT: usize = 256;
/// Most bus calls or file areas a manifest declares.
pub const MAX_NAMES: usize = 64;

/// Why a manifest was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManifestError {
    /// Larger than [`MAX_BYTES`] or not UTF-8.
    Size,
    /// Not TOML, an unknown key or a value of the wrong type.
    Syntax,
    /// The id breaks the rules of [`AppId`].
    Id,
    /// Not a native app (`kind`); other kinds run in compatibility cells.
    NotNative,
    /// The program path or an argument breaks the rules.
    Program,
    /// The version is empty or has foreign characters.
    Version,
    /// An unknown permission or kernel right, or a name list too long.
    Permission,
    /// `network = "vpn-only"`: not enforceable yet.
    Unsupported,
    /// A limit of zero or out of range.
    Limit,
    /// An unknown restart policy.
    Restart,
}

impl ManifestError {
    /// Stable key.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Size => "launcher.manifest.size",
            Self::Syntax => "launcher.manifest.syntax",
            Self::Id => "launcher.manifest.id",
            Self::NotNative => "launcher.manifest.not_native",
            Self::Program => "launcher.manifest.program",
            Self::Version => "launcher.manifest.version",
            Self::Permission => "launcher.manifest.permission",
            Self::Unsupported => "launcher.manifest.unsupported",
            Self::Limit => "launcher.manifest.limit",
            Self::Restart => "launcher.manifest.restart",
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl std::error::Error for ManifestError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    id: String,
    version: String,
    kind: String,
    #[serde(default)]
    exec: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    permissions: RawPermissions,
    #[serde(default)]
    limits: RawLimits,
    service: Option<RawService>,
    #[serde(default)]
    open: RawOpen,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawOpen {
    #[serde(default)]
    schemes: Vec<String>,
    #[serde(default)]
    multi_window: bool,
}

/// Keys of the app rules that others read (store, shell, installer):
/// accepted here with their type checked, not used.
const FOREIGN_KEYS: [(&str, Kind); 11] = [
    ("publisher", Kind::Text),
    ("name", Kind::Text),
    ("description", Kind::Text),
    ("icon", Kind::Text),
    ("category", Kind::Text),
    ("min_os", Kind::Text),
    ("arch", Kind::Text),
    ("size", Kind::Number),
    ("form", Kind::Table),
    ("needs", Kind::Table),
    ("agent", Kind::Table),
];

/// Type of a foreign key.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Number,
    Table,
}

/// Removes the foreign keys from `table`, refusing one of the wrong type.
fn strip_foreign(table: &mut toml::Table) -> Result<(), ManifestError> {
    for (key, kind) in FOREIGN_KEYS {
        let ok = match (table.remove(key), kind) {
            (None, _) => true,
            (Some(toml::Value::String(_)), Kind::Text) => true,
            (Some(toml::Value::Integer(n)), Kind::Number) => n >= 0,
            (Some(toml::Value::Table(_)), Kind::Table) => true,
            (Some(_), _) => false,
        };
        if !ok {
            return Err(ManifestError::Syntax);
        }
    }
    Ok(())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
    #[serde(default)]
    bus: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    network: Option<String>,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    ui: Vec<String>,
    #[serde(default)]
    kernel: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    memory_mb: Option<u32>,
    pids: Option<u32>,
    cpu: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawService {
    exec: String,
    #[serde(default)]
    args: Vec<String>,
    restart: Option<String>,
}

/// When a background service is started again after it ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Restart {
    /// Never.
    Never,
    /// After a crash (signal or failure code).
    OnFailure,
    /// After every end that was not requested.
    Always,
}

impl Restart {
    /// Stable word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

/// A program of the app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Program {
    /// Absolute, normalised path (as the cell sees it).
    pub exec: String,
    /// Its arguments.
    pub args: Vec<String>,
}

/// The limits an app asks for (the profile caps and fills them).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Requested {
    /// Memory in MiB.
    pub memory_mb: Option<u32>,
    /// Processes and threads.
    pub pids: Option<u32>,
}

/// A checked manifest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Manifest {
    /// App id.
    pub id: AppId,
    /// Version text.
    pub version: String,
    /// The app's window program.
    pub ui: Program,
    /// The app's background service, if it has one.
    pub service: Option<(Program, Restart)>,
    /// Permissions the launcher enforces that the app declares.
    pub declared: PermissionSet,
    /// `network = "allowed"` (for system apps granted at installation).
    pub network_allowed: bool,
    /// Bus calls it declares.
    pub bus: Vec<String>,
    /// File areas it declares.
    pub files: Vec<String>,
    /// Kernel rights it asks for (bit mask).
    pub kernel_rights: u64,
    /// Limits it asks for.
    pub limits: Requested,
    /// The `publisher` the manifest names, if any.
    pub publisher: Option<String>,
    /// URI schemes the app opens (`[open] schemes`).
    pub open_schemes: Vec<String>,
    /// The app opens more than one window (`[open] multi_window`).
    pub multi_window: bool,
}

/// Whether `path` is absolute and normalised (no `.`/`..`/empty part).
pub fn normalised_absolute(path: &str) -> bool {
    path.len() <= MAX_TEXT
        && path.starts_with('/')
        && path.len() > 1
        && !path.contains('\0')
        && path[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn program(exec: String, args: Vec<String>) -> Result<Program, ManifestError> {
    let arg_ok = |a: &String| a.len() <= MAX_TEXT && !a.contains('\0');
    if !normalised_absolute(&exec) || args.len() > MAX_ARGS || !args.iter().all(arg_ok) {
        return Err(ManifestError::Program);
    }
    Ok(Program { exec, args })
}

fn name_list(names: Vec<String>) -> Result<Vec<String>, ManifestError> {
    let name_ok = |n: &String| {
        !n.is_empty()
            && n.len() <= 64
            && n.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".:_*-".contains(&b))
    };
    if names.len() > MAX_NAMES || !names.iter().all(name_ok) {
        return Err(ManifestError::Permission);
    }
    Ok(names)
}

fn declared(raw: &RawPermissions) -> Result<(PermissionSet, bool), ManifestError> {
    let mut set = PermissionSet::EMPTY;
    let allowed = match raw.network.as_deref() {
        None | Some("none") => false,
        Some("ask") => {
            set = set.with(Permission::Network);
            false
        }
        Some("allowed") => {
            set = set.with(Permission::Network);
            true
        }
        Some("vpn-only") => return Err(ManifestError::Unsupported),
        Some(_) => return Err(ManifestError::Permission),
    };
    if raw.background {
        set = set.with(Permission::Background);
    }
    for name in &raw.ui {
        match Permission::from_name(name) {
            Some(p) if p.is_ui() => set = set.with(p),
            _ => return Err(ManifestError::Permission),
        }
    }
    Ok((set, allowed))
}

fn version_ok(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 32
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
}

fn limits(raw: &RawLimits) -> Result<Requested, ManifestError> {
    let cpu_ok = raw
        .cpu
        .as_deref()
        .is_none_or(|c| ["foreground", "background", "frozen-when-hidden"].contains(&c));
    let in_range = |v: Option<u32>, max: u32| v.is_none_or(|v| (1..=max).contains(&v));
    if !cpu_ok || !in_range(raw.memory_mb, 1 << 20) || !in_range(raw.pids, 1 << 16) {
        return Err(ManifestError::Limit);
    }
    Ok(Requested {
        memory_mb: raw.memory_mb,
        pids: raw.pids,
    })
}

fn restart(word: Option<&str>) -> Result<Restart, ManifestError> {
    match word {
        None | Some("on-failure") => Ok(Restart::OnFailure),
        Some("never") => Ok(Restart::Never),
        Some("always") => Ok(Restart::Always),
        Some(_) => Err(ManifestError::Restart),
    }
}

impl Manifest {
    /// Reads and checks the manifest `bytes`.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_BYTES {
            return Err(ManifestError::Size);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| ManifestError::Size)?;
        let mut table: toml::Table = text.parse().map_err(|_| ManifestError::Syntax)?;
        let publisher = table
            .get("publisher")
            .and_then(|p| p.as_str())
            .map(str::to_owned);
        strip_foreign(&mut table)?;
        let raw: Raw = toml::Value::Table(table)
            .try_into()
            .map_err(|_| ManifestError::Syntax)?;
        let id = AppId::new(&raw.id).map_err(|_| ManifestError::Id)?;
        if raw.kind != "native" {
            return Err(ManifestError::NotNative);
        }
        if !version_ok(&raw.version) {
            return Err(ManifestError::Version);
        }
        let (declared, network_allowed) = declared(&raw.permissions)?;
        let service = match raw.service {
            Some(s) => Some((program(s.exec, s.args)?, restart(s.restart.as_deref())?)),
            None => None,
        };
        Ok(Self {
            id,
            version: raw.version,
            ui: program(raw.exec, raw.args)?,
            service,
            declared,
            network_allowed,
            bus: name_list(raw.permissions.bus)?,
            files: name_list(raw.permissions.files)?,
            kernel_rights: kernel_rights(&raw.permissions.kernel)
                .ok_or(ManifestError::Permission)?,
            limits: limits(&raw.limits)?,
            publisher,
            open_schemes: crate::open::schemes(raw.open.schemes)?,
            multi_window: raw.open.multi_window,
        })
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
