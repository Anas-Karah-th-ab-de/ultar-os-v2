//! Service files: `/etc/ultar/services/*.toml` (and the services of
//! installed apps below `/data/ultar/services`).
//!
//! Every key is checked (`deny_unknown_fields`); a file that does not
//! parse, names an unknown right or an impossible identity is refused as a
//! whole and the service does not exist – a typo never widens what a
//! service may do.
//!
//! What a service runs as:
//!
//! | Key | Meaning | Default |
//! |---|---|---|
//! | `user`, `group` | user and group id | init's own (root); `group` defaults to `user` |
//! | `groups` | supplementary group ids (≤ 16) | none |
//! | `rights` | kernel rights the service keeps (names, see [`RIGHT_NAMES`]; `all` only with the administrator's override in `init.toml`) | none |
//! | `slice` | memory group parent: `system`, `session`, `apps` | `system` |
//! | `[cgroup]` | limits written into its group (`memory.max`, `pids.max`, …) | none |
//! | `dirs` | directories created for it, owned by it, mode `0700` | none |
//! | `public_dirs` | the same with mode `0755` (others may pass through to its sockets) | none |
//! | `notify` | started only when it reports `READY` | `false` |
//! | `start_timeout_ms`, `stop_timeout_ms` | deadlines for readiness and for a stop | 10 s, `shutdown_grace_ms` |

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Names of the kernel rights a service file may keep, in bit order
/// (`kernel/ultar-kern/src/isolation/rights.rs`).
pub const RIGHT_NAMES: [&str; 15] = [
    "mount",
    "net_admin",
    "load_driver",
    "kill_any",
    "set_time",
    "raw_device",
    "reboot",
    "manage_groups",
    "set_id",
    "file_admin",
    "bluetooth",
    "input_inject",
    "sound",
    "camera",
    "cell_chown",
];
/// Mask of every right the kernel knows.
pub const ALL_RIGHTS: u64 = 0x7fff;
/// Default deadline for `READY`.
pub const DEFAULT_START_TIMEOUT_MS: u64 = 10_000;
/// Longest accepted deadline (start or stop).
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// Most dependencies, directories or environment entries of one service.
pub const MAX_LIST: usize = 32;

/// When a service is started again after it ended.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    /// Always (with back-off).
    #[default]
    Always,
    /// Only after a failure (exit code ≠ 0 or a signal).
    OnFailure,
    /// Never.
    Never,
    /// Always, and never given up: a crash loop only stretches the pause to the longest
    /// back-off (the lock screen: a session must never stay locked without one, see
    /// docs/COMPOSITOR.md §8a).
    Essential,
}

/// Parent of the service's memory group.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Slice {
    /// System services (the kernel never refuses or ends them first).
    #[default]
    System,
    /// The user's session (the surface).
    Session,
    /// Apps.
    Apps,
}

impl Slice {
    /// Directory name in the group tree.
    pub const fn dir(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Session => "session",
            Self::Apps => "apps",
        }
    }
}

/// Every key a service file may have (checked before parsing: the parts
/// below are flattened, which `deny_unknown_fields` does not support).
pub const KEYS: [&str; 26] = [
    "name",
    "description",
    "exec",
    "args",
    "env",
    "workdir",
    "console",
    "compat",
    "restart",
    "after",
    "oneshot",
    "manual",
    "switch",
    "editions",
    "profiles",
    "notify",
    "start_timeout_ms",
    "stop_timeout_ms",
    "user",
    "group",
    "groups",
    "rights",
    "slice",
    "cgroup",
    "dirs",
    "public_dirs",
];

/// A service file as written.
#[derive(Debug, Deserialize, Clone)]
struct ServiceFile {
    #[serde(default)]
    name: String,
    #[serde(flatten)]
    program: Program,
    #[serde(flatten)]
    life: LifeFile,
    #[serde(flatten)]
    confine: ConfineFile,
}

