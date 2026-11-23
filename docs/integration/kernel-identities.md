# Ultar kernel – identities, file modes, Unix socket nodes, fsync (review finding R3-K1)

Patch: `FIX2.patch` (`patch -p1` against repo commit 9175347). Finding: `docs/review/services-runde-3.md`,
R3-K1 – on the Ultar kernel neither identity mechanism of the bus worked (no permission on `connect`, `chmod`/
`chown` were no-ops, `bind` created no file, every regular file was `0o100755`, so `read_secret_file` refused
every token file).

## What changed

| File | Change |
|---|---|
| `src/cred.rs` (new) | `Cred`: real/effective/saved uid and gid, up to 16 supplementary groups; the id system calls |
| `src/perm.rs` (new) | mode/owner of every entry, the permission checks, `chmod`/`chown`/`access` families, `/tmp` and `/run` at boot |
| `src/uds.rs` (new) | socket node on `bind`, permission on `connect`, peer identity, `getsockopt` for Unix sockets (`SO_PEERCRED`) |
| `src/isolation/rights.rs` | rights `SET_ID` (bit 8) and `FILE_ADMIN` (bit 9); `Rights::ALL = 0x3ff` |
| `src/isolation/{mod,path,tmpfs,calls}.rs` | `Isolation.cred`; `View::host_of`; tmpfs `mode=`/`uid=`/`gid=`; attributes dropped with their entries; `symlink` checks the directory |
| `src/ext4.rs`, `src/fs.rs` | inode uid/gid read; `Ext4::set_attr` writes mode/uid/gid into the inode; `fs::disk_attr`/`set_disk_attr` |
| `src/syscall.rs` | dispatch of the new calls; search permission in `locate`; checks in `open`, `mkdir`, `unlink`, `rename`, `chdir`, `execve`; `stat`/`fstat` report mode, uid, gid; `fsync`/`fdatasync`; Unix `bind`/`connect`/`socketpair`/`getsockopt` |
| `src/pipe.rs`, `src/proc.rs` | identity of the writing end of a Unix connection; a closed listener leaves the listener list |
| `src/oom.rs` | race fix (see "Also fixed") |
| `src/main.rs`, `src/selftest.rs` | modules; `perm::standard_dirs()` after mounting; `rechtetest` in the hardware self test |
| `user/rechtetest.c` (new), `user/Makefile`, `scripts/build-kern.sh` | the self test (102 checks, marker `RECHTETEST: ALLES OK`), `--ext4 <dir>` mode |
| `user/zelltest.h`, `user/grenztest.h`, `tools/kern-test/attacks/ausbruch.c` | `R_ALL` is `0x3ff` (the two new rights); nothing else |
| `tools/kern-test/rechte-ext4.sh` (new) | ext4 disk (GPT name `ultar-data` → `/data`): write, fsync, chmod, chown, hard stop, then `debugfs`/`e2fsck` on the host |
| `tools/kern-test/ci.sh`, `tools/kern-test/target-stage1.sh` | run `rechtetest` / `rechte-ext4.sh`; verdict `RECHTETEST: ALLES OK` |

## 1. User and group ids

* Every thread group has `Cred` (in `Isolation`, so `fork`/`clone` copy it and `execve` keeps it). Programs the
  kernel starts are root: all ids 0, no groups – unchanged default.
* Set-user/group-id bits of files are **not** honoured on `execve` (stored and reported, nothing more).
* Calls (Linux numbers and semantics):
  `getuid` 102, `getgid` 104, `geteuid` 107, `getegid` 108, `getresuid` 118, `getresgid` 120, `getgroups` 115;
  `setuid` 105, `setgid` 106, `setreuid` 113, `setregid` 114, `setresuid` 117, `setresgid` 119, `setgroups` 116.
  Before this patch they all answered 0 and changed nothing.
* Rights check: with the right **`SET_ID`** (Linux `CAP_SETUID`/`CAP_SETGID`, capabilities 6/7 in
  `PR_CAPBSET_READ/DROP`) any id may be set (`setuid` with it sets all three, as root on Linux). Without it a
  process may only move between its current real/effective/saved ids (`EPERM` otherwise), exactly as an
  unprivileged Linux process. `setgroups` needs `SET_ID`; more than 16 groups: `EINVAL`.
