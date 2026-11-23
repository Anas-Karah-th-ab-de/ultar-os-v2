//! System calls of the isolation layer: `mount`, `umount2`, `pivot_root`, `chroot`, `unshare`, `setns`,
//! `symlink`, the rights part of `prctl`, and the right checks of calls that only need a gate.

use super::mount::{join, repath, MountNs, Source};
use super::path::{link_at, parent};
use super::pid::PidNs;
use super::rights::Rights;
use super::tmpfs::{self, Tmpfs};
use super::{Isolation, NsRef};
use crate::fs;
use crate::proc::{self, FdKind, Proc, Shared, State, PROCS};
use crate::syscall::{locate, ustr, with};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const ESRCH: i64 = 3;
const EBADF: i64 = 9;
const EFAULT: i64 = 14;
const EEXIST: i64 = 17;
const ENODEV: i64 = 19;
const ENOTDIR: i64 = 20;
const EINVAL: i64 = 22;
const ENOSPC: i64 = 28;
const EROFS: i64 = 30;
const ENAMETOOLONG: i64 = 36;

/// `clone`/`unshare` flag: new mount namespace.
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// `clone`/`unshare` flag: new pid namespace.
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// Namespace kinds this stage does not have: cgroup, uts, ipc, user, net.
pub const CLONE_NEW_UNSUPPORTED: u64 = 0x0200_0000 | 0x0400_0000 | 0x0800_0000 | 0x1000_0000 | 0x4000_0000;

const MS_RDONLY: u64 = 1;
const MS_REMOUNT: u64 = 0x20;
const MS_BIND: u64 = 0x1000;
const MS_MOVE: u64 = 0x2000;
const MS_UNBINDABLE: u64 = 1 << 17;
const MS_PRIVATE: u64 = 1 << 18;
const MS_SLAVE: u64 = 1 << 19;
const MS_SHARED: u64 = 1 << 20;
const MNT_DETACH: u64 = 2;
const UMOUNT_NOFOLLOW: u64 = 8;

const PR_CAPBSET_READ: u64 = 23;
const PR_CAPBSET_DROP: u64 = 24;
const PR_GET_AUXV: u64 = 0x4155_5856;
/// `prctl` option: return the rights of the caller as a bit mask.
pub const PR_ULTAR_RIGHTS_GET: u64 = 0x554c_5200;
/// `prctl` option: drop the rights named by the bit mask in the second argument.
pub const PR_ULTAR_RIGHTS_DROP: u64 = 0x554c_5201;

/// Isolation state of the calling group (snapshot).
fn snapshot() -> Option<Isolation> {
    with(|p| p.g.iso.clone())
}

/// Whether the calling group holds `right`.
pub fn holds(right: Rights) -> bool {
    with(|p| p.g.iso.rights.has(right)).unwrap_or(false)
}

/// Run `f` on the calling thread with the number of live threads of its group.
fn with_threads<R>(f: impl FnOnce(&mut Proc, usize) -> R) -> Option<R> {
    let mut ps = PROCS.lock();
    let me = proc::current();
    let tgid = ps.iter().find(|p| p.pid == me)?.tgid;
    let threads = ps.iter().filter(|p| p.tgid == tgid && !matches!(p.state, State::Dead | State::Zombie(_) | State::Exiting)).count();
    ps.iter_mut().find(|p| p.pid == me).map(|p| f(p, threads))
}

/// Calls that need a right but no further work in this stage: `Some(-EPERM)` if the caller lacks it.
/// With the right they behave as before (the operations themselves do not exist yet).
pub fn gate(nr: u64) -> Option<i64> {
    let right = match nr {
        175 | 176 | 313 => Rights::LOAD_DRIVER,   // init_module, delete_module, finit_module
        159 | 164 | 227 | 305 => Rights::SET_TIME, // adjtimex, settimeofday, clock_settime, clock_adjtime
        _ => return None,
    };
    (!holds(right)).then_some(-EPERM)
}

/// `prctl`: rights (Linux bounding-set options and the Ultar options); everything else as before.
pub fn sys_prctl(option: u64, arg: u64) -> i64 {
    match option {
        // PR_GET_AUXV (Linux 6.4+) is not provided: EINVAL makes glibc/Rust take their fallback
        PR_GET_AUXV => -EINVAL,
        PR_CAPBSET_READ => Rights::from_linux_cap(arg).map(|r| holds(r) as i64).unwrap_or(0),
        PR_CAPBSET_DROP => {
            if let Some(r) = Rights::from_linux_cap(arg) {
                with(|p| p.g.iso.rights.drop_mask(r.bits()));
            }
            0
        }
        PR_ULTAR_RIGHTS_GET => with(|p| p.g.iso.rights.bits() as i64).unwrap_or(0),
        PR_ULTAR_RIGHTS_DROP => {
            with(|p| p.g.iso.rights.drop_mask(arg));
            0
        }
        _ => 0,
    }
}