/// What runs.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Program {
    /// Program.
    pub exec: String,
    /// Arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory.
    #[serde(default)]
    pub workdir: Option<String>,
    /// Standard streams on `/dev/console` instead of the log service.
    #[serde(default)]
    pub console: bool,
    /// Run in the Ubuntu compatibility area.
    #[serde(default)]
    pub compat: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct LifeFile {
    #[serde(default)]
    restart: Restart,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default)]
    oneshot: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    manual: bool,
    #[serde(default)]
    switch: Option<String>,
    #[serde(default)]
    editions: Vec<String>,
    #[serde(default)]
    profiles: Option<BTreeMap<String, ProfileStart>>,
    #[serde(default)]
    notify: bool,
    #[serde(default)]
    start_timeout_ms: Option<u64>,
    #[serde(default)]
    stop_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct ConfineFile {
    #[serde(default)]
    user: Option<u32>,
    #[serde(default)]
    group: Option<u32>,
    #[serde(default)]
    groups: Vec<u32>,
    #[serde(default)]
    rights: Vec<String>,
    #[serde(default)]
    slice: Slice,
    #[serde(default)]
    cgroup: BTreeMap<String, String>,
    #[serde(default)]
    dirs: Vec<String>,
    #[serde(default)]
    public_dirs: Vec<String>,
}

/// How a service starts in a boot profile (`[profiles]` of a service file).
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileStart {
    /// Started with the system.
    Start,
    /// Started only on request.
    Manual,
}

/// Longest `description` in bytes.
pub const MAX_DESCRIPTION: usize = 120;

/// The boot profile of an ordinary start.
pub const SYSTEM_PROFILE: &str = "system";
/// Most profiles one service file names.
pub const MAX_PROFILES: usize = 8;

/// When a service runs and how it is started and stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Life {
    /// Restart policy.
    pub restart: Restart,
    /// Services that must be running (or, one-shot, done) first.
    pub after: Vec<String>,
    /// Runs once; dependants start after it ended successfully.
    pub oneshot: bool,
    /// What the service does, for the log (`description`, at most
    /// [`MAX_DESCRIPTION`] bytes, no control characters): a one-shot step's
    /// start and end lines say it, so a short run reads as the step it is.
    pub description: Option<String>,
    /// Started only on request.
    pub manual: bool,
    /// The one system service that may switch it on and off for the person
    /// (`init.switch`, without administrator rights); `None`: nobody may.
    pub switch: Option<String>,
    /// Editions the service exists in (empty: all).
    pub editions: Vec<String>,
    /// Boot profiles the service exists in and whether it starts there by
    /// itself (`[profiles]`; without that table: only the profile
    /// [`SYSTEM_PROFILE`], started unless `manual`).
    pub profiles: BTreeMap<String, ProfileStart>,
    /// Readiness reported through the ready pipe.
    pub notify: bool,
    /// Deadline for readiness.
    pub start_timeout_ms: u64,
    /// Deadline between the stop signal and the hard end; `None`: the
    /// default of `init.toml`.
    pub stop_timeout_ms: Option<u64>,
}

/// Who a service runs as and what it may do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Confinement {
    /// User id; `None`: init's own.
    pub uid: Option<u32>,
    /// Group id; `None`: the user id if one is given, else init's own.
    pub gid: Option<u32>,
    /// Supplementary groups.
    pub groups: Vec<u32>,
    /// Kernel rights kept (bit mask); all others are dropped.
    pub rights: u64,
    /// The file asks for `all` rights: refused at start unless the
    /// administrator named the service in `all_rights_allowed`.
    pub all_rights: bool,
    /// Parent of its memory group.
    pub slice: Slice,
    /// Limits for its memory group.
    pub limits: BTreeMap<String, String>,
    /// Directories created for the service, private to it.
    pub dirs: Vec<PathBuf>,
    /// Directories created for the service that others may pass through.
    pub public_dirs: Vec<PathBuf>,
}

