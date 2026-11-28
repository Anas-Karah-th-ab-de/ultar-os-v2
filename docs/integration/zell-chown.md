# Single files in a cell's view and `chown` inside a cell (stage P0 of `docs/FREMD-APPS.md`)

The Linux package tools need two things the Ultar kernel refused so far: a cell built from single
device nodes and sockets (`/dev/null`, `/dev/urandom`, a bus socket), and `chown` by the cell's root
while it installs packages (`dpkg` hands `/var/cache/apt/archives/partial` to `_apt`, files to
`root:shadow` …). Both are now possible without giving the cell `FILE_ADMIN`, and without a way out of
the cell. Companion of `docs/integration/zell-netz.md`.

## 1. Bind mounts of single files

| Call | Behaviour |
|---|---|
| `mount(src, dst, NULL, MS_BIND …)` | `src` and `dst` may be files of any kind: regular file, device node, the node of a bound Unix socket. The kinds must match as on Linux: a file onto a file, a directory onto a directory, else `ENOTDIR`. A missing `src` is `ENOENT` |
| other file systems (`tmpfs`, `proc`, `devtmpfs`, `cgroup2`) | still need a directory as mount point (`ENOTDIR` for a file) |
| `MS_REMOUNT` | accepts a file mount point; `MS_RDONLY` and `MS_NOEXEC` are kept per mount like for directories |
| `umount2` | accepts a file mount point; afterwards the file underneath shows again |
| `unlink`, `rmdir`, `rename` (old or new name) of a mount point | `EBUSY`, as on Linux. Before, the call went through to what the mount shows – a cell could have removed or replaced the bound host entry (a directory bind had the same gap) |

`nosuid` and `nodev` need no flag of their own: the kernel never honours set-id bits at `execve`, and
device nodes exist only in the kernel's `/dev` – a bound node is opened at its host path with that
node's gate (mode, `RAW_DEVICE`, or the service right of `/dev/ultar/*`). A cell cannot make a device
node (there is no `mknod`).

**Launcher.** The probe's bind of `/dev/null` onto a file now succeeds on the Ultar kernel
(`PROBE_FILE_BIND`), so cells get the device mode `binds` there as on Linux: a small tmpfs at `/dev`
with `null`, `zero`, `random`, `urandom` (`ultar_sys::cell::DEV_NODES`; nodes the kernel lacks, like
`full`, are skipped). A cell that keeps the kernel right `sound` (the system profile) also gets
`/dev/ultar/pcm` (`ultar_sys::cell::SOUND_NODE`). Before, a cell on the Ultar kernel saw the whole
`/dev` tree (gated by modes) – including the console and `tty`.

## 2. The right `cell_chown` (bit 14)

A new kernel right, far narrower than `file_admin`:

* it acts only in a **confined view** (a mount namespace of its own, or a root other than `/`);
* the entry must lie on one of the view's **writable mounts whose source is not the whole host tree**
  (a writable bind of the Linux area, a tmpfs of the cell). It is checked from the host side
  (`MountNs::owned_by_cell`): the entry must be visible at some path of the view where the visible
  mount (not one hidden below a later mount) is writable – so it holds for `fchown` through a
  descriptor that came from outside as well;
* never in the kernel's trees (`/proc`, `/dev`, `/sys`) and never on a read-only mount (`EROFS`);
* any owner and group may be set; the **set-id bits are always cleared**;
* without the right, outside a cell, or for entries elsewhere: `EPERM` as before;
* `PR_CAPBSET_DROP` of `CAP_CHOWN` drops it together with `file_admin`.

Names: `cell_chown` in `ultar-init` (`RIGHT_NAMES`, `ALL_RIGHTS = 0x7fff`), the launcher
(`KERNEL_RIGHT_NAMES`) and `ultar_service::Right::CellChown`. No profile and no shipped service keeps
it; the launcher drops every right a profile does not name (default deny). The install cell of foreign
Linux packages is meant to keep it – with its writable mounts being exactly the Linux area.

Why this is safe: a cell can already write its own writable mounts, so owning entries there gives
nothing it could not do with their contents. What `chown` is dangerous for on Linux is closed off:
taking over files of others outside the area (not reachable), handing out set-id programs (bits
cleared, never honoured), and the kernel's control files (excluded).

## 3. No set-id bits in a cell, and changes through a descriptor

Two rules for every confined caller without `file_admin` (they close gaps the install cell would have
opened, since it runs as uid 0):

* **set-id bits never appear**: `chmod`/`fchmod` and newly created entries (`open(O_CREAT)`, `mkdir`)
  lose `04000`/`02000` silently (Linux clears `S_ISGID` the same way); the sticky bit stays. `dpkg`
  goes on, its set-id programs come out without the bits – which the Ultar kernel ignores anyway; the
  rule keeps such files from leaving the cell for a system that would honour them;
* **`fchmod`/`fchown` need a writable mount of the view** (`EROFS` otherwise), as the path calls
  always did: before, a uid-0 cell could change the mode of a root-owned system file through a
  descriptor opened read-only from a read-only bind.

## 4. Tests

`tools/kern-test/zell-bind-e2e.sh` with `kernel/ultar-kern/user/zellbind.c` (verdict
`ZELL-BIND: ALLES OK`, in `tools/kern-test/ci.sh` after `ZELL-NETZ`):

* binds of `/dev/null` (writes vanish, reads are empty, `stat` shows 1:3), `/dev/zero`, `/dev/urandom`,
  `/dev/tty` onto files; a host file read-only (`EROFS`, also after the remount) and writable (the host
  sees what the cell wrote); the node of a host Unix socket – the cell connects, the host service gets
  its message;
* kind mismatch and `tmpfs` onto a file `ENOTDIR`, missing source `ENOENT`; `unlink`/`rename` of and
  over mount points and `rmdir` of a tmpfs mount point `EBUSY`; afterwards every bound host entry is
  still there; `umount2` of a file mount point shows the empty file again;
* cell root without `file_admin`: `chmod 4755` gives `0755`, `open(O_CREAT, 06755)` no set-id bits,
  the sticky bit stays; `chown`/`fchown` to `_apt` (42) and back in the Linux area, a directory to
  `_apt:root`, in the cell's tmpfs; `chown` clears set-id bits; a host file outside the area through the
  copied (writable) host tree `EPERM`; read-only host tree, read-only bind, `fchown`/`fchmod` through a
  read-only bind and through a descriptor opened before the cell `EROFS`; `/dev/null` `EPERM`; without
  the right (dropped with `CAP_CHOWN`) `EPERM`; a host process with the right but outside a cell
  `EPERM`; afterwards the host sees the owners the cell set in the area and nothing else changed.

Unit tests: the right lists of `ultar-service`, `ultar-init` and the launcher agree with the kernel
bits (`0x7fff`); `rechtetest`, `zelltest` and `grenztest` expect the new full set.

## 5. Not done / open

* No user namespaces: cell root is uid 0 of the machine, kept small by the missing rights and the
  read-only view. The ids `cell_chown` sets are the machine's ids (the Linux area's `_apt` is uid 42 of
  the machine).
* `setgid` directories do not pass their group to new entries (no inheritance in the kernel), and a
  set-id bit is simply lost in a cell – `dpkg --verify` reports such files.
* Remounting a bind without `MS_RDONLY` makes it writable again even if its source was read-only (only
  the first bind keeps the source's flags). Only a holder of `MOUNT` can do that; cells of apps drop it.
