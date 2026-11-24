//! What an app sees: a small in-memory root that holds only mount points,
//! read-only system directories, its own data directory (the only
//! writable place that outlives it), a private `/tmp`, its own `/proc` and
//! a few harmless device nodes. After the root switch the old root is
//! detached: no table entry is left through which a host path could be
//! named. Then the root itself becomes read-only.

use super::{CellFailure, CellPlan, DevMode};
use std::path::{Path, PathBuf};
use ultar_sys::cell::{self as sys_cell, BindMode};

/// Where the app's data directory appears (also its home and working
/// directory).
pub const DATA: &str = "/data";
/// The private temporary directory.
pub const TMP: &str = "/tmp";
/// Name of the old root during the switch.
const OLD: &str = ".old";
/// Size of the cell's root tree (only directories for mount points).
const ROOT_OPTIONS: &str = "size=1048576,mode=0755";

/// `root` + host path `path` (absolute).
fn inside(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

fn mount_point(root: &Path, path: &Path) -> Result<PathBuf, CellFailure> {
    let target = inside(root, path);
    std::fs::create_dir_all(&target).map_err(|_| CellFailure::Mounts)?;
    Ok(target)
}

/// Binds each existing host directory of `dirs` read-only.
fn bind_dirs(root: &Path, dirs: &[PathBuf]) -> Result<(), CellFailure> {
    for (n, dir) in dirs.iter().enumerate() {
        if !dir.is_dir() || dirs[..n].contains(dir) {
            continue; // not on this system (e.g. /lib64), or named twice: left out
        }
        let target = mount_point(root, dir)?;
        sys_cell::bind(dir, &target, BindMode::ReadOnly).map_err(|_| CellFailure::Mounts)?;
    }
    Ok(())
}

fn tmp_options(plan: &CellPlan) -> String {
    let owner = plan
        .identity
        .map(|(uid, gid)| format!(",uid={uid},gid={gid}"))
        .unwrap_or_default();
    format!("size={},mode=0700{owner}", plan.tmp_bytes.max(65536))
}

fn devices(root: &Path, mode: DevMode) -> Result<(), CellFailure> {
    let dev = Path::new(sys_cell::DEV);
    match mode {
        DevMode::None => Ok(()),
        DevMode::DevTmpfs => {
            let target = mount_point(root, dev)?;
            sys_cell::mount_devtmpfs(&target).map_err(|_| CellFailure::Mounts)
        }
        DevMode::Binds => {
            let target = mount_point(root, dev)?;
            sys_cell::mount_tmpfs(&target, "size=65536,mode=0755")
                .map_err(|_| CellFailure::Mounts)?;
            for node in sys_cell::DEV_NODES {
                let source = dev.join(node);
                if !source.exists() {
                    continue;
                }
                let at = target.join(node);
                std::fs::File::create(&at).map_err(|_| CellFailure::Mounts)?;
                sys_cell::bind(&source, &at, BindMode::Device).map_err(|_| CellFailure::Mounts)?;
            }
            Ok(())
        }
    }
}

/// Builds the view at `plan.root` and switches to it (stage 2, pid 1 of
/// the cell, in its own mount namespace).
pub fn build(plan: &CellPlan) -> Result<(), CellFailure> {
    let root = &plan.root;
    sys_cell::mount_tmpfs(root, ROOT_OPTIONS).map_err(|_| CellFailure::Mounts)?;
    // the private /tmp first: a view directory below /tmp stays visible
    let tmp = mount_point(root, Path::new(TMP))?;
    sys_cell::mount_tmpfs(&tmp, &tmp_options(plan)).map_err(|_| CellFailure::Mounts)?;
    bind_dirs(root, &plan.view)?;
    let data = mount_point(root, Path::new(DATA))?;
    sys_cell::bind(&plan.data, &data, BindMode::WritableNoExec).map_err(|_| CellFailure::Mounts)?;
    if plan.namespaces.pid {
        let proc = mount_point(root, Path::new(sys_cell::PROC))?;
        sys_cell::mount_proc(&proc).map_err(|_| CellFailure::Mounts)?;
    }
    devices(root, plan.dev)?;
    bind_dirs(root, &plan.shares)?;
    let old = root.join(OLD);
    std::fs::create_dir(&old).map_err(|_| CellFailure::Root)?;
    sys_cell::pivot_root(root, &old).map_err(|_| CellFailure::Root)?;
    std::env::set_current_dir("/").map_err(|_| CellFailure::Root)?;
    let old = Path::new("/").join(OLD);
    sys_cell::detach(&old).map_err(|_| CellFailure::Root)?;
    std::fs::remove_dir(&old).map_err(|_| CellFailure::Root)?;
    sys_cell::remount_read_only(Path::new("/")).map_err(|_| CellFailure::Root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Program;
    use ultar_sys::cell::Namespaces;

    #[test]
    fn paths_inside_the_root_and_tmp_options() {
        assert_eq!(
            inside(Path::new("/c"), Path::new("/usr/lib")),
            PathBuf::from("/c/usr/lib")
        );
        let mut plan = CellPlan {
            identity: Some((20001, 20001)),
            group_dir: None,
            namespaces: Namespaces::default(),
            root: "/c".into(),
            data: "/d".into(),
            view: Vec::new(),
            shares: Vec::new(),
            dev: DevMode::None,
            tmp_bytes: 64 << 20,
            drop_rights: None,
            nproc: None,
            program: Program {
                exec: "/bin/x".into(),
                args: Vec::new(),
            },
        };
        assert_eq!(
            tmp_options(&plan),
            "size=67108864,mode=0700,uid=20001,gid=20001"
        );
        plan.identity = None;
        plan.tmp_bytes = 1;
        assert_eq!(tmp_options(&plan), "size=65536,mode=0700");
        // nothing to do without a /dev
        assert_eq!(devices(Path::new("/nonexistent"), DevMode::None), Ok(()));
    }
}
