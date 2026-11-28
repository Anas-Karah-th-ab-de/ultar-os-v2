//! What the engine sees inside its cell: a small in-memory root that holds only mount points, the
//! bundle and system libraries read-only, the engine's profile and downloads read-write (never
//! executable), a private `/tmp`, its own `/proc` and a few harmless device nodes. After the root
//! switch the old root is detached: no entry is left through which a host path could be named.
//! Same build as the launcher's (`system/core/ultar-launcher/src/cell/view.rs`).

use super::{Failure, Plan};
use std::path::{Path, PathBuf};
use ultar_sys::cell::{self as sys, BindMode};

/// The private temporary folder (in memory, never executable).
pub const TMP: &str = "/tmp";
/// Size of the private `/tmp`.
const TMP_OPTIONS: &str = "size=268435456,mode=0700";
/// Size of the cell's root tree (only folders for mount points).
const ROOT_OPTIONS: &str = "size=1048576,mode=0755";
/// Shared memory some libraries expect below `/dev`.
const SHM_OPTIONS: &str = "size=67108864,mode=1777";
/// Name of the old root during the switch.
const OLD: &str = ".old";

fn step(name: &'static str) -> impl Fn(ultar_sys::Errno) -> Failure {
    move |e| Failure::Step(name, e.to_string())
}

fn io(name: &'static str) -> impl Fn(std::io::Error) -> Failure {
    move |e| Failure::Step(name, e.to_string())
}

/// `root` + host path `path` (absolute).
pub fn inside(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

fn mount_point(root: &Path, path: &Path) -> Result<PathBuf, Failure> {
    let target = inside(root, path);
    std::fs::create_dir_all(&target).map_err(io("mount point"))?;
    Ok(target)
}

/// The harmless device nodes of a cell: the names below `/dev` it gets.
pub fn device_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = sys::DEV_NODES.to_vec();
    names.push("shm");
    names
}

fn devices(root: &Path) -> Result<(), Failure> {
    let dev = Path::new(sys::DEV);
    let target = mount_point(root, dev)?;
    sys::mount_tmpfs(&target, "size=65536,mode=0755").map_err(step("dev"))?;
    for node in sys::DEV_NODES {
        let source = dev.join(node);
        if !source.exists() {
            continue;
        }
        let at = target.join(node);
        std::fs::File::create(&at).map_err(io("dev node"))?;
        sys::bind(&source, &at, BindMode::Device).map_err(step("dev node"))?;
    }
    let shm = target.join("shm");
    std::fs::create_dir(&shm).map_err(io("dev/shm"))?;
    sys::mount_tmpfs(&shm, SHM_OPTIONS).map_err(step("dev/shm"))
}

/// Builds the view at `root` (a folder of the outside, empty) and switches to it. Stage 2:
/// pid 1 of the cell, in its own mount namespace.
pub fn build(plan: &Plan, root: &Path) -> Result<(), Failure> {
    sys::mount_tmpfs(root, ROOT_OPTIONS).map_err(step("root"))?;
    // the private /tmp first: a folder below /tmp that is bound later stays visible
    let tmp = mount_point(root, Path::new(TMP))?;
    sys::mount_tmpfs(&tmp, TMP_OPTIONS).map_err(step("tmp"))?;
    for (n, dir) in plan.read_only.iter().enumerate() {
        if !dir.is_dir() || plan.read_only[..n].contains(dir) {
            continue; // not on this system (e.g. /lib64), or named twice: left out
        }
        let target = mount_point(root, dir)?;
        sys::bind(dir, &target, BindMode::ReadOnly).map_err(step("bundle"))?;
    }
    for dir in &plan.read_write {
        // the browser makes these before it starts the host: a missing one is a failure
        let target = mount_point(root, dir)?;
        sys::bind(dir, &target, BindMode::WritableNoExec).map_err(step("profile"))?;
    }
    let proc = mount_point(root, Path::new(sys::PROC))?;
    sys::mount_proc(&proc).map_err(step("proc"))?;
    devices(root)?;
    let old = root.join(OLD);
    std::fs::create_dir(&old).map_err(io("old root"))?;
    sys::pivot_root(root, &old).map_err(step("pivot_root"))?;
    std::env::set_current_dir("/").map_err(io("chdir"))?;
    let old = Path::new("/").join(OLD);
    sys::detach(&old).map_err(step("detach old root"))?;
    std::fs::remove_dir(&old).map_err(io("old root"))?;
    sys::remount_read_only(Path::new("/")).map_err(step("root read-only"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_root_and_the_device_names() {
        assert_eq!(
            inside(Path::new("/c"), Path::new("/usr/lib")),
            PathBuf::from("/c/usr/lib")
        );
        let names = device_names();
        assert!(names.contains(&"null") && names.contains(&"shm"));
        assert!(!names
            .iter()
            .any(|n| n.starts_with("sd") || n.starts_with("dri")));
    }
}