/// A checked service definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Name (file stem unless given).
    pub name: String,
    /// What runs.
    pub program: Program,
    /// When it runs.
    pub life: Life,
    /// Identity, rights, group, directories.
    pub confinement: Confinement,
}

/// Why a service file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// Not readable.
    Read(String),
    /// Not TOML or an unknown key.
    Syntax(String),
    /// A name that is not `a-z 0-9 - _` (1–32 characters).
    Name,
    /// An unknown right.
    Right(String),
    /// A value out of its bounds (named).
    Bound(&'static str),
    /// A path that is not absolute.
    Path,
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(detail) => write!(f, "unreadable: {detail}"),
            Self::Syntax(detail) => write!(f, "syntax: {detail}"),
            Self::Name => f.write_str("invalid name"),
            Self::Right(name) => write!(f, "unknown right {name}"),
            Self::Bound(what) => write!(f, "{what} out of bounds"),
            Self::Path => f.write_str("path not absolute"),
        }
    }
}

/// Whether `name` is a service name (also the name its output is stored
/// under in the log service).
pub fn valid_name(name: &str) -> bool {
    ultar_service::log::output::valid_name(name)
}

/// The rights mask of `names`.
pub fn rights_mask(names: &[String]) -> Result<u64, SpecError> {
    let mut mask = 0;
    for name in names {
        if name == "all" {
            mask |= ALL_RIGHTS;
            continue;
        }
        let bit = RIGHT_NAMES
            .iter()
            .position(|known| known == name)
            .ok_or_else(|| SpecError::Right(name.clone()))?;
        mask |= 1 << bit;
    }
    Ok(mask)
}

fn timeout(value: Option<u64>, what: &'static str) -> Result<Option<u64>, SpecError> {
    match value {
        Some(ms) if ms == 0 || ms > MAX_TIMEOUT_MS => Err(SpecError::Bound(what)),
        other => Ok(other),
    }
}

/// `switch` names the one bus identity that may switch the service: a system service,
/// never an app. Only a service that is started on request, runs as a user of its own
/// and holds no kernel right can be given away like that – so the call can never start,
/// stop or bring back anything that matters to the safety of the system.
fn check_switch(file: &ServiceFile) -> Result<(), SpecError> {
    let Some(owner) = &file.life.switch else {
        return Ok(());
    };
    let name = owner.strip_prefix("system.").unwrap_or("");
    let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let named = !name.is_empty() && name.len() <= 48 && name.chars().all(plain);
    let confine = &file.confine;
    let harmless = file.life.manual && confine.rights.is_empty() && confine.user.is_some();
    if named && harmless {
        Ok(())
    } else {
        Err(SpecError::Bound("switch"))
    }
}

fn check_lists(file: &ServiceFile) -> Result<(), SpecError> {
    let (life, confine) = (&file.life, &file.confine);
    let dirs = confine.dirs.len() + confine.public_dirs.len();
    if life.after.len() > MAX_LIST || dirs > MAX_LIST || file.program.env.len() > MAX_LIST {
        return Err(SpecError::Bound("list"));
    }
    if confine.groups.len() > ultar_sys::MAX_GROUPS {
        return Err(SpecError::Bound("groups"));
    }
    let relative = confine
        .dirs
        .iter()
        .chain(&confine.public_dirs)
        .any(|dir| !dir.starts_with('/'));
    if !file.program.exec.starts_with('/') || relative {
        return Err(SpecError::Path);
    }
    if life.after.iter().any(|name| !valid_name(name)) {
        return Err(SpecError::Name);
    }
    Ok(())
}

fn paths(list: Vec<String>) -> Vec<PathBuf> {
    list.into_iter().map(PathBuf::from).collect()
}

impl ServiceSpec {
    /// The spec as it runs in boot profile `profile` (`manual` set from the
    /// profile table); `None` when the service does not exist there.
    pub fn in_profile(mut self, profile: &str) -> Option<Self> {
        let start = *self.life.profiles.get(profile)?;
        self.life.manual = start == ProfileStart::Manual;
        Some(self)
    }