* Ids and rights are independent: changing the uid does **not** drop rights (Linux would clear the capabilities
  on a change away from uid 0). A service that wants to be held by file modes must drop `FILE_ADMIN`; to change
  its ids it must do so **before** dropping `SET_ID`. `ultar-service` drops by default-deny mask (`!keep`), so
  both new rights are gone after its drop; `RightSet::ALL_BITS` there is still `0x7f` (only used in its own test).
* `setfsuid` 122 / `setfsgid` 123: return the effective id and change nothing (the file-system ids are the
  effective ids).
* `SO_PEERCRED` pid is translated into the reader's pid namespace (0 if the peer is not visible there).

## 2. File modes and owners

**Where they live**

| File system | Mode/owner storage | Survives restart |
|---|---|---|
| writable ext4 (`ultar-compat`, `ultar-data`) | the inode (`Ext4::set_attr`, synchronous block write) | yes |
| read-only ext4 | the inode (read only; `chmod`/`chown` → `EROFS`) | – |
| tmpfs (private trees) and RAM layer (incl. the overlay over the read-only FAT) | kernel table `perm::ATTRS` (host path → mode/uid/gid), dropped with the entry or the tree | no (like the data) |
| `/proc`, `/dev`, `/sys` (kernel trees) | none: their gates are rights; `chmod`/`chown` → `EPERM` | – |
| FAT files without an entry in the table | reported as before: root, `0755` | – |

* New entries: type | (`mode & ~umask`), owner = effective uid/gid of the creator (links `0777`). Entries made
  by `bind` are sockets (`S_IFSOCK`).
* `/tmp` (`1777`) and `/run` (`0755`) are created in memory at boot if the disk has none.
* tmpfs mount: root of the tree `mode=` (octal), `uid=`, `gid=`; default root `0755` (Linux: `1777`, stricter
  on purpose: whoever mounts a tmpfs for others says so).
* `stat`/`lstat`/`fstat`/`newfstatat`: mode, uid, gid as stored (before: every regular file
  `0o100755`, owner 0).

**Enforcement** (owner/group/other bits, effective ids, supplementary groups)

| Call | Check |
|---|---|
| every path call (`locate`) | search (`x`) on every directory along the path in the caller's view |
| `open`/`openat`/`creat` | `r`/`w` by access mode, `w` for `O_TRUNC`; new entry: `w`+`x` on the directory; a socket node: `ENXIO` |
| `mkdir`, `symlink`, `bind` | `w`+`x` on the directory |
| `unlink`, `rmdir`, `rename` | `w`+`x` on the directory; sticky directory: only the owner of the entry or of the directory (`EPERM`) |
| `chdir` | `x` on the directory |
| `execve` (program and `#!` interpreter) | `x`; with `FILE_ADMIN` still at least one `x` bit |
| `chmod` 90, `fchmod` 91, `fchmodat` 268, `fchmodat2` 452 | owner or `FILE_ADMIN` (else `EPERM`); read-only mount `EROFS` (were no-ops returning 0) |
| `chown` 92, `fchown` 93, `lchown` 94, `fchownat` 260 | `FILE_ADMIN`; without it the owner may only change the group to one of its own; without it set-id bits are cleared (were no-ops) |
| `access` 21, `faccessat` 269, `faccessat2` 439 | real ids unless `AT_EACCESS`; `w` on a read-only mount: `EROFS` |

* Bypass ("root") is the right **`FILE_ADMIN`** (Linux `CAP_CHOWN`, `CAP_DAC_OVERRIDE`,
  `CAP_DAC_READ_SEARCH`, `CAP_FOWNER`: capabilities 0–3), not uid 0 as such – uid 0 without `FILE_ADMIN` is
  checked like any owner. Programs the kernel starts have every right, so behaviour without a launcher is as before.
* Cells and memory groups: unchanged. Checks come after the view walk (a cell still sees only its view);
  `/proc`, `/dev`, `/sys` keep their right-based gates; the attribute table belongs to the kernel (shared, not
  charged to a group), entries are removed with files and whole tmpfs trees.

## 3. AF_UNIX

* `bind` to a path creates a socket node (empty RAM-layer entry with `S_IFSOCK`, owner = caller, mode
  `0777 & ~umask`) – also below a directory on disk (the node does not outlive a restart). Errors:
  `EADDRINUSE` if anything exists at the path, `ENOENT` without the directory, `EACCES` without `w`+`x` on it,
  `EROFS`, `EPERM` below `/proc`/`/dev` from a cell, `ENOSPC` if the tree is full.
