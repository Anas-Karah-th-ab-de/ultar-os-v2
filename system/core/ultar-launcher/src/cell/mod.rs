//! The cell an app runs in, and how it is built.
//!
//! The launcher never confines itself. It executes **itself** as a helper
//! (no `unsafe`, no `pre_exec`) in two stages:
//!
//! 1. `ultar-launcher --cell …` (outside the cell): moves into the app's
//!    memory group, creates the namespaces (the next child is pid 1 of the
//!    new pid namespace), makes its mounts private, starts stage 2, reports
//!    the app's process id, waits for it and reports how it ended.
//! 2. `ultar-launcher --cell-init …` (pid 1 of the cell): builds the view
//!    ([`view`]), switches the root, takes the app's user, forbids new
//!    privileges, drops every kernel right and starts the app as pid 2. It
//!    stays pid 1 (as the app's user, without rights): a pid 1 gets no
//!    default signal actions, an app must (its own `abort`, `SIGTERM`). It
//!    passes `SIGTERM` on, reaps every child and ends with the app – then
//!    the kernel ends everything else in the cell.
//!
//! Each step that fails ends the stage with its own code
//! ([`CellFailure`]) before the app ran: nothing starts half confined. The
//! stages report on a status pipe whose number is in [`STATUS_ENV`]
//! ([`StatusLine`]).

mod stage;
pub mod view;

pub use stage::{probe, stage_one, stage_two};

use crate::manifest::{normalised_absolute, Program};
use std::path::PathBuf;
use ultar_sys::cell::Namespaces;

/// First argument of stage 1.
pub const CELL_ARG: &str = "--cell";
/// First argument of stage 2.
pub const CELL_INIT_ARG: &str = "--cell-init";
/// First argument of the platform probe.
pub const PROBE_ARG: &str = "--probe";
/// Environment variable with the number of the status descriptor.
pub const STATUS_ENV: &str = "ULTAR_CELL_STATUS_FD";

/// Why a stage stopped before the app ran: its exit code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellFailure {
    /// The command line is not one the launcher writes.
    Arguments = 110,
    /// The memory group refused the process.
    Group = 111,
    /// The namespaces could not be created.
    Namespaces = 112,
    /// A mount of the view failed.
    Mounts = 113,
    /// The root could not be switched.
    Root = 114,
    /// The ids could not be switched (or did not read back).
    Identity = 115,
    /// New privileges or kernel rights could not be dropped.
    Rights = 116,
    /// Stage 2 could not be started.
    Spawn = 117,
    /// Limits, mask, signals or working directory.
    Setup = 118,
    /// The program could not be executed.
    Exec = 127,
}

impl CellFailure {
    /// Every failure.
    pub const ALL: [CellFailure; 10] = [
        Self::Arguments,
        Self::Group,
        Self::Namespaces,
        Self::Mounts,
        Self::Root,
        Self::Identity,
        Self::Rights,
        Self::Spawn,
        Self::Setup,
        Self::Exec,
    ];

    /// The failure with exit code `code`.
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|f| *f as i32 == code)
    }

    /// Stable word for records.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Arguments => "cell_arguments",
            Self::Group => "cell_group",
            Self::Namespaces => "cell_namespaces",
            Self::Mounts => "cell_mounts",
            Self::Root => "cell_root",
            Self::Identity => "cell_identity",
            Self::Rights => "cell_rights",
            Self::Spawn => "cell_spawn",
            Self::Setup => "cell_setup",
            Self::Exec => "cell_exec",
        }
    }
}

/// Where the cell's device nodes come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevMode {
    /// Only the harmless nodes, each bound into a private `/dev` (Linux).
    Binds,
    /// The kernel's device tree (Ultar kernel: raw devices need a right the
    /// app does not have, the other nodes are gated by their modes).
    DevTmpfs,
    /// No `/dev` (no cell).
    None,
}

impl DevMode {
    const fn word(self) -> &'static str {
        match self {
            Self::Binds => "binds",
            Self::DevTmpfs => "devtmpfs",
            Self::None => "none",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        [Self::Binds, Self::DevTmpfs, Self::None]
            .into_iter()
            .find(|d| d.word() == word)
    }
}

/// Everything the two stages do for one start.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CellPlan {
    /// User and group id of the app; `None`: keep the launcher's.
    pub identity: Option<(u32, u32)>,
    /// Memory group to move into.
    pub group_dir: Option<PathBuf>,
    /// Namespaces to create (none: no cell).
    pub namespaces: Namespaces,
    /// Mount point of the cell's root (a directory of the launcher).
    pub root: PathBuf,
    /// The app's data directory on the host.
    pub data: PathBuf,
    /// Host directories the cell sees read-only.
    pub view: Vec<PathBuf>,
    /// Host socket directories the cell sees read-only.
    pub shares: Vec<PathBuf>,
    /// Device nodes.
    pub dev: DevMode,
    /// Size of the private `/tmp`.
    pub tmp_bytes: u64,
    /// Kernel rights to drop (`None`: the kernel has none).
    pub drop_rights: Option<u64>,
    /// Process limit of the app's user.
    pub nproc: Option<u64>,
    /// The program.
    pub program: Program,
}

fn flags_word(ns: Namespaces) -> String {
    let parts = [
        (ns.mount, "mount"),
        (ns.pid, "pid"),
        (ns.net, "net"),
        (ns.ipc, "ipc"),
    ];
    let words: Vec<&str> = parts
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, w)| *w)
        .collect();
    if words.is_empty() {
        "none".to_owned()
    } else {
        words.join(",")
    }
}