/// Namespace path of a directory named by the caller, or the error number.
fn dir_in_ns(iso: &Isolation, path: u64, follow: bool) -> Result<String, i64> {
    let p = ustr(path).ok_or(-EFAULT)?;
    let loc = locate(-100, &p, follow)?;
    match fs::stat(&loc.host) {
        Some((1, _)) => Ok(join(&iso.root, &loc.view)),
        Some(_) => Err(-ENOTDIR),
        None => Err(-ENOENT),
    }
}

fn mount_source(iso: &Isolation, source: u64, fstype: u64, flags: u64, data: u64) -> Result<(Source, bool, &'static str), i64> {
    if flags & MS_BIND != 0 {
        let from = dir_in_ns(iso, source, true)?;
        let (src, read_only) = iso.mnt.source_of(&from);
        return Ok((src, read_only, "none"));
    }
    let kind = if fstype == 0 { String::new() } else { ustr(fstype).ok_or(-EFAULT)? };
    match kind.as_str() {
        "tmpfs" | "ramfs" => {
            // `size=` in the mount options sets the quota of the tree (default: see isolation::tmpfs)
            let options = if data == 0 { String::new() } else { ustr(data).ok_or(-EFAULT)? };
            let quota = crate::isolation::tmpfs::parse_size(&options).map_err(|_| -EINVAL)?;
            let (mode, uid, gid) = crate::isolation::tmpfs::parse_root_attr(&options).map_err(|_| -EINVAL)?;
            let tree = Tmpfs::create(quota);
            crate::perm::store(&tree.host_root(), crate::perm::Attr { mode: crate::perm::S_IFDIR | mode, uid, gid });
            Ok((Source::Tmpfs(tree, String::new()), false, "tmpfs"))
        }
        // the group tree, for a cell whose manager wants it visible inside
        "cgroup2" => Ok((Source::Tree(String::from(crate::groups::files::ROOT_PATH)), false, "cgroup2")),
        "proc" => Ok((Source::Tree(String::from("/proc")), false, "proc")),
        "devtmpfs" => Ok((Source::Tree(String::from("/dev")), false, "devtmpfs")),
        _ => Err(-ENODEV),
    }
}

/// `mount(source, target, fstype, flags, data)`.
pub fn sys_mount(source: u64, target: u64, fstype: u64, flags: u64, data: u64) -> i64 {
    let Some(iso) = snapshot() else { return -ESRCH };
    if !iso.rights.has(Rights::MOUNT) {
        return -EPERM;
    }
    // The initial namespace is the kernel's base tree: unchanged behaviour (accepted, no effect).
    if iso.mnt.is_initial() {
        return 0;
    }
    let at = match dir_in_ns(&iso, target, true) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if flags & MS_SHARED != 0 || flags & MS_MOVE != 0 {
        return -EINVAL;
    }
    // every namespace is private already
    if flags & (MS_PRIVATE | MS_SLAVE | MS_UNBINDABLE) != 0 {
        return 0;
    }
    let read_only = flags & MS_RDONLY != 0;
    let done = if flags & MS_REMOUNT != 0 {
        iso.mnt.remount(&at, read_only)
    } else {
        match mount_source(&iso, source, fstype, flags, data) {
            Ok((src, src_ro, kind)) => iso.mnt.add(&at, src, read_only || src_ro, kind),
            Err(e) => return e,
        }
    };
    done.map(|_| 0).unwrap_or_else(|e| e.errno())
}

/// `umount2(target, flags)`.
pub fn sys_umount(target: u64, flags: u64) -> i64 {
    let Some(iso) = snapshot() else { return -ESRCH };
    if !iso.rights.has(Rights::MOUNT) {
        return -EPERM;
    }
    if iso.mnt.is_initial() {
        return 0;
    }
    let at = match dir_in_ns(&iso, target, flags & UMOUNT_NOFOLLOW == 0) {
        Ok(a) => a,
        Err(e) => return e,
    };
    iso.mnt.remove(&at, flags & MNT_DETACH != 0).map(|_| 0).unwrap_or_else(|e| e.errno())
}

