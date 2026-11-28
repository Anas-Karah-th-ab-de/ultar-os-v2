//! The two stages of the cell build (see the module above). Stage 1 runs in the process the
//! browser started, single-threaded; stage 2 is the same program started again as pid 1 of the
//! new pid namespace.

use super::{view, Entered, Failure, Plan};
use crate::args::Args;
use std::net::TcpListener;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use ultar_sys::cell::{self as sys, Namespaces};
use ultar_sys::{cellnet, Errno, Resource};

fn step(name: &'static str) -> impl Fn(Errno) -> Failure {
    move |e| Failure::Step(name, e.to_string())
}

fn io(name: &'static str) -> impl Fn(std::io::Error) -> Failure {
    move |e| Failure::Step(name, e.to_string())
}

/// Removes an empty folder that the cell's ending still holds for a moment: the kernel takes the
/// rest of the cell down (processes, mounts) after its first process ended, so the folder is
/// busy for a short time. Gives up after about three seconds (the folder then stays, empty).
fn remove_when_free(dir: &Path) {
    for _ in 0..150 {
        match std::fs::remove_dir(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

/// The memory group of the cell: a folder of the group tree with `memory.max`, joined by the
/// process before it starts the cell, so everything in the cell is counted. Left again (and the
/// folder removed) when this value goes.
struct Group {
    dir: PathBuf,
    home: PathBuf,
    pid: u32,
}

impl Group {
    fn join(megabytes: u32, pid: u32) -> Result<Group, Failure> {
        let root = Path::new(ultar_sys::machine::GROUP_ROOT);
        // the group this process is in now, to return to it at the end ("0::/path")
        let current = cellnet::own_group();
        let home = root
            .join(current.trim_start_matches('/'))
            .join("cgroup.procs");
        let dir = root.join(format!("ultar-web-{pid}"));
        std::fs::create_dir(&dir).map_err(io("memory group"))?;
        let group = Group { dir, home, pid };
        let bytes = u64::from(megabytes) * 1024 * 1024;
        std::fs::write(group.dir.join("memory.max"), bytes.to_string())
            .map_err(io("memory.max"))?;
        // no swap and one verdict for the whole cell: best effort, the limit above is the rule
        let _ = std::fs::write(group.dir.join("memory.swap.max"), "0");
        let _ = std::fs::write(group.dir.join("memory.oom.group"), "1");
        std::fs::write(group.dir.join("cgroup.procs"), pid.to_string())
            .map_err(io("memory group join"))?;
        Ok(group)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.home, self.pid.to_string());
        remove_when_free(&self.dir);
    }
}

/// The folder the cell's root is built on (outside: empty, mode 0700), removed at the end.
struct Root(PathBuf);

impl Root {
    fn make(pid: u32) -> Result<Root, Failure> {
        let path = std::env::temp_dir().join(format!("ultar-web-cell-{pid}"));
        let _ = std::fs::remove_dir(&path); // an empty left-over of a killed run
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(io("root folder"))?;
        Ok(Root(path))
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        remove_when_free(&self.0);
    }
}

/// The namespaces stage 1 makes for the cell: its own processes, IPC objects and **network**.
/// (The mounts come in stage 2: stage 1 keeps the host's file tree, to clean up at the end.)
const CELL: Namespaces = Namespaces {
    mount: false,
    pid: true,
    net: true,
    ipc: true,
};

/// The mount namespace of stage 2.
const MOUNTS: Namespaces = Namespaces {
    mount: true,
    pid: false,
    net: false,
    ipc: false,
};

/// Why `unshare` refused, in words (the Ultar kernel is recognised by its answer to the rights
/// call, never assumed).
fn unshare_words(errno: Errno) -> String {
    let kernel = if ultar_sys::ultar_rights().is_ok() {
        "; this is the Ultar kernel: it has no network namespace (unshare(CLONE_NEWNET) \
         answers EINVAL), so a page cell without network cannot be built here and the host \
         refuses to run with network instead"
    } else {
        ""
    };
    format!("{errno}{kernel}")
}

/// Stage 1: group, namespaces, stage 2, wait. Returns the exit code of the cell.
pub fn outside(plan: &Plan) -> Result<u8, Failure> {
    let pid = std::process::id();
    let me = std::env::current_exe().map_err(io("own path"))?;
    let _group = Group::join(plan.memory_mb, pid)?;
    let root = Root::make(pid)?;
    // not root: the user namespace gives the right to make the others (Linux)
    if ultar_sys::euid() != 0 {
        cellnet::enter_user_namespace().map_err(|e| {
            Failure::Step(
                "user namespace",
                format!("{e} (not root and no user namespaces)"),
            )
        })?;
    }
    sys::unshare(CELL).map_err(|e| Failure::Step("namespaces", unshare_words(e)))?;
    let status = Command::new(me)
        .args(std::env::args_os().skip(1))
        .arg("--cell-init")
        .arg(&root.0)
        .status()
        .map_err(io("start of stage 2"))?;
    Ok(match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
        (None, None) => 1,
    })
}

/// Everything that makes the process harmless, once the view stands: limits, no new
/// privileges, no capabilities, no Ultar rights. Single-threaded: capabilities are per thread.
fn confine() -> Result<(), Failure> {
    ultar_sys::set_umask(0o077);
    ultar_sys::set_limit(Resource::Core, 0, 0).map_err(step("core limit"))?;
    // a kernel without the flag honours no set-id bits either: detected by its answer
    match ultar_sys::set_no_new_privileges() {
        Ok(()) | Err(Errno::ENOSYS | Errno::EINVAL) => {}
        Err(e) => return Err(Failure::Step("no new privileges", e.to_string())),
    }
    match ultar_sys::ultar_rights() {
        Ok(mask) => {
            // the Ultar kernel: every right goes, and none may be left
            ultar_sys::ultar_drop_rights(mask).map_err(step("drop rights"))?;
            let left = ultar_sys::ultar_rights().map_err(step("read rights"))?;
            if left & mask != 0 {
                return Err(Failure::Step("drop rights", "rights are left".into()));
            }
        }
        Err(_) => cellnet::drop_capabilities().map_err(step("drop capabilities"))?,
    }
    Ok(())
}

/// Stage 2: view, loopback, confinement, relay. Returns what the host does next.
pub fn inside(args: &Args, plan: &Plan, root: &Path) -> Result<Entered, Failure> {
    if std::process::id() != 1 {
        return Err(Failure::Step(
            "start",
            "--cell-init is only for the first process of a new pid namespace".into(),
        ));
    }
    // ends with stage 1 (Linux), so the cell never outlives the process the browser started
    let _ = sys::kill_with_parent();
    // its own mount table (stage 1 must keep the host's: the root switch is only for the cell)
    sys::unshare(MOUNTS).map_err(|e| Failure::Step("mount namespace", e.to_string()))?;
    sys::make_mounts_private().map_err(step("private mounts"))?;
    view::build(plan, root)?;
    // the only door of the cell: a port of its private loopback (bound before the capabilities
    // go). A new Linux network namespace has `lo` down: bring it up (binding works even then,
    // connecting does not). A kernel whose private loopback is up from the start and that has
    // no such call is accepted if the loopback is proven to work: the cell connects to its own
    // port.
    let up = cellnet::loopback_up();
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io("loopback port"))?;
    let port = listener.local_addr().map_err(io("loopback port"))?.port();
    let proof = std::net::TcpStream::connect(("127.0.0.1", port))
        .and_then(|_| listener.accept().map(|_| ()));
    if let Err(e) = proof {
        return Err(Failure::Step(
            "loopback",
            format!("no working private loopback: {e} (bring-up: {up:?})"),
        ));
    }
    confine()?;
    std::env::set_current_dir("/").map_err(io("chdir"))?;
    // font caches go to the private /tmp (the bundle is read-only; nothing is kept)
    std::env::set_var("XDG_CACHE_HOME", view::TMP);
    crate::relay::start(listener, plan.socket.clone()).map_err(io("relay"))?;
    if args.selftest {
        return Ok(Entered::Finished(super::selftest::run(plan, port)));
    }
    Ok(Entered::Inside(format!("http://127.0.0.1:{port}")))
}