    /// Checks a parsed file; `stem` names the service when the file does not.
    fn from_file(file: ServiceFile, stem: &str) -> Result<Self, SpecError> {
        let name = if file.name.is_empty() {
            stem.to_owned()
        } else {
            file.name.clone()
        };
        if !valid_name(&name) {
            return Err(SpecError::Name);
        }
        check_lists(&file)?;
        check_switch(&file)?;
        let (life, confine) = (file.life, file.confine);
        let profiles = match life.profiles {
            // a profile table says everything about starting: `manual` beside it is a contradiction
            Some(_) if life.manual => return Err(SpecError::Bound("manual beside profiles")),
            Some(table) if table.is_empty() || table.len() > MAX_PROFILES => {
                return Err(SpecError::Bound("profiles"))
            }
            Some(table) if table.keys().any(|name| !valid_name(name)) => {
                return Err(SpecError::Name)
            }
            Some(table) => table,
            None => {
                let start = if life.manual {
                    ProfileStart::Manual
                } else {
                    ProfileStart::Start
                };
                BTreeMap::from([(SYSTEM_PROFILE.to_owned(), start)])
            }
        };
        let confinement = Confinement {
            uid: confine.user,
            gid: confine.group.or(confine.user),
            groups: confine.groups,
            rights: rights_mask(&confine.rights)?,
            all_rights: confine.rights.iter().any(|name| name == "all"),
            slice: confine.slice,
            limits: confine.cgroup,
            dirs: paths(confine.dirs),
            public_dirs: paths(confine.public_dirs),
        };
        let life = Life {
            restart: life.restart,
            after: life.after,
            oneshot: life.oneshot,
            description: match life.description {
                Some(d)
                    if d.is_empty()
                        || d.len() > MAX_DESCRIPTION
                        || d.chars().any(char::is_control) =>
                {
                    return Err(SpecError::Bound("description"))
                }
                d => d,
            },
            manual: profiles.get(SYSTEM_PROFILE) == Some(&ProfileStart::Manual),
            switch: life.switch,
            editions: life.editions,
            profiles,
            notify: life.notify,
            start_timeout_ms: timeout(life.start_timeout_ms, "start_timeout_ms")?
                .unwrap_or(DEFAULT_START_TIMEOUT_MS),
            stop_timeout_ms: timeout(life.stop_timeout_ms, "stop_timeout_ms")?,
        };
        Ok(Self {
            name,
            program: file.program,
            life,
            confinement,
        })
    }

    /// Parses service file text: unknown keys are refused first.
    pub fn parse(text: &str, stem: &str) -> Result<Self, SpecError> {
        let syntax = |error: toml::de::Error| SpecError::Syntax(error.message().to_owned());
        let table: toml::Table = text.parse().map_err(syntax)?;
        if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(SpecError::Syntax(format!("unknown key {key}")));
        }
        let file: ServiceFile = table.try_into().map_err(syntax)?;
        Self::from_file(file, stem)
    }

    /// Reads and parses the service file at `path`.
    pub fn load(path: &Path) -> Result<Self, SpecError> {
        let text =
            std::fs::read_to_string(path).map_err(|error| SpecError::Read(error.to_string()))?;
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::parse(&text, &stem)
    }
}

/// Reads every `*.toml` of `dirs` (in name order). Refused files are
/// returned with their error; a later file with the same name is refused.
pub fn load_dir(dirs: &[PathBuf]) -> (Vec<ServiceSpec>, Vec<(PathBuf, SpecError)>) {
    let mut paths: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let (mut specs, mut refused) = (Vec::<ServiceSpec>::new(), Vec::new());
    for path in paths {
        match ServiceSpec::load(&path) {
            Ok(spec) if specs.iter().any(|known| known.name == spec.name) => {
                refused.push((path, SpecError::Name));
            }
            Ok(spec) => specs.push(spec),
            Err(error) => refused.push((path, error)),
        }
    }
    (specs, refused)
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