/// `pivot_root(new_root, put_old)`: `new_root` must be a mount point, `put_old` a directory below it.
pub fn sys_pivot_root(new_root: u64, put_old: u64) -> i64 {
    let Some(iso) = snapshot() else { return -ESRCH };
    if !iso.rights.has(Rights::MOUNT) {
        return -EPERM;
    }
    if iso.mnt.is_initial() || iso.root != "/" {
        return -EINVAL;
    }
    let (new_root, put_old) = match (dir_in_ns(&iso, new_root, true), dir_in_ns(&iso, put_old, true)) {
        (Ok(n), Ok(p)) => (n, p),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let put_rel = match super::mount::below(&put_old, &new_root) {
        Some(r) if !r.is_empty() => String::from(r),
        _ => return -EINVAL,
    };
    // table and the roots/working directories of every group in the namespace change together
    let mut ps = PROCS.lock();
    if let Err(e) = iso.mnt.pivot(&new_root, &put_old) {
        return e.errno();
    }
    let mut seen: Vec<Shared<proc::Group>> = Vec::new();
    for p in ps.iter_mut().filter(|p| Arc::ptr_eq(&p.g.iso.mnt, &iso.mnt)) {
        if seen.iter().any(|g| g.same(&p.g)) {
            continue;
        }
        seen.push(p.g.clone());
        if p.g.iso.root == "/" {
            if p.g.cwd != "/" {
                p.g.cwd = repath(&p.g.cwd, &new_root, &put_rel);
            }
        } else {
            p.g.iso.root = repath(&p.g.iso.root, &new_root, &put_rel);
        }
    }
    0
}

/// `chroot(path)`: the directory becomes the root of the calling group; the working directory becomes `/`.
pub fn sys_chroot(path: u64) -> i64 {
    let Some(iso) = snapshot() else { return -ESRCH };
    if !iso.rights.has(Rights::MOUNT) {
        return -EPERM;
    }
    let root = match dir_in_ns(&iso, path, true) {
        Ok(r) => r,
        Err(e) => return e,
    };
    with(|p| {
        p.g.iso.set_root(root);
        p.g.cwd = String::from("/");
    });
    0
}

/// `unshare(flags)`: `CLONE_NEWNS` and `CLONE_NEWPID`; `CLONE_FS`/`CLONE_FILES`/`CLONE_SYSVSEM` are accepted.
pub fn sys_unshare(flags: u64) -> i64 {
    const ACCEPTED: u64 = CLONE_NEWNS | CLONE_NEWPID | 0x200 | 0x400 | 0x4_0000;
    if flags & !ACCEPTED != 0 {
        return -EINVAL;
    }
    with_threads(|p, threads| {
        if flags & (CLONE_NEWNS | CLONE_NEWPID) != 0 && !p.g.iso.rights.has(Rights::MOUNT) {
            return -EPERM;
        }
        // file tree view and descriptors are shared by the threads of a group and cannot be split off
        if flags & (CLONE_NEWNS | 0x200 | 0x400) != 0 && threads > 1 {
            return -EINVAL;
        }
        let mut child_ns = None;
        if flags & CLONE_NEWPID != 0 {
            if !Arc::ptr_eq(p.link.ns(), &p.g.iso.pid_ns_children) {
                return -EINVAL;
            }
            match PidNs::child(p.link.ns()) {
                Some(ns) => child_ns = Some(ns),
                None => return -ENOSPC,
            }
        }
        if flags & CLONE_NEWNS != 0 {
            let copy: Arc<MountNs> = p.g.iso.mnt.copy();
            p.g.iso.mnt = copy;
        }
        if let Some(ns) = child_ns {
            p.g.iso.pid_ns_children = ns;
        }
        0
    })
    .unwrap_or(-ESRCH)
}

/// `setns(fd, nstype)` with a descriptor of `/proc/<pid>/ns/mnt` or `/proc/<pid>/ns/pid`.
pub fn sys_setns(fd: u64, nstype: u64) -> i64 {
    with_threads(|p, threads| {
        let target = match p.fd(fd).map(|d| &d.kind) {
            Some(FdKind::Ns(r)) => r.clone(),
            Some(_) => return -EINVAL,
            None => return -EBADF,
        };
        if !p.g.iso.rights.has(Rights::MOUNT) {
            return -EPERM;
        }
        match target {
            NsRef::Mnt(ns) => {
                if nstype != 0 && nstype != CLONE_NEWNS || threads > 1 {
                    return -EINVAL;
                }
                // a narrowed root must not be widened by entering another namespace
                if p.g.iso.root != "/" {
                    return -EPERM;
                }
                p.g.iso.mnt = ns;
                p.g.iso.set_root(String::from("/"));
                p.g.cwd = String::from("/");
            }
            NsRef::Pid(ns) => {
                if nstype != 0 && nstype != CLONE_NEWPID || !p.link.ns().contains(&ns) || ns.is_dead() {
                    return -EINVAL;
                }
                p.g.iso.pid_ns_children = ns;
            }
        }
        0
    })
    .unwrap_or(-ESRCH)
}

/// `symlinkat(target, dirfd, linkpath)`: symbolic links exist on tmpfs mounts; elsewhere `EPERM` as before.
pub fn sys_symlink(target: u64, dirfd: i64, linkpath: u64) -> i64 {
    let (Some(target), Some(link)) = (ustr(target), ustr(linkpath)) else { return -EFAULT };
    if target.is_empty() {
        return -ENOENT;
    }
    if target.len() > 4095 {
        return -ENAMETOOLONG;
    }
    let loc = match locate(dirfd, &link, false) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if !tmpfs::is_private(&loc.host) {
        return -EPERM;
    }
    if loc.read_only {
        return -EROFS;
    }
    if link_at(&loc).is_some() || fs::stat(&loc.host).is_some() {
        return -EEXIST;
    }
    if !matches!(fs::stat(&parent(&loc.host)), Some((1, _))) {
        return -ENOENT;
    }
    if let Err(e) = crate::perm::may_create(&loc.host) {
        return e;
    }
    if tmpfs::symlink(&loc.host, &target) {
        crate::perm::created(&loc.host, crate::perm::S_IFLNK, 0o777);
        0
    } else {
        -EEXIST
    }
}
