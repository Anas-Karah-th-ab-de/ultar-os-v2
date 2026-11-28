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
/// Where the user's file areas appear (`/areas/<name>`).
pub const AREAS: &str = "/areas";
/// Where the app's publish directory appears (`ULTAR_PUBLISH`), if it has one.
pub const PUBLISH: &str = "/publish";
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

/// A top-level link of the Linux area (merged `/usr`: `bin -> usr/bin`): only a
/// relative target that stays inside the area is made again; anything else is left out.
fn safe_link_target(target: &Path) -> bool {
    target.is_relative()
        && target.components().next().is_some()
        && target
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        && target.as_os_str().len() <= 64
}

/// An app from another system: the entries of the Linux area at `runtime` become the
/// cell's `/usr`, `/etc` … – directories bound read-only (`nosuid`, `nodev`), top-level
/// links made again as links. Nothing of the host's own system is bound.
fn bind_runtime(root: &Path, runtime: &Path) -> Result<(), CellFailure> {
    for entry in crate::foreign::RUNTIME_ENTRIES {
        let host = runtime.join(entry);
        let Ok(meta) = std::fs::symlink_metadata(&host) else {
            continue; // not in this area (e.g. lib32)
        };
        let at = root.join(entry);
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&host).map_err(|_| CellFailure::Mounts)?;
            if safe_link_target(&target) {
                std::os::unix::fs::symlink(&target, &at).map_err(|_| CellFailure::Mounts)?;
            }
        } else if meta.is_dir() {
            std::fs::create_dir_all(&at).map_err(|_| CellFailure::Mounts)?;
            sys_cell::bind(&host, &at, BindMode::ReadOnly).map_err(|_| CellFailure::Mounts)?;
        }
    }
    Ok(())
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

/// One device node of the kernel at the same name below the cell's `/dev` tmpfs `target`
/// (skipped where the kernel has no such node).
fn bind_node(dev: &Path, target: &Path, node: &str) -> Result<(), CellFailure> {
    let source = dev.join(node);
    if !source.exists() {
        return Ok(());
    }
    let at = target.join(node);
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir).map_err(|_| CellFailure::Mounts)?;
    }
    std::fs::File::create(&at).map_err(|_| CellFailure::Mounts)?;
    sys_cell::bind(&source, &at, BindMode::Device).map_err(|_| CellFailure::Mounts)
}

/// The device nodes of the cell. `sound`: the cell keeps the kernel right that opens the sound
/// card's streams, so their node is bound as well (it opens with that right alone).
fn devices(root: &Path, mode: DevMode, sound: bool) -> Result<(), CellFailure> {
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
                bind_node(dev, &target, node)?;
            }
            if sound {
                bind_node(dev, &target, sys_cell::SOUND_NODE)?;
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
    match &plan.runtime {
        Some(runtime) => bind_runtime(root, runtime)?,
        None => bind_dirs(root, &plan.view)?,
    }
    if let Some(bridge) = plan.bridge.as_ref().filter(|b| b.is_dir()) {
        let target = mount_point(root, Path::new(crate::foreign::BRIDGE_IN_CELL))?;
        sys_cell::bind(bridge, &target, BindMode::ReadOnly).map_err(|_| CellFailure::Mounts)?;
    }
    let data = mount_point(root, Path::new(DATA))?;
    sys_cell::bind(&plan.data, &data, BindMode::WritableNoExec).map_err(|_| CellFailure::Mounts)?;
    if plan.namespaces.pid {
        let proc = mount_point(root, Path::new(sys_cell::PROC))?;
        sys_cell::mount_proc(&proc).map_err(|_| CellFailure::Mounts)?;
    }
    let sound = plan
        .drop_rights
        .is_some_and(|drop| drop & crate::perm::KERNEL_SOUND == 0);
    devices(root, plan.dev, sound)?;
    bind_dirs(root, &plan.shares)?;
    for (host, name, rw) in &plan.areas {
        if !host.is_dir() || !super::valid_area_name(name) {
            continue; // not made on this system: the app shows the area as missing
        }
        let target = mount_point(root, &Path::new(AREAS).join(name))?;
        let mode = if *rw {
            BindMode::WritableNoExec
        } else {
            BindMode::ReadOnly
        };
        sys_cell::bind(host, &target, mode).map_err(|_| CellFailure::Mounts)?;
    }
    if let Some(host) = plan.publish.as_ref().filter(|h| h.is_dir()) {
        let target = mount_point(root, Path::new(PUBLISH))?;
        sys_cell::bind(host, &target, BindMode::WritableNoExec).map_err(|_| CellFailure::Mounts)?;
    }
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
            net_guard: false,
            root: "/c".into(),
            data: "/d".into(),
            view: Vec::new(),
            shares: Vec::new(),
            areas: Vec::new(),
            publish: None,
            groups: Vec::new(),
            dev: DevMode::None,
            tmp_bytes: 64 << 20,
            drop_rights: None,
            nproc: None,
            runtime: None,
            bridge: None,
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
        assert_eq!(
            devices(Path::new("/nonexistent"), DevMode::None, true),
            Ok(())
        );
    }

    #[test]
    fn only_inner_links_of_the_linux_area_are_made() {
        assert!(safe_link_target(Path::new("usr/bin")));
        for bad in ["/usr/bin", "../usr", "usr/../../etc", "./usr", ""] {
            assert!(!safe_link_target(Path::new(bad)), "{bad}");
        }
    }
}
