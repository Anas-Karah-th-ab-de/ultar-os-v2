//! The cells in which the service lets the Linux area's own tools work – it never runs
//! them, nor parses a package's insides, itself.
//!
//! | Cell | Linux area | User | Network | Used for |
//! |---|---|---|---|---|
//! | `inspect` | read-only | `nobody` | none (own namespace) | `dpkg-deb --field` of a file |
//! | `install` | writable | root of the cell, every kernel right dropped | none, or the kernel's barrier "internet only" once it exists (stage P0) | `apt-get install`/`remove`, `upgrade` |
//!
//! Both: mount + pid namespaces, the area as `/` (`nosuid`, `nodev`), private `/run` and
//! `/tmp`, own `/proc`, only the harmless device nodes, the file in `/run/inbox`
//! (read-only), no new privileges. On Linux (development) the capability bounding set
//! is cut down to what the package manager needs inside the cell.
//!
//! Like the launcher, the service executes **itself** as a helper in two stages
//! (`--cell`: namespaces; `--cell-init`: pid 1, view, identity, rights, the program).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use ultar_sys::cell::{self as sys_cell, BindMode, Namespaces};

/// First argument of stage 1.
pub const CELL_ARG: &str = "--cell";
/// First argument of stage 2.
pub const CELL_INIT_ARG: &str = "--cell-init";
/// Where the inbox appears in the cell.
pub const INBOX_IN_CELL: &str = "/run/inbox";
/// `nobody`.
const NOBODY: u32 = 65534;
/// Most output kept from a cell.
const MAX_OUTPUT: usize = 256 * 1024;
/// Exit code of a stage that failed before the program ran.
pub const SETUP_FAILED: i32 = 120;
/// Exit codes `SETUP_STEP + n`: which step of building the cell failed (`Step` below), so
/// that a kernel without a feature says which one.
pub const SETUP_STEP: i32 = 140;

/// The steps of building a cell, in order (exit code [`SETUP_STEP`] + the number).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SetupStep {
    /// New mount, pid and network namespaces (stage 1).
    Namespaces = 0,
    /// Mounts made private.
    Private = 1,
    /// The area bound (read-only, or writable without set-id and devices).
    Area = 2,
    /// `tmpfs` on `/run` and `/tmp`.
    Tmpfs = 3,
    /// Own `/proc`.
    Proc = 4,
    /// Kernel switches below `/proc` read-only.
    ProcReadOnly = 5,
    /// The harmless device nodes.
    Devices = 6,
    /// The inbox read-only.
    Inbox = 7,
    /// Root switch and the old root detached.
    Root = 8,
    /// Capabilities cut (Linux).
    Capabilities = 9,
    /// Identity (nobody, root of the cell).
    Identity = 10,
    /// Every kernel right dropped (Ultar kernel).
    Rights = 11,
    /// No new privileges.
    NoNewPrivileges = 12,
}

impl SetupStep {
    /// All steps.
    pub const ALL: [SetupStep; 13] = [
        Self::Namespaces,
        Self::Private,
        Self::Area,
        Self::Tmpfs,
        Self::Proc,
        Self::ProcReadOnly,
        Self::Devices,
        Self::Inbox,
        Self::Root,
        Self::Capabilities,
        Self::Identity,
        Self::Rights,
        Self::NoNewPrivileges,
    ];

    /// The step an exit code names.
    pub fn of_code(code: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| SETUP_STEP + *s as i32 == code)
    }

    const fn code(self) -> i32 {
        SETUP_STEP + self as i32
    }
}

/// Which cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Read-only area, `nobody`.
    Inspect,
    /// Writable area, root of the cell.
    Install,
}

impl Mode {
    const fn word(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Install => "install",
        }
    }
}

/// One run of a program in a cell.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CellJob {
    /// Which cell.
    pub mode: Mode,
    /// The Linux area on the host.
    pub runtime: PathBuf,
    /// A host directory shown read-only at [`INBOX_IN_CELL`].
    pub inbox: Option<PathBuf>,
    /// The program and its arguments (as the cell sees them).
    pub program: Vec<String>,
}

