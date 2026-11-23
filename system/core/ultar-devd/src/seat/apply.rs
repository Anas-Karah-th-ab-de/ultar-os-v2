//! Changing owner and mode of one node, the scan of all seat nodes and the
//! state file for the compositor.

use super::{is_seat_node, wanted, SeatConfig, Wanted, STATE_FILE};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{lchown, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use ultar_hal_system::seat_nodes;

/// Most nodes looked at in one scan.
pub const MAX_NODES: usize = 256;

/// What happened to one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// Owner, group and mode were already right.
    Unchanged,
    /// Changed and read back.
    Set,
    /// The kernel refused the change (`EPERM`, `EROFS`, `EOPNOTSUPP`).
    KernelRefused,
    /// Not a character device, a link, or below a directory others may change:
    /// left alone.
    NotDevice,
    /// Any other error, or the change did not stay.
    Failed(String),
}

/// One node and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReport {
    /// Name below `/dev`.
    pub devname: String,
    /// What it should look like.
    pub wanted: Wanted,
    /// What happened.
    pub applied: Applied,
}

/// Whether the kernel keeps owner and mode of device nodes, as found out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceModes {
    /// Every node has the wanted owner and mode (or there was none to change).
    Kept,
    /// The Ultar kernel refused to change device nodes: it gates them by the
    /// right `raw_device`, so the group of a node says nothing.
    KernelGated,
    /// Something went wrong: the compositor keeps checking the group (and
    /// finds none of its nodes), fail closed.
    Failed,
}

impl DeviceModes {
    /// The word in the state file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::KernelGated => "kernel-gated",
            Self::Failed => "failed",
        }
    }

    /// The verdict of a scan. `ultar_kernel`: the Ultar kernel runs (only there
    /// is a refusal read as "the kernel gates by rights"; on Linux root may
    /// change any node, so a refusal is a failure).
    pub fn of(reports: &[NodeReport], ultar_kernel: bool) -> Self {
        if reports
            .iter()
            .any(|r| matches!(r.applied, Applied::Failed(_)))
        {
            return Self::Failed;
        }
        if reports.iter().any(|r| r.applied == Applied::KernelRefused) {
            return if ultar_kernel {
                Self::KernelGated
            } else {
                Self::Failed
            };
        }
        Self::Kept
    }
}

/// Reads at most `max` bytes of a small text file.
pub(crate) fn read_small(path: &Path, max: u64) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(max)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

/// True if `dir` is a real directory of root that nobody else may change.
fn trusted_dir(dir: &Path) -> bool {
    fs::symlink_metadata(dir)
        .is_ok_and(|m| m.file_type().is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0)
}

fn refused(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(1 | 30 | 95))
}

fn outcome(error: std::io::Error) -> Applied {
    if refused(&error) {
        Applied::KernelRefused
    } else {
        Applied::Failed(error.to_string())
    }
}

fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Gives node `devname` (below `root/dev`) the owner, group and mode in
/// `wanted`. Narrows the mode first, then changes owner and group, then sets the
/// final mode, so no step grants more than the start or the end does.
pub fn apply_node(root: &Path, devname: &str, wanted: Wanted) -> NodeReport {
    let report = |applied| NodeReport {
        devname: devname.to_owned(),
        wanted,
        applied,
    };
    let dev = seat_nodes::dev_dir(root);
    let path = seat_nodes::node_path(root, devname);
    let parent_ok = path.parent().is_some_and(trusted_dir) && trusted_dir(&dev);
    if !is_seat_node(devname) || !parent_ok {
        return report(Applied::NotDevice);
    }
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) if std::os::unix::fs::FileTypeExt::is_char_device(&meta.file_type()) => meta,
        Ok(_) => return report(Applied::NotDevice),
        Err(error) => return report(Applied::Failed(error.to_string())),
    };
    let mode = meta.mode() & 0o7777;
    if (meta.uid(), meta.gid(), mode) == (wanted.uid, wanted.gid, wanted.mode) {
        return report(Applied::Unchanged);
    }
    let steps = chmod(&path, mode & wanted.mode)
        .and_then(|()| lchown(&path, Some(wanted.uid), Some(wanted.gid)))
        .and_then(|()| chmod(&path, wanted.mode));
    if let Err(error) = steps {
        return report(outcome(error));
    }
    match fs::symlink_metadata(&path) {
        Ok(m) if (m.uid(), m.gid(), m.mode() & 0o7777) == (wanted.uid, wanted.gid, wanted.mode) => {
            report(Applied::Set)
        }
        Ok(_) => report(Applied::KernelRefused),
        Err(error) => report(Applied::Failed(error.to_string())),
    }
}

fn names_in(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten().take(MAX_NODES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let devname = format!("{prefix}{name}");
        if is_seat_node(&devname) {
            out.push(devname);
        }
    }
}

/// Every seat node below `root/dev`, sorted, at most [`MAX_NODES`].
pub fn seat_nodes(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for (dir, prefix) in seat_nodes::NODE_DIRS {
        names_in(&root.join(dir), prefix, &mut names);
    }
    names.sort();
    names.truncate(MAX_NODES);
    names
}

/// Applies the configuration to every seat node (at boot, after lost events).
pub fn scan(root: &Path, config: &SeatConfig) -> Vec<NodeReport> {
    seat_nodes(root)
        .into_iter()
        .filter_map(|name| wanted(root, &name, config).map(|w| apply_node(root, &name, w)))
        .collect()
}

/// Writes `modes` into `dir/modes` (replaced atomically, mode `0644`).
pub fn write_state(dir: &Path, modes: DeviceModes) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(std::io::Error::from_raw_os_error(20));
    }
    let temp = dir.join(".modes.new");
    let _ = fs::remove_file(&temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&temp)?;
    file.write_all(format!("{}\n", modes.word()).as_bytes())?;
    file.sync_all()?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o644))?;
    fs::rename(&temp, dir.join(STATE_FILE))
}