fn parse_flags(word: &str) -> Option<Namespaces> {
    let mut ns = Namespaces::default();
    if word == "none" {
        return Some(ns);
    }
    for part in word.split(',') {
        let slot = match part {
            "mount" => &mut ns.mount,
            "pid" => &mut ns.pid,
            "net" => &mut ns.net,
            "ipc" => &mut ns.ipc,
            _ => return None,
        };
        if std::mem::replace(slot, true) {
            return None;
        }
    }
    Some(ns)
}

fn text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl CellPlan {
    /// The words after [`CELL_ARG`] / [`CELL_INIT_ARG`].
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some((uid, gid)) = self.identity {
            args.extend([
                "--uid".into(),
                uid.to_string(),
                "--gid".into(),
                gid.to_string(),
            ]);
        }
        if let Some(dir) = &self.group_dir {
            args.extend(["--group-dir".into(), text(dir)]);
        }
        args.extend(["--ns".into(), flags_word(self.namespaces)]);
        args.extend([
            "--root".into(),
            text(&self.root),
            "--data".into(),
            text(&self.data),
        ]);
        for dir in &self.view {
            args.extend(["--view".into(), text(dir)]);
        }
        for dir in &self.shares {
            args.extend(["--share".into(), text(dir)]);
        }
        args.extend(["--dev".into(), self.dev.word().into()]);
        args.extend(["--tmp".into(), self.tmp_bytes.to_string()]);
        if let Some(mask) = self.drop_rights {
            args.extend(["--drop".into(), format!("{mask:x}")]);
        }
        if let Some(n) = self.nproc {
            args.extend(["--nproc".into(), n.to_string()]);
        }
        args.push("--".into());
        args.push(self.program.exec.clone());
        args.extend(self.program.args.iter().cloned());
        args
    }

    /// Parses [`CellPlan::to_args`]; `None` for anything else.
    pub fn parse(words: &[String]) -> Option<Self> {
        let mut plan = CellPlan {
            identity: None,
            group_dir: None,
            namespaces: Namespaces::default(),
            root: PathBuf::new(),
            data: PathBuf::new(),
            view: Vec::new(),
            shares: Vec::new(),
            dev: DevMode::None,
            tmp_bytes: 0,
            drop_rights: None,
            nproc: None,
            program: Program {
                exec: String::new(),
                args: Vec::new(),
            },
        };
        let (mut uid, mut gid) = (None, None);
        let mut rest = words.iter();
        loop {
            let option = rest.next()?;
            if option == "--" {
                break;
            }
            let value = rest.next()?;
            let path = || normalised_absolute(value).then(|| PathBuf::from(value));
            match option.as_str() {
                "--uid" => uid = Some(value.parse::<u32>().ok()?),
                "--gid" => gid = Some(value.parse::<u32>().ok()?),
                "--group-dir" => plan.group_dir = Some(path()?),
                "--ns" => plan.namespaces = parse_flags(value)?,
                "--root" => plan.root = path()?,
                "--data" => plan.data = path()?,
                "--view" => plan.view.push(path()?),
                "--share" => plan.shares.push(path()?),
                "--dev" => plan.dev = DevMode::from_word(value)?,
                "--tmp" => plan.tmp_bytes = value.parse().ok()?,
                "--drop" => plan.drop_rights = Some(u64::from_str_radix(value, 16).ok()?),
                "--nproc" => plan.nproc = Some(value.parse().ok()?),
                _ => return None,
            }
        }
        plan.identity = match (uid, gid) {
            (Some(uid), Some(gid)) => Some((uid, gid)),
            (None, None) => None,
            _ => return None,
        };
        plan.program.exec = rest.next().filter(|e| normalised_absolute(e))?.clone();
        plan.program.args = rest.cloned().collect();
        let paths_set = plan.root.is_absolute() && plan.data.is_absolute();
        paths_set.then_some(plan)
    }
}

/// One line on the status pipe (at most 64 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusLine {
    /// Stage 1 started the app's process (its id as the launcher sees it).
    Cell(u32),
    /// Stage 2 started the app.
    Ready,
    /// A stage failed before the app ran.
    Fail(CellFailure),
    /// The app ended with this code.
    Exit(i32),
    /// A signal ended the app.
    Signal(i32),
}

impl StatusLine {
    /// The line's text (without line feed).
    pub fn render(self) -> String {
        match self {
            Self::Cell(pid) => format!("CELL {pid}"),
            Self::Ready => "READY".to_owned(),
            Self::Fail(failure) => format!("FAIL {}", failure as i32),
            Self::Exit(code) => format!("EXIT {code}"),
            Self::Signal(signal) => format!("SIGNAL {signal}"),
        }
    }

    /// Parses a line; `None` for anything else.
    pub fn parse(line: &str) -> Option<Self> {
        let (word, value) = line.split_once(' ').unwrap_or((line, ""));
        let number = || value.parse::<i32>().ok();
        match word {
            "CELL" => value.parse().ok().filter(|p| *p > 0).map(Self::Cell),
            "READY" if value.is_empty() => Some(Self::Ready),
            "FAIL" => number().and_then(CellFailure::from_code).map(Self::Fail),
            "EXIT" => number().map(Self::Exit),
            "SIGNAL" => number().filter(|s| (1..=64).contains(s)).map(Self::Signal),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