impl CellJob {
    fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--mode".into(),
            self.mode.word().into(),
            "--runtime".into(),
            self.runtime.to_string_lossy().into_owned(),
        ];
        if let Some(dir) = &self.inbox {
            args.extend(["--inbox".into(), dir.to_string_lossy().into_owned()]);
        }
        args.push("--".into());
        args.extend(self.program.iter().cloned());
        args
    }

    fn parse(words: &[String]) -> Option<Self> {
        let (mut mode, mut runtime, mut inbox) = (None, None, None);
        let mut rest = words.iter();
        loop {
            let option = rest.next()?;
            if option == "--" {
                break;
            }
            let value = rest.next()?;
            let absolute = || Path::new(value).is_absolute().then(|| PathBuf::from(value));
            match option.as_str() {
                "--mode" => {
                    mode = Some(match value.as_str() {
                        "inspect" => Mode::Inspect,
                        "install" => Mode::Install,
                        _ => return None,
                    })
                }
                "--runtime" => runtime = Some(absolute()?),
                "--inbox" => inbox = Some(absolute()?),
                _ => return None,
            }
        }
        let program: Vec<String> = rest.cloned().collect();
        let first_ok = program.first().is_some_and(|p| p.starts_with('/'));
        first_ok.then_some(Self {
            mode: mode?,
            runtime: runtime?,
            inbox,
            program,
        })
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    /// Exit code (`None`: killed after the time ran out, or by a signal).
    pub code: Option<i32>,
    /// What the program wrote to its standard output (at most 256 KiB).
    pub output: Vec<u8>,
}

/// Runs `job` with `helper` (this program) and waits at most `limit`.
pub fn run(helper: &Path, job: &CellJob, limit: Duration) -> std::io::Result<Ran> {
    let mut child = Command::new(helper)
        .arg(CELL_ARG)
        .args(job.to_args())
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout).take(MAX_OUTPUT as u64).read_to_end(&mut out);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        out
    });
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = reader.join().unwrap_or_default();
    Ok(Ran { code, output })
}

/// Stage 1 (`--cell`): new mount, pid and network namespaces, then stage 2 as pid 1.
pub fn stage_one(words: &[String]) -> i32 {
    let _ = sys_cell::kill_with_parent();
    if CellJob::parse(words).is_none() {
        return SETUP_FAILED;
    }
    let ns = Namespaces {
        mount: true,
        pid: true,
        net: true,
        ipc: false,
    };
    if sys_cell::unshare(ns).is_err() {
        return SetupStep::Namespaces.code();
    }
    let Ok(me) = std::env::current_exe() else {
        return SETUP_FAILED;
    };
    match Command::new(me)
        .arg(CELL_INIT_ARG)
        .args(words)
        .env_clear()
        .status()
    {
        Ok(status) => status.code().unwrap_or(SETUP_FAILED),
        Err(_) => SETUP_FAILED,
    }
}

fn mount_point(root: &Path, inside: &str) -> Result<PathBuf, ()> {
    let at = root.join(inside.trim_start_matches('/'));
    std::fs::create_dir_all(&at).map_err(|_| ())?;
    Ok(at)
}

/// The area writable and executable (the installation cell): bind, then remount
/// without set-id programs and devices.
fn bind_writable(runtime: &Path) -> Result<(), ()> {
    use rustix::mount::{mount_bind, mount_remount, MountFlags};
    mount_bind(runtime, runtime).map_err(|_| ())?;
    mount_remount(
        runtime,
        MountFlags::BIND | MountFlags::NOSUID | MountFlags::NODEV,
        "",
    )
    .map_err(|_| ())
}

/// Only the harmless device nodes, each bound on its own (never the kernel's device tree;
/// a kernel that cannot bind a single node builds no cell). Nodes the kernel lacks (the
/// Ultar kernel has no `full`) are left out.
fn devices(root: &Path) -> Result<(), ()> {
    let dev = mount_point(root, "/dev")?;
    sys_cell::mount_tmpfs(&dev, "size=65536,mode=0755").map_err(|_| ())?;
    for node in sys_cell::DEV_NODES {
        let source = Path::new(sys_cell::DEV).join(node);
        if std::fs::symlink_metadata(&source).is_err() {
            continue;
        }
        let at = dev.join(node);
        std::fs::File::create(&at).map_err(|_| ())?;
        sys_cell::bind(&source, &at, BindMode::Device).map_err(|_| ())?;
    }
    Ok(())
}

