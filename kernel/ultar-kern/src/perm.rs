//! File modes and owners, and the permission checks of the file system calls (Linux semantics).
//!
//! Every entry of the file tree has a mode (type + permission bits) and an owner and group:
//!
//! * entries of the RAM layer and of a tmpfs – files, directories, symbolic links and the nodes of
//!   bound Unix sockets – keep them in a table here, set when a process creates the entry
//!   (`mode & ~umask`, effective ids of the creator) and changed by `chmod`/`chown`;
//! * ext4 stores them in the inode: read from there, and written there on a writable ext4;
//! * what has neither (FAT, entries the kernel itself wrote, read-only disks after `chmod`) reads as
//!   root-owned `0755` (directories and files) – the values every entry reported before – with a
//!   `chmod`/`chown` kept in the table until restart.
//!
//! A process is checked by owner / group / other bits with its effective ids and supplementary
//! groups (`cred.rs`). The right `FILE_ADMIN` passes every check, like root with full capabilities
//! on Linux – except executing a file without any execute bit. Being uid 0 *without* that right
//! gives nothing beyond owning root's files. The kernel's own trees (`/proc`, `/dev`, `/sys`) are
//! not subject to modes; their gates are rights – except the device nodes below `/dev`: they have
//! owner, group and mode like on Linux (devtmpfs; `fb0` and input nodes start as root `0600`, the
//! others `0666`), changed with `FILE_ADMIN` only. Root owning a node gives no access by itself
//! (raw devices need the right `RAW_DEVICE`, `FILE_ADMIN`, or group / other bits that grant it).

use crate::cred::Cred;
use crate::fs;
use crate::isolation::path::{parent, Loc, View};
use crate::isolation::rights::Rights;
use crate::isolation::tmpfs;
use crate::syscall::{locate, ustr, with};
use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;

/// File type bits of `st_mode`.
pub const S_IFMT: u32 = 0o170000;
/// Unix socket node.
pub const S_IFSOCK: u32 = 0o140000;
/// Symbolic link.
pub const S_IFLNK: u32 = 0o120000;
/// Regular file.
pub const S_IFREG: u32 = 0o100000;
/// Directory.
pub const S_IFDIR: u32 = 0o040000;
const S_IFCHR: u32 = 0o020000;
const STICKY: u32 = 0o1000;
/// Read permission.
pub const R: u32 = 4;
/// Write permission.
pub const W: u32 = 2;
/// Execute / search permission.
pub const X: u32 = 1;
const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const EACCES: i64 = 13;
const EFAULT: i64 = 14;
const EINVAL: i64 = 22;
const EROFS: i64 = 30;
const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const AT_EACCESS: u64 = 0x200;
/// "Leave unchanged" in `chown`.
const KEEP: u32 = u32::MAX;

/// Mode (type and permission bits), owner and group of one entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Attr {
    /// `st_mode`.
    pub mode: u32,
    /// Owner.
    pub uid: u32,
    /// Group.
    pub gid: u32,
}

mod dev;
use dev::{dev_gate, raw_device};
pub use dev::{dev_node, rdev};

/// Host path → attributes, for entries that do not store them on disk.
static ATTRS: Mutex<BTreeMap<String, Attr>> = Mutex::new(BTreeMap::new());

/// Who asks: ids, whether every check passes (`FILE_ADMIN`; the kernel console counts as that), umask.
fn subject() -> (Cred, bool, u32) {
    with(|p| (p.g.iso.cred, p.g.iso.rights.has(Rights::FILE_ADMIN), p.g.umask)).unwrap_or((Cred::ROOT, true, 0o22))
}

/// The kernel's own trees: no modes, their gates are rights.
fn exempt(path: &str) -> bool {
    ["/proc", "/dev", "/sys"].iter().any(|d| crate::isolation::mount::below(path, d).is_some())
}

/// Attributes of the entry at host path `path`; `None` if nothing is there.
pub fn of(path: &str) -> Option<Attr> {
    if let Some(a) = ATTRS.lock().get(path).copied() {
        return Some(a);
    }
    if dev_node(path) {
        return Some(Attr { mode: S_IFCHR | if raw_device(path) { 0o600 } else { 0o666 }, uid: 0, gid: 0 });
    }
    // a link itself (only reached without following it): the disk lookup below would follow it
    if tmpfs::readlink(path).is_some() || fs::disk_link(path).is_some() {
        return Some(Attr { mode: S_IFLNK | 0o777, uid: 0, gid: 0 });
    }
    if let Some((mode, uid, gid)) = fs::disk_attr(path) {
        return Some(Attr { mode, uid, gid });
    }
    let mode = match fs::stat(path)? {
        (0, _) => S_IFREG | 0o755,
        (1, _) => S_IFDIR | 0o755,
        _ => S_IFCHR | 0o666,
    };
    Some(Attr { mode, uid: 0, gid: 0 })
}

/// Store the attributes of `path`: in the inode on a writable ext4, else in the table.
pub fn store(path: &str, attr: Attr) {
    if fs::set_disk_attr(path, attr.mode & 0o7777, attr.uid, attr.gid) {
        return;
    }
    let key = String::from(path);
    // the table is shared by everybody: it belongs to the kernel
    crate::heap::kernel(|| ATTRS.lock().insert(key, attr));
}

