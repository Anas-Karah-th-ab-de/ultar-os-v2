//! Device nodes below `/dev`: owner, group and mode like Linux' devtmpfs, the Linux device numbers,
//! and the gate of opening one (see the module above).

use super::{allows, of, subject, Attr};
use crate::fs;
use crate::isolation::rights::Rights;
use crate::syscall::with;

const ENOENT: i64 = 2;
const EACCES: i64 = 13;

/// Whether `path` is a device node below `/dev` (not a directory).
pub fn dev_node(path: &str) -> bool {
    path.starts_with("/dev/") && matches!(fs::open(path), Some(fs::Node::Dev(_)))
}

/// A raw device: the framebuffer, the input event nodes and the raw A/B partitions.
pub(super) fn raw_device(path: &str) -> bool {
    path == "/dev/fb0" || path.starts_with("/dev/input/event") || path.starts_with("/dev/ultar/")
}

/// `st_rdev` of a device node (Linux numbers), 0 for anything else.
pub fn rdev(path: &str) -> u64 {
    let Some(d) = path.strip_prefix("/dev/") else { return 0 };
    if let Some(n) = d.strip_prefix("input/").and_then(crate::input::parse) {
        return crate::input::rdev(n);
    }
    if let Some(r) = crate::svcdev::rdev(d) {
        return r;
    }
    match d {
        "null" => 1 << 8 | 3,
        "zero" => 1 << 8 | 5,
        "random" => 1 << 8 | 8,
        "urandom" => 1 << 8 | 9,
        "tty" => 5 << 8,
        "console" => 5 << 8 | 1,
        "fb0" => 29 << 8,
        _ => 0,
    }
}

/// May the caller open device node `path` for `want`?
pub(super) fn dev_gate(path: &str, want: u32) -> Result<(), i64> {
    // the service nodes: their own right and nothing else (svcdev)
    if let Some(right) = crate::svcdev::right_for(path) {
        let rights = with(|p| p.g.iso.rights).unwrap_or(Rights::ALL);
        return if rights.has(right) { Ok(()) } else { Err(-EACCES) };
    }
    let (cred, admin, _) = subject();
    let raw = with(|p| p.g.iso.rights.has(Rights::RAW_DEVICE)).unwrap_or(true);
    if admin || raw && raw_device(path) {
        return Ok(());
    }
    let a = of(path).ok_or(-ENOENT)?;
    // the owner root grants nothing by itself: group and other bits decide
    let a = if a.uid == 0 { Attr { uid: u32::MAX, ..a } } else { a };
    if allows(&a, &cred, want) {
        Ok(())
    } else {
        Err(-EACCES)
    }
}
