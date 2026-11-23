//! Preparing and starting one service: its directories, its memory group,
//! the readiness and output pipes, and the launcher (`launch.rs`).

use crate::groups::GroupTree;
use crate::launch::{command, Plan};
use crate::spec::{ServiceSpec, ALL_RIGHTS};
use crate::unit::{OutputPipe, ReadyPipe, Run};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use ultar_service::log::output::Stream;
use ultar_service::READY_FD_ENV;

/// The compatibility runner (services with `compat = true`).
pub const COMPAT_BIN: &str = "/usr/bin/ultar-compat";
/// Group whose processes may reach the internet although the device is
/// "offline" (as `ultar_zellen::regeln::NETZ_GID`).
pub const NET_GID: u32 = 47900;
/// Search path of every service.
const PATH: &str = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// What init knows about the machine it starts services on.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Program executed as launcher (init itself).
    pub launcher: PathBuf,
    /// The memory group tree, if the kernel has one.
    pub groups: GroupTree,
    /// Whether the kernel has rights to drop (the Ultar kernel).
    pub kernel_rights: bool,
    /// Whether init runs as root (only then can it switch ids).
    pub root: bool,
    /// Default deadline between stop signal and hard end.
    pub stop_timeout: Duration,
    /// Edition word passed to every service.
    pub edition: String,
    /// File naming services that may use the network group.
    pub net_allowed: Option<PathBuf>,
    /// Services the administrator allows to keep `all` rights
    /// (`all_rights_allowed` in `init.toml`); every other file asking for
    /// `all` is refused.
    pub all_rights_allowed: Vec<String>,
}

/// Why a service could not be started (word for records, detail for the
/// operator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    /// Stable lower-case word.
    pub word: &'static str,
    /// Detail.
    pub detail: String,
}

fn error(word: &'static str, detail: impl ToString) -> StartError {
    StartError {
        word,
        detail: detail.to_string(),
    }
}

fn net_allowed(env: &Environment, name: &str) -> bool {
    env.net_allowed.as_ref().is_some_and(|file| {
        std::fs::read_to_string(file)
            .map(|text| text.lines().any(|line| line.trim() == name))
            .unwrap_or(false)
    })
}

/// The ids the launcher switches to; `None`: keep init's.
fn identity(
    spec: &ServiceSpec,
    env: &Environment,
) -> Result<Option<ultar_sys::Identity>, StartError> {
    let confinement = &spec.confinement;
    if env.root {
        let uid = confinement.uid.unwrap_or(0);
        let mut gid = confinement.gid.unwrap_or(uid);
        if confinement.uid.is_none() && net_allowed(env, &spec.name) {
            gid = NET_GID;
        }
        return Ok(Some(ultar_sys::Identity {
            uid,
            gid,
            groups: confinement.groups.clone(),
        }));
    }
    // Not root (development): only init's own ids are possible.
    let own_uid = ultar_sys::euid();
    let wanted_uid = confinement.uid.unwrap_or(own_uid);
    let wanted_gid = confinement.gid.unwrap_or(ultar_sys::gid());
    if wanted_uid != own_uid || wanted_gid != ultar_sys::gid() || !confinement.groups.is_empty() {
        return Err(error("identity_needs_root", wanted_uid));
    }
    Ok(None)
}

/// Most entries below a service directory handed to the service (state of
/// releases that ran the service as root).
const ADOPT_LIMIT: usize = 10_000;
/// Deepest level below a service directory that is handed over.
const ADOPT_DEPTH: usize = 8;