/// The directories every program expects: `/tmp` (anyone may add entries, the sticky bit protects
/// them from others) and `/run` (root only). Kept in memory unless the disk already has them.
pub fn standard_dirs() {
    for (dir, mode) in [("/tmp", 0o1777), ("/run", 0o755)] {
        if fs::stat(dir).is_none() && tmpfs::store(&alloc::format!("{dir}/"), alloc::sync::Arc::new(alloc::vec::Vec::new())) {
            store(dir, Attr { mode: S_IFDIR | mode, uid: 0, gid: 0 });
        }
    }
}

/// A process created the entry at `path`: type `kind`, permission bits `mode & ~umask` (links:
/// `0777`), owned by the creator's effective ids.
pub fn created(path: &str, kind: u32, mode: u32) {
    if exempt(path) {
        return;
    }
    let (cred, _, umask) = subject();
    let perm = if kind == S_IFLNK { 0o777 } else { mode & 0o7777 & !umask };
    store(path, Attr { mode: kind | perm, uid: cred.euid(), gid: cred.egid() });
}

/// The entry at `path` is gone.
pub fn forget(path: &str) {
    ATTRS.lock().remove(path);
}

/// A whole tree (a tmpfs) is gone.
pub fn forget_tree(root: &str) {
    ATTRS.lock().retain(|k, _| crate::isolation::mount::below(k, root).is_none());
}

/// The entry moved (`rename`).
pub fn moved(from: &str, to: &str) {
    let old = ATTRS.lock().remove(from);
    match old {
        Some(a) => store(to, a),
        None => forget(to),
    }
}

/// Whether ids `cred` have all of `want` (`R`/`W`/`X`) on an entry with `attr`.
pub fn allows(attr: &Attr, cred: &Cred, want: u32) -> bool {
    let bits = if cred.euid() == attr.uid {
        attr.mode >> 6
    } else if cred.in_group(attr.gid) {
        attr.mode >> 3
    } else {
        attr.mode
    };
    bits & 7 & want == want
}

fn check_as(path: &str, cred: &Cred, admin: bool, want: u32) -> Result<(), i64> {
    if admin || exempt(path) {
        return Ok(());
    }
    match of(path) {
        Some(a) if !allows(&a, cred, want) => Err(-EACCES),
        _ => Ok(()),
    }
}

/// The calling process needs `want` on the entry at host path `path` (`EACCES`). An entry that does
/// not exist passes: the caller reports that itself.
pub fn check(path: &str, want: u32) -> Result<(), i64> {
    let (cred, admin, _) = subject();
    check_as(path, &cred, admin, want)
}

/// Search permission on every directory that leads to `loc` in the view it was resolved in.
pub fn search(view: &View, loc: &Loc, cred: &Cred) -> Result<(), i64> {
    let mut end = 0;
    loop {
        // "/" first, then every longer prefix that ends before a "/"
        let dir = if end == 0 { "/" } else { &loc.view[..end] };
        check_as(&view.host_of(dir), cred, false, X)?;
        match loc.view[end + 1..].find('/') {
            Some(i) => end += 1 + i,
            None => return Ok(()),
        }
    }
}

/// May the caller add an entry at `path`? (write and search on the directory)
pub fn may_create(path: &str) -> Result<(), i64> {
    check(&parent(path), W | X)
}

/// May the caller remove or replace the entry at `path`? Write and search on the directory; in a
/// sticky directory only the owner of the entry or of the directory.
pub fn may_remove(path: &str) -> Result<(), i64> {
    let (cred, admin, _) = subject();
    let dir = parent(path);
    check_as(&dir, &cred, admin, W | X)?;
    if admin || exempt(path) {
        return Ok(());
    }
    match (of(&dir), of(path)) {
        (Some(d), Some(e)) if d.mode & STICKY != 0 && cred.euid() != e.uid && cred.euid() != d.uid => Err(-EPERM),
        _ => Ok(()),
    }
}

/// May the caller execute the file at `path`? Even `FILE_ADMIN` needs one execute bit to be set.
pub fn may_exec(path: &str) -> Result<(), i64> {
    let (cred, admin, _) = subject();
    match of(path) {
        Some(a) if admin && a.mode & 0o111 == 0 => Err(-EACCES),
        Some(a) if !admin && !allows(&a, &cred, X) => Err(-EACCES),
        _ => Ok(()),
    }
}

/// May the caller open the existing entry at `path` with access mode `acc` (0 read, 1 write, 2 both)?
pub fn may_open(path: &str, acc: u64, truncate: bool) -> Result<(), i64> {
    let want = match acc {
        0 => R,
        1 => W,
        _ => R | W,
    } | if truncate { W } else { 0 };
    check(path, want)
}