/// Kernel switches under `/proc` that belong to root by owner alone: the installation
/// cell's root keeps no capability that matters, but on Linux a write there is checked
/// by owner only, so they are bound read-only (as container runtimes do).
const PROC_READ_ONLY: [&str; 5] = ["sys", "sysrq-trigger", "irq", "bus", "fs"];

fn protect_proc(proc: &Path) -> Result<(), ()> {
    for name in PROC_READ_ONLY {
        let at = proc.join(name);
        if std::fs::symlink_metadata(&at).is_err() {
            continue;
        }
        if sys_cell::bind(&at, &at, BindMode::ReadOnly).is_err() {
            // the Ultar kernel: root without rights passes no permission check there;
            // Linux: a cell that cannot protect them does not start
            if ultar_sys::ultar_rights().is_err() {
                return Err(());
            }
        }
    }
    Ok(())
}

/// Builds the view (stage 2, pid 1, own mount namespace) and switches into it.
fn build_view(job: &CellJob) -> Result<(), SetupStep> {
    use SetupStep as S;
    let root = &job.runtime;
    sys_cell::make_mounts_private().map_err(|_| S::Private)?;
    match job.mode {
        Mode::Install => bind_writable(root).map_err(|()| S::Area)?,
        Mode::Inspect => sys_cell::bind(root, root, BindMode::ReadOnly).map_err(|_| S::Area)?,
    }
    let run = mount_point(root, "/run").map_err(|()| S::Tmpfs)?;
    sys_cell::mount_tmpfs(&run, "size=8388608,mode=0755").map_err(|_| S::Tmpfs)?;
    let tmp = mount_point(root, "/tmp").map_err(|()| S::Tmpfs)?;
    sys_cell::mount_tmpfs(&tmp, "size=268435456,mode=1777").map_err(|_| S::Tmpfs)?;
    let proc = mount_point(root, "/proc").map_err(|()| S::Proc)?;
    sys_cell::mount_proc(&proc).map_err(|_| S::Proc)?;
    protect_proc(&proc).map_err(|()| S::ProcReadOnly)?;
    devices(root).map_err(|()| S::Devices)?;
    if let Some(inbox) = &job.inbox {
        let at = mount_point(root, INBOX_IN_CELL).map_err(|()| S::Inbox)?;
        sys_cell::bind(inbox, &at, BindMode::ReadOnly).map_err(|_| S::Inbox)?;
    }
    let old = mount_point(root, "/run/.old").map_err(|()| S::Root)?;
    sys_cell::pivot_root(root, &old).map_err(|_| S::Root)?;
    std::env::set_current_dir("/").map_err(|_| S::Root)?;
    sys_cell::detach(Path::new("/run/.old")).map_err(|_| S::Root)?;
    let _ = std::fs::remove_dir("/run/.old");
    Ok(())
}

/// Capabilities the installation cell keeps on Linux: owners and modes of the packages'
/// files. No set-id: apt runs with `APT::Sandbox::User "root"` (the cell is its sandbox).
pub fn install_caps() -> rustix::thread::CapabilitySet {
    use rustix::thread::CapabilitySet as C;
    C::CHOWN | C::DAC_OVERRIDE | C::FOWNER | C::FSETID
}

/// The Ultar kernel's right `cell_chown` (bit 14, `docs/integration/zell-chown.md`): owners
/// of entries on the cell's own writable mounts only. The only right the installation
/// cell keeps there.
pub const CELL_CHOWN: u64 = 1 << 14;