/// Hands what lies below `dir` to `uid`/`gid` where it belongs to someone
/// else (a service that ran as root before). Links are changed themselves,
/// never followed; at most [`ADOPT_LIMIT`] entries, [`ADOPT_DEPTH`] levels.
fn adopt_tree(dir: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let mut pending = vec![(dir.to_owned(), 0)];
    let mut seen = 0;
    while let Some((current, depth)) = pending.pop() {
        for entry in std::fs::read_dir(&current)? {
            let path = entry?.path();
            seen += 1;
            if seen > ADOPT_LIMIT {
                return Ok(());
            }
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.uid() != uid || meta.gid() != gid {
                std::os::unix::fs::lchown(&path, Some(uid), Some(gid))?;
            }
            if meta.is_dir() && depth < ADOPT_DEPTH {
                pending.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

/// Creates the service's directories, owned by it: `dirs` private,
/// `public_dirs` open for passing through.
fn prepare_dirs(
    spec: &ServiceSpec,
    env: &Environment,
    uid: u32,
    gid: u32,
) -> Result<(), StartError> {
    let private = spec.confinement.dirs.iter().map(|dir| (dir, 0o700));
    let public = spec.confinement.public_dirs.iter().map(|dir| (dir, 0o755));
    for (dir, mode) in private.chain(public) {
        std::fs::create_dir_all(dir)
            .map_err(|e| error("dir", format!("{}: {e}", dir.display())))?;
        if env.root {
            std::os::unix::fs::chown(dir, Some(uid), Some(gid))
                .and_then(|()| adopt_tree(dir, uid, gid))
                .map_err(|e| error("dir", format!("{}: {e}", dir.display())))?;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
            .map_err(|e| error("dir", format!("{}: {e}", dir.display())))?;
    }
    Ok(())
}

fn console() -> Result<(Stdio, Stdio, Stdio), StartError> {
    let open = |write: bool| {
        std::fs::OpenOptions::new()
            .read(!write)
            .write(write)
            .open(ultar_sys::machine::CONSOLE)
            .map(Stdio::from)
            .map_err(|e| error("console", e))
    };
    Ok((open(false)?, open(true)?, open(true)?))
}

/// Starts `spec`; returns its run.
pub fn start(spec: &ServiceSpec, env: &Environment, now: Instant) -> Result<Run, StartError> {
    if spec.confinement.all_rights && !env.all_rights_allowed.contains(&spec.name) {
        return Err(error("rights_all_refused", "no administrator override"));
    }
    let identity = identity(spec, env)?;
    let (uid, gid) = identity
        .as_ref()
        .map_or((ultar_sys::euid(), ultar_sys::gid()), |id| (id.uid, id.gid));
    prepare_dirs(spec, env, uid, gid)?;
    let confinement = &spec.confinement;
    let group = env
        .groups
        .prepare(confinement.slice, &spec.name, &confinement.limits)
        .map_err(|e| error("group", e))?;
    let (exec, args) = if spec.program.compat {
        let mut args = vec!["run".to_owned(), "--".to_owned(), spec.program.exec.clone()];
        args.extend(spec.program.args.iter().cloned());
        (COMPAT_BIN.to_owned(), args)
    } else {
        (spec.program.exec.clone(), spec.program.args.clone())
    };
    let plan = Plan {
        exec,
        args,
        identity,
        drop_rights: env
            .kernel_rights
            .then_some(!(confinement.rights & ALL_RIGHTS)),
        group_dir: group.as_ref().map(|prepared| prepared.dir.clone()),
        workdir: spec.program.workdir.clone(),
        console: spec.program.console,
    };
    spawn(spec, env, &plan, uid, now).map(|mut run| {
        run.group = group.map(|prepared| prepared.dir);
        run
    })
}

/// Sets up streams and environment, spawns the launcher.
fn spawn(
    spec: &ServiceSpec,
    env: &Environment,
    plan: &Plan,
    uid: u32,
    now: Instant,
) -> Result<Run, StartError> {
    let mut launcher = command(&env.launcher, plan);
    launcher
        .env_clear()
        .env("PATH", PATH)
        .env("HOME", if uid == 0 { "/root" } else { "/" })
        .env("ULTAR_SERVICE", &spec.name)
        .env("ULTAR_EDITION", &env.edition)
        .envs(&spec.program.env);
    let mut outputs = Vec::new();
    let mut writers = Vec::new();
    if spec.program.console {
        let (input, out, err) = console()?;
        launcher.stdin(input).stdout(out).stderr(err);
    } else {
        launcher.stdin(Stdio::null());
        for stream in [Stream::Out, Stream::Err] {
            let (reader, writer) = std::io::pipe().map_err(|e| error("pipe", e))?;
            outputs.push(OutputPipe::new(reader, stream));
            writers.push(writer);
        }
        let mut writers = writers.drain(..);
        if let (Some(out), Some(err)) = (writers.next(), writers.next()) {
            launcher.stdout(out).stderr(err);
        }
    }
    let ready = if spec.life.notify {
        let (reader, writer) = std::io::pipe().map_err(|e| error("pipe", e))?;
        // Inherited by the launcher and through it by the service; init's
        // copy is closed right after the start (dropped with `writer`).
        ultar_sys::set_inherited(&writer, true).map_err(|e| error("pipe", e))?;
        launcher.env(READY_FD_ENV, writer.as_raw_fd().to_string());
        Some((reader, writer))
    } else {
        None
    };
    // `std` may start the launcher with `posix_spawn` (vfork + execve): the
    // kernel lets init go on at the launcher's `execve` (weckertest checks it).
    let child = launcher.spawn().map_err(|e| error("spawn", e))?;
    // The launcher is reaped by `waitpid(-1)` in the main loop; the handle
    // only holds the id.
    let pid = child.id();
    drop(launcher);
    Ok(Run {
        pid,
        since: now,
        group: None,
        ready: ready.map(|(reader, _writer)| ReadyPipe::new(reader)),
        outputs,
    })
}

/// The launcher: init's own program.
pub fn own_program(configured: Option<&Path>) -> PathBuf {
    if let Some(path) = configured {
        return path.to_owned();
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/sbin/init"))
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