/// Gate of `open` at host path `path`: a socket node cannot be opened (`ENXIO`); an existing entry
/// needs the permissions of the access mode, a new one write and search on its directory.
pub fn open_gate(path: &str, acc: u64, truncate: bool, create: bool) -> Result<(), i64> {
    let known = ATTRS.lock().get(path).copied();
    if matches!(known, Some(a) if a.mode & S_IFMT == S_IFSOCK) {
        return Err(-6);
    }
    if dev_node(path) {
        return dev_gate(path, if acc == 0 { R } else if acc == 1 { W } else { R | W });
    }
    let (_, admin, _) = subject();
    if admin || exempt(path) {
        return Ok(());
    }
    match of(path) {
        Some(_) => may_open(path, acc, truncate),
        None if create => may_create(path),
        None => Ok(()),
    }
}

/// Resolve a path argument for `chmod`/`chown`: the entry must exist, outside the kernel's trees,
/// on a writable mount.
fn target(dirfd: i64, path: u64, follow: bool) -> Result<String, i64> {
    let p = ustr(path).ok_or(-EFAULT)?;
    let loc = locate(dirfd, &p, follow)?;
    if of(&loc.host).is_none() {
        return Err(-ENOENT);
    }
    // device nodes have modes; changing them is file administration
    if exempt(&loc.host) && !(dev_node(&loc.host) && subject().1) {
        return Err(-EPERM);
    }
    if loc.read_only {
        return Err(-EROFS);
    }
    Ok(loc.host)
}

/// Host path behind a descriptor of a file or directory.
fn fd_path(fd: u64) -> Result<String, i64> {
    use crate::proc::FdKind;
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::File { path, .. }) | Some(FdKind::Dir { path, .. }) => Ok(path.clone()),
        Some(_) => Err(-EINVAL),
        None => Err(-9),
    })
    .unwrap_or(Err(-9))
}

fn chmod_host(path: &str, mode: u64) -> i64 {
    let (cred, admin, _) = subject();
    let Some(a) = of(path) else { return -ENOENT };
    if !admin && cred.euid() != a.uid {
        return -EPERM;
    }
    store(path, Attr { mode: a.mode & S_IFMT | mode as u32 & 0o7777, ..a });
    0
}

/// Owner changes need `FILE_ADMIN`; the owner itself may hand the entry to a group it belongs to.
/// Without the right the set-id bits are cleared, as on Linux.
fn chown_host(path: &str, uid: u32, gid: u32) -> i64 {
    let (cred, admin, _) = subject();
    let Some(a) = of(path) else { return -ENOENT };
    let (new_uid, new_gid) = (if uid == KEEP { a.uid } else { uid }, if gid == KEEP { a.gid } else { gid });
    let own_group_only = cred.euid() == a.uid && new_uid == a.uid && (new_gid == a.gid || cred.in_group(new_gid));
    if !admin && !own_group_only {
        return -EPERM;
    }
    let mode = if admin { a.mode } else { a.mode & !0o6000 };
    store(path, Attr { mode, uid: new_uid, gid: new_gid });
    0
}

/// `chmod`, `fchmodat`.
pub fn sys_chmod(dirfd: i64, path: u64, mode: u64, flags: u64) -> i64 {
    match target(dirfd, path, flags & AT_SYMLINK_NOFOLLOW == 0) {
        Ok(host) => chmod_host(&host, mode),
        Err(e) => e,
    }
}

/// `fchmod`.
pub fn sys_fchmod(fd: u64, mode: u64) -> i64 {
    match fd_path(fd) {
        Ok(host) if exempt(&host) => -EPERM,
        Ok(host) => chmod_host(&host, mode),
        Err(e) => e,
    }
}

/// `chown`, `lchown`, `fchownat`.
pub fn sys_chown(dirfd: i64, path: u64, uid: u64, gid: u64, flags: u64) -> i64 {
    match target(dirfd, path, flags & AT_SYMLINK_NOFOLLOW == 0) {
        Ok(host) => chown_host(&host, uid as u32, gid as u32),
        Err(e) => e,
    }
}

/// `fchown`.
pub fn sys_fchown(fd: u64, uid: u64, gid: u64) -> i64 {
    match fd_path(fd) {
        Ok(host) if exempt(&host) => -EPERM,
        Ok(host) => chown_host(&host, uid as u32, gid as u32),
        Err(e) => e,
    }
}

/// `access`, `faccessat`, `faccessat2`: checked with the real ids unless `AT_EACCESS`.
pub fn sys_access(dirfd: i64, path: u64, mode: u64, flags: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    let loc = match locate(dirfd, &p, flags & AT_SYMLINK_NOFOLLOW == 0) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let Some(a) = of(&loc.host) else { return -ENOENT };
    let (mut cred, admin, _) = subject();
    if flags & AT_EACCESS == 0 {
        (cred.uid[1], cred.gid[1]) = (cred.uid[0], cred.gid[0]);
    }
    let want = mode as u32 & 7;
    let refused = if dev_node(&loc.host) {
        dev_gate(&loc.host, want).is_err()
    } else if admin {
        want & X != 0 && a.mode & S_IFMT == S_IFREG && a.mode & 0o111 == 0
    } else {
        !exempt(&loc.host) && !allows(&a, &cred, want)
    };
    if refused {
        return -EACCES;
    }
    if want & W != 0 && loc.read_only {
        return -EROFS;
    }
    0
}
