# Ultar kernel – process isolation, stage 1 (PLAN.md §3.7)

Scope of this stage: **mount namespaces + pid namespaces + the foundation of per-process rights**, with escape
tests (`user/zelltest.c`). Everything else of §3.7 (net/uts/ipc/user/dev namespaces, the group tree, profiles)
is out of scope and listed at the end.

## 1. What exists today

* `proc.rs`: thread groups (`Group`: address space, fds, cwd, exe) and threads (`Proc`: pid/tgid/ppid/pgid, all
  global numbers), one global list `PROCS` under one spin lock, round-robin scheduler, `end_group` for exits.
* `fs.rs`: **one global, path-keyed tree**. A path string is looked up in the RAM layer, then in the disk mounts
  (`MOUNTS`, FAT/ext4, set once at boot), `/dev` and `/proc` are synthetic prefixes. There are no inodes or
  dentries; file descriptors remember the *path string* (`FdKind::File{path}`, `FdKind::Dir{path}`).
  Symbolic links exist only inside ext4 (followed inside `Ext4::lookup`, relative to that filesystem's root);
  `symlink(2)` returns `EPERM`.
* `syscall.rs`: Linux numbers. `resolve(dirfd, path)` = lexical normalisation against cwd or the directory fd.
  `mount`/`umount2` (165/166) are **no-op stubs returning 0**; `chroot`, `pivot_root`, `unshare`, `setns` are
  not implemented (`ENOSYS`); `clone` silently ignores every `CLONE_NEW*` flag; `prctl` returns 0 for
  everything; every process is "root" and may do everything (reboot, kill anything, open the frame buffer,
  write the firewall).
* No capabilities, no sandbox, no cgroups in the kernel (`docs/legacy/KERN.md` K4 lists namespaces as open;
  `SICHERHEIT-KERN.md` names "rights per program" as the next step; `ZELLEN.md` describes cells on Linux).

## 2. Model

Three coordinate systems for paths:

| Name | Meaning |
|---|---|
| host path (H) | key in the kernel's global tree, what `fs::open` takes (unchanged) |
| namespace path (N) | path in the mount tree of one mount namespace; the mount table maps N → H |
| view path (V) | what the process passes to system calls; `N = root ⊕ V` (`root` = chroot directory) |

**Every** path a process hands to the kernel is a V path. It is normalised component by component in V
coordinates (`..` is clamped at the view root, symbolic links are resolved in V coordinates, an absolute link
target restarts at the *view* root) and only the final result is translated V → N → H. Nothing a process stores
(cwd, exe, the base of a directory fd) is ever an H path that is fed back into resolution, so there is no input
from which a path outside the root could be formed. That is the whole escape argument; the tests attack each
of its three inputs (`..`, links, directory fds).

### 2.1 Data structures (new module `src/isolation/`)

```
MountNs   { id, epoch: AtomicU64, mounts: Mutex<Vec<Mount>> }      // table is never empty: "/" always exists
Mount     { at: String (N path), source: Source, read_only: bool }
Source    = Tree(String)              // a subtree of the host tree (bind, proc, devtmpfs, the initial "/")
          | Tmpfs(Arc<Tmpfs>, sub)    // (a directory of) a private RAM tree; freed with its last mount
PidNs     { id, level, parent: Option<Arc<PidNs>>, next: AtomicU64, init: AtomicU64, dead: AtomicBool,
            map: Mutex<Vec<(local, global)>> }
PidLink   { ns: Arc<PidNs>, global }  // one per thread; Drop removes its numbers from every level
Rights(u64)                           // bit set, only ever shrinks
Isolation { mnt: Arc<MountNs>, root: String (N path), root_id, pid_ns_children, rights }           // in Group
ViewTag   { ns id, ns epoch, root id }  // identity of a view, stored in every directory descriptor
```

* `Group` gets one field `iso: Isolation` (threads of a group share view, pid namespace and rights – a thread
  with other rights than its siblings in the same address space would be meaningless) and `exe_host` (the host
  path of the running program, for `/proc/<pid>/exe`; `exe` stays the view path the process itself sees).
* `Proc` gets one field `link: PidLink` (it also names the pid namespace the thread lives in).
* Files: `isolation/{mod,rights,pid,signals,mount,tmpfs,path,calls}.rs`; hooks in `proc.rs` (fields, exit of
  pid 1, `/proc` view), `syscall.rs` (`locate` replaces `resolve`, pid translation), `fs.rs` (private
  prefix, `disk_link`), `ext4.rs` (`link_at`), `procfs.rs` (reader's view, `ns/`).
* Kernel-internal numbers stay **global** (`pid`, `tgid`, `ppid`, `pgid`, scheduler, `Wait::Child`); numbers
  are translated only at the system-call boundary and in `/proc`.
* A tmpfs is stored in the existing RAM layer under a host prefix that contains a NUL byte
  (`/\0tmpfs/<id>`). No path coming from a C string can contain NUL, so it is unreachable except through a
  mount; `fs.rs` treats the prefix as RAM-only and hides it from the listing of `/`.
* Lock order: `PROCS` → (`MountNs.mounts` | `PidNs.map` | tmpfs registry → `RAM`). `PROCS` is never taken
  while one of the inner locks is held. Path walking snapshots the view under `PROCS` and walks without it
  (because `/proc` lookups take `PROCS` themselves).

### 2.2 Mount namespace

* `clone(CLONE_NEWNS)` / `unshare(CLONE_NEWNS)`: new namespace with a **copy** of the caller's table. All
  namespaces are private; there is no propagation (`MS_PRIVATE`/`MS_SLAVE` are accepted as no-ops, `MS_SHARED`
  is `EINVAL`).
* `mount(2)` inside a non-initial namespace: `tmpfs`/`ramfs` (fresh private RAM tree), `MS_BIND` (alias of an
  existing directory of the caller's view; sub-mounts of the namespace are not carried over, i.e. non-recursive),
  `proc` (pid-namespace aware, see 2.3), `devtmpfs`. `MS_RDONLY` is recorded and enforced (`EROFS` on
  create/write-open/mkdir/unlink/rename/symlink); `MS_REMOUNT` changes only that flag. Anything else: `ENODEV`.
  At most 256 mounts per namespace (`ENOSPC`).
* `umount2(2)`: the target must be a mount point (`EINVAL`), `/` cannot be unmounted (`EINVAL`), child mounts
  → `EBUSY` unless `MNT_DETACH`, which removes the whole subtree.
* `pivot_root(new_root, put_old)`: `new_root` must be a mount point, `put_old` a directory strictly below it;
  the table is re-prefixed (mounts under `new_root` move to `/`, everything else under `put_old`), the
  namespace epoch is bumped. `umount2(put_old, MNT_DETACH)` afterwards removes every mount of the old tree from
  the table: after that there is no table entry through which an outside path could be named at all.
* `chroot(2)`: sets `root` of the calling group (N path). Works in every namespace including the initial one.
* **The initial namespace is the kernel's base tree and is not changed by `mount(2)`**: there `mount`/`umount2`
  keep today's behaviour (return 0, no effect; only the new right check is added). Reason: kernel-internal
  code (console, self test, DHCP writing `/etc/resolv.conf`, `/run/ultar/...`) works on host paths, and
  existing init programs call `mount` and expect 0. Making the base tree mutable is a later step.
* Deviation from Linux, deliberate: `chroot`, `pivot_root`, `setns(mnt)` set cwd to `/` of the new view
  (on Linux a stale cwd outside the new root is the classic chroot escape; here cwd is a V path and can never
  point outside, resetting it only avoids a silently re-interpreted cwd).
* Symbolic links: `symlink(2)`/`symlinkat(2)` work on tmpfs mounts (elsewhere `EPERM` as before, nothing
  changes in the legacy tree). `readlink`, `lstat`, `O_NOFOLLOW`, `unlink`, `rename` handle them. For a
  *confined* view (not initial namespace with root `/`), ext4 links are also resolved by the walker (ext4's own
  internal following would resolve an absolute target against the filesystem root, i.e. outside a bind
  mount). The unconfined legacy view keeps the old behaviour and the old cost exactly.

**The fd rule.** A directory fd remembers the view path it was opened with and a *view tag*
`(mount namespace id, namespace epoch, root id)`. `openat`/`fstatat`/`mkdirat`/`unlinkat`/`renameat`/
`readlinkat`/`faccessat`/`symlinkat` with that fd as base:
1. tag differs from the caller's current view (other namespace, `pivot_root` or `chroot` since the open)
   → `EXDEV`;
2. otherwise the relative path is resolved from the remembered *view* path inside the caller's view (so
   `fd + "../../.."` is clamped at the root like any other path).
Reading the listing of an inherited directory fd (`getdents`) and reading/writing inherited *file* fds stays
possible: an open file is a deliberate hand-over by whoever opened it (that is how a launcher passes a single
file into a cell). `fchdir` does not exist in this kernel (`ENOSYS`), so a directory fd cannot become cwd.

### 2.3 PID namespace

* `clone(CLONE_NEWPID)`: the child is pid 1 of a new namespace. `unshare(CLONE_NEWPID)`: the *next children*
  go into a new namespace (the caller keeps its pid), like Linux. Nesting up to 8 levels (`ENOSPC` beyond).
* Every thread has one number per level from its own namespace up to the initial one. A process is *visible*
  in a namespace iff it has a number there (its own namespace and all ancestors). The parent namespace sees the
  real (its own) numbers.
* Translated at the boundary: `getpid`, `gettid`, `getppid` (0 if the parent is not visible), `getpgid`/
  `getpgrp`/`getsid`/`setsid`, `set_tid_address`, return value of `fork`/`vfork`/`clone` and
  `PARENT_SETTID`/`CHILD_SETTID`, `wait4` (argument and result), `kill`, `tkill`, `tgkill`.
  `kill(-1)`, `kill(0)` and `kill(-pgid)` only reach visible processes (`kill(-1)` never the pid 1 of the
  sender's own namespace). An invisible pid is `ESRCH`.
* pid 1 semantics: (a) when the first process of a namespace ends, every other process in the namespace (and
  nested ones) is killed with `SIGKILL` and the namespace is closed (`fork` into it → `ENOMEM`, as Linux);
  (b) a signal sent to the namespace's pid 1 **from inside** is dropped unless pid 1 installed a handler for
  it (`SIGKILL`/`SIGSTOP` from inside are always dropped); from an ancestor namespace it is an ordinary process.
  Not done: re-parenting of orphans to pid 1 – this kernel has no re-parenting at all (orphans get ppid 0 and
  are reaped by the kernel), and that stays the same inside a namespace.
* `/proc` shows the **reader's** pid namespace: only visible processes, with the reader's numbers; `self` is
  the reader. `/proc/<n>` of an invisible process does not exist (`ENOENT`). `/proc/ultar/*` (kernel log,
  firewall, mesh) exists only for readers in the initial pid namespace. `/proc/<pid>/ns/{mnt,pid}` can be
  opened (namespace handle fd) and `readlink` gives `mnt:[id]` / `pid:[id]`.
  (Linux binds a proc mount to the pid namespace of the mounter; binding to the reader is simpler and never
  shows more than the reader may see.)
* `setns(fd, CLONE_NEWNS)`: caller must hold `MOUNT`, be single-threaded and not be chrooted; root and cwd
  become `/` of the target. `setns(fd, CLONE_NEWPID)`: target must be the caller's pid namespace or a
  descendant; affects future children.

### 2.4 Rights

Bit set per thread group, all bits set for programs started by the kernel (so nothing changes for existing
programs), inherited by `fork`/`clone`/`execve`, can only be dropped, never regained (there is no call that
sets a bit).

| Right | Linux capability it answers to | Checked at |
|---|---|---|
| `MOUNT` | `CAP_SYS_ADMIN` (21) | `mount`, `umount2`, `pivot_root`, `chroot`, `unshare`/`clone` with `CLONE_NEW*`, `setns` → `EPERM` |
| `NET_ADMIN` | `CAP_NET_ADMIN` (12) | opening `/proc/ultar/firewall`, `/proc/ultar/verbund` for writing → `EACCES` (and nothing can be renamed into `/proc` or `/dev`: `EPERM`) |
| `LOAD_DRIVER` | `CAP_SYS_MODULE` (16) | `init_module`, `finit_module`, `delete_module` → `EPERM` (with the right: `ENOSYS` as today) |
| `KILL_ANY` | `CAP_KILL` (5) | `kill`/`tkill`/`tgkill`: without it only the own group and descendants → `EPERM` |
| `SET_TIME` | `CAP_SYS_TIME` (25) | `settimeofday`, `clock_settime`, `adjtimex` → `EPERM` (with the right: `ENOSYS` as today) |
| `RAW_DEVICE` | `CAP_SYS_RAWIO` (17) | opening `/dev/fb0`, `/dev/input/*` → `EACCES` |
| `REBOOT` | `CAP_SYS_BOOT` (22) | `reboot` → `EPERM` |

Interface: `prctl(PR_CAPBSET_READ=23, cap)` → 1/0 and `prctl(PR_CAPBSET_DROP=24, cap)` with the Linux
capability numbers above (existing container tools already issue these), plus Ultar-native
`prctl(PR_ULTAR_RIGHTS_GET=0x554c5200)` → bit mask and `prctl(PR_ULTAR_RIGHTS_DROP=0x554c5201, mask)`.
Profiles (§3.7) will later be applied by the kernel at process start through the same drop path.

## 3. Linux-compatible calls added or changed

`clone` (56: `CLONE_NEWNS`, `CLONE_NEWPID`; other `CLONE_NEW*` → `EINVAL` instead of being ignored),
`unshare` (272), `setns` (308), `mount` (165), `umount2` (166), `pivot_root` (155), `chroot` (161),
`symlink` (88), `symlinkat` (266), `prctl` (157), `readlink`/`readlinkat`, `lstat`/`newfstatat(AT_SYMLINK_NOFOLLOW)`,
`open`/`openat` (`O_NOFOLLOW`, fd rule, device right), all path calls (view translation), all pid calls (2.3),
`reboot`, `init_module`/`delete_module`/`finit_module`, `settimeofday`/`clock_settime`/`adjtimex` (right checks).

## 4. Deliberately out of scope for stage 1

* net / uts / ipc / user / dev namespaces (`clone`/`unshare` with those flags fail with `EINVAL`).
* The group tree (cgroups): no memory/CPU/process-count limits; a tmpfs has no size limit (it lives in the same
  RAM layer as today's `/tmp`).
* System-call filters, per-path file permissions, uid/gid (every process is still uid 0), signed profiles.
* Mutable initial namespace (`mount` in the base tree), mount propagation, recursive bind, bind of single
  files, real device/disk filesystems inside a namespace, `mount_setattr`/`open_tree`/`move_mount`, `openat2`.
* Re-parenting of orphans to a namespace's pid 1; `fchdir`; `SCM_RIGHTS` (not in the kernel yet, so a directory
  fd cannot be sent into a cell anyway).
* Unix-socket paths are translated through the view (so they are separated by the mount namespace), abstract
  sockets are not separated further (that belongs to the net namespace stage).

## 5. How a launcher builds a cell (what `zelltest` does)

```
clone(CLONE_NEWNS | CLONE_NEWPID | SIGCHLD)          child is pid 1 of the cell
mount("tmpfs", <dir>, "tmpfs")                        the cell's root (must be a mount point)
mount(<host dir>, <dir>/bin, MS_BIND | MS_RDONLY)     what the cell may see of the host, read-only
pivot_root(<dir>, <dir>/alt); chdir("/"); umount2("/alt", MNT_DETACH)
mount("proc", "/proc", "proc")                        optional: shows only the cell's processes
prctl(PR_ULTAR_RIGHTS_DROP, mask)                     at least MOUNT, so the view is final
execve(...)
```
Descriptors the launcher leaves open are handed over on purpose (files: usable; directories: listable, not a
base for paths; a namespace handle is only usable with `MOUNT`). A launcher closes what the cell must not get.