/// Linux: cuts the bounding set (and with it what any program started here can ever
/// get) to `keep`. The Ultar kernel has no capabilities (it answers `EINVAL`): its
/// rights are dropped separately.
fn cut_capabilities(keep: rustix::thread::CapabilitySet) -> Result<(), ()> {
    for bit in 0..64u32 {
        let cap = rustix::thread::CapabilitySet::from_bits_retain(1u64 << bit);
        if keep.contains(cap) {
            continue;
        }
        match rustix::thread::remove_capability_from_bounding_set(cap) {
            Ok(()) => {}
            Err(e) if e == rustix::io::Errno::INVAL => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

/// Identity and rights of the cell's programs.
fn confine(mode: Mode) -> Result<(), SetupStep> {
    use SetupStep as S;
    // The Ultar kernel has rights, not capabilities: there `PR_CAPBSET_DROP` takes the
    // matching rights at once (CAP_SETUID would take `set_id` before the switch below), so
    // the bounding set is cut on Linux only and the Ultar rights are dropped after the switch.
    let ultar = ultar_sys::ultar_rights().is_ok();
    if !ultar {
        let keep = match mode {
            Mode::Install => install_caps(),
            Mode::Inspect => rustix::thread::CapabilitySet::empty(),
        };
        cut_capabilities(keep).map_err(|()| S::Capabilities)?;
    }
    // inspection: nobody; installation: root of the cell (the area's files belong to root)
    let id = if mode == Mode::Inspect { NOBODY } else { 0 };
    if ultar_sys::euid() != id {
        let identity = ultar_sys::Identity {
            uid: id,
            gid: id,
            groups: Vec::new(),
        };
        ultar_sys::switch_identity(&identity).map_err(|_| S::Identity)?;
    }
    if ultar {
        // every right but `cell_chown` in the installation cell; none in the inspection cell
        let keep = if mode == Mode::Install { CELL_CHOWN } else { 0 };
        ultar_sys::ultar_drop_rights(!keep).map_err(|_| S::Rights)?;
        if ultar_sys::ultar_rights().map_err(|_| S::Rights)? & !keep != 0 {
            return Err(S::Rights);
        }
    }
    match ultar_sys::set_no_new_privileges() {
        Ok(()) | Err(ultar_sys::Errno::ENOSYS | ultar_sys::Errno::EINVAL) => Ok(()),
        Err(_) => Err(S::NoNewPrivileges),
    }
}

/// The environment of the cell's programs (nothing of the service's own).
fn environment() -> [(&'static str, &'static str); 6] {
    [
        ("PATH", "/usr/sbin:/usr/bin:/sbin:/bin"),
        ("HOME", "/tmp"),
        ("LANG", "C.UTF-8"),
        ("DEBIAN_FRONTEND", "noninteractive"),
        ("APT_LISTCHANGES_FRONTEND", "none"),
        ("TMPDIR", "/tmp"),
    ]
}

/// Stage 2 (`--cell-init`): pid 1 of the cell. Builds the view, confines itself, runs the
/// program as its child and ends with its code; the kernel then ends the rest of the cell.
pub fn stage_two(words: &[String]) -> i32 {
    let _ = sys_cell::kill_with_parent();
    let Some(job) = CellJob::parse(words) else {
        return SETUP_FAILED;
    };
    if let Err(step) = build_view(&job).and_then(|()| confine(job.mode)) {
        return step.code();
    }
    let mut command = Command::new(&job.program[0]);
    // the installation cell's messages (apt and dpkg write their errors there) go with its
    // output; the inspection cell's output stays the fields alone
    if job.mode == Mode::Install {
        use std::os::fd::AsFd;
        if let Ok(out) = std::io::stdout().as_fd().try_clone_to_owned() {
            command.stderr(Stdio::from(out));
        }
    }
    command
        .args(&job.program[1..])
        .env_clear()
        .stdin(Stdio::null());
    for (k, v) in environment() {
        command.env(k, v);
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(SETUP_FAILED + 1),
        Err(_) => SETUP_FAILED + 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_survives_its_command_line_and_nothing_else_is_taken() {
        let job = CellJob {
            mode: Mode::Install,
            runtime: "/data/ultar/foreign/linux".into(),
            inbox: Some("/data/ultar/foreign/inbox/7".into()),
            program: vec!["/usr/bin/apt-get".into(), "install".into(), "-y".into()],
        };
        assert_eq!(CellJob::parse(&job.to_args()), Some(job.clone()));
        let mut bad = job.to_args();
        bad[1] = "root".into();
        assert_eq!(CellJob::parse(&bad), None);
        let relative = CellJob {
            program: vec!["apt-get".into()],
            ..job
        };
        assert_eq!(CellJob::parse(&relative.to_args()), None);
    }

    #[test]
    fn a_helper_that_cannot_start_is_an_error() {
        let job = CellJob {
            mode: Mode::Inspect,
            runtime: "/nonexistent".into(),
            inbox: None,
            program: vec!["/bin/true".into()],
        };
        assert!(run(
            Path::new("/nonexistent/helper"),
            &job,
            Duration::from_secs(1)
        )
        .is_err());
    }
}