* The node stays after `close` (Linux); `unlink` removes it; a new `bind` needs the path free.
* `connect`: `ENOENT` without node, `ECONNREFUSED` if the entry is not a socket or nobody listens on it
  (a listener that closed or was replaced), `EACCES` without `w` on the node (plus search on the path).
* Abstract names (`\0…`): no node, as before; `EADDRINUSE` if one is bound.
* Peer identity: `SO_PEERCRED` (`getsockopt(SOL_SOCKET, 17)`, `struct ucred` 12 bytes: pid, uid, gid; buffer
  smaller than 12: `EINVAL`) for connected stream sockets and `socketpair`; it is the identity at `connect`
  (client) / `bind`+`listen` (server) / `socketpair` time, as on Linux. Before it answered `ENOPROTOOPT`.
* `getsockopt` on Unix sockets also answers `SO_TYPE`, `SO_ERROR`, `SO_DOMAIN`, `SO_PASSCRED`,
  `SO_ACCEPTCONN`, `SO_SNDBUF`, `SO_RCVBUF`; other options `ENOPROTOOPT`.
* **`SCM_CREDENTIALS` is not implemented** (ancillary data is not carried on this kernel's Unix sockets; not
  cheap). `SO_PASSCRED` reads 0.

## 4. fsync / fdatasync (74, 75)

A file's data lives in the descriptor's buffer until it is stored (`close`, or now `fsync`).

| File system | What `fsync`/`fdatasync` does |
|---|---|
| writable ext4 | stores the buffer now; every disk write of this kernel is synchronous, so the blocks are on the device when the call returns |
| tmpfs, RAM layer (FAT overlay) | stores the buffer: visible to others; durable only as far as memory is |
| directory, read-only file | 0, nothing to do |
| pipe, socket, other descriptors | `EINVAL` (Linux) |

Before: 0 without doing anything (data reached the disk only at `close`).
**Not done:** no cache flush command is sent to the device. virtio-blk without `VIRTIO_BLK_F_FLUSH` negotiated
is write-through; NVMe (`FLUSH`) and AHCI (`FLUSH CACHE`) disks with a volatile write cache may still hold
the blocks after `fsync` returns. `sync` (162) returns 0 and does nothing beyond this.

## Tests

* `user/rechtetest.c` (102 checks, `RECHTETEST: ALLES OK`): ids (get/set families, rights check, inheritance by
  fork/exec, `setgroups`), modes and owners on tmpfs (create/umask, chmod/chown rules, open/exec/unlink/rename/
  sticky/search checks as another uid, rename keeps attributes), socket nodes (`EADDRINUSE`, unlink, `EACCES`
  on connect, `ECONNREFUSED` without listener, `ENXIO` on open, `SO_PEERCRED` values and errors), fsync per
  descriptor kind. Negative checks run as uid 1000/2000 without `FILE_ADMIN`.
* `tools/kern-test/rechte-ext4.sh`: ext4 on a second disk, `rechtetest --ext4 /data`, VM killed while the file
  is open, host checks size/content/mode `0640`/owner `1000:100`, directory `0750` `2000:200`, `e2fsck -fn` clean.
* Both are in `ci.sh`; `rechtetest` is in the hardware self test and the `target-stage1.sh` verdict.

## Also fixed

* `oom.rs`: two CPUs could both pass the "victim still dying" hold and kill two victims in a row (seen as a hang
  of `grenztest` with SMP=4 / 2 GiB after FIX1: the second kill hit the test parent). Now one CPU decides at a
  time (`DECIDING`) and the hold is set while the process lock is held.
* `fstat` looked up attributes after this patch while holding the process lock: moved out (a look into `/proc`
  takes it).

## Open

* `SCM_CREDENTIALS`, device cache flush (above), set-id bits on `execve`, POSIX ACLs/xattrs, `O_TMPFILE`.
* Attributes of RAM/tmpfs entries are not charged to a memory group (kernel table, bounded by the entries).
* Services: R3-K1's runtime half (warning `bus.identity.unenforced`, `--token-fd`) is not part of this patch;
  with this kernel the token file of `read_secret_file` works when created with `0600` on tmpfs/ext4.
