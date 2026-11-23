# Ultar kernel – no panic without memory, resource groups with limits: integration notes

Fix for finding **K5** ("running out of memory is a kernel panic – a cell without rights halts the whole
machine") = stage 2 of PLAN.md §3.7 (groups with limits). Design, rules and the argument why the machine keeps
running: `kernel-memory-groups-design.md`. Patch: `CHANGES.patch` (`patch -p1` in the repo root).

Result in one line: the four attacks (`ausbruch --dos mem|tmpfs|pipe|fork`) and five more floods end with the
attacker refused or ended, the machine alive, all memory back and nothing left charged; normal work is not
disturbed; explicit limits are exact; the bare paths cost 5–15 % more instructions, files and pipes got much
faster (numbers: design §9).

## What changed

| File | Change |
|---|---|
| `src/mem.rs`, `src/buddy.rs` (rewritten, English) | page allocator: buddy with merge, two zones (DMA below 4 GiB), single-page cache, per-page owner; kernel reserve; `alloc_page` with policy |
| `src/slab.rs`, `src/heap.rs`, `src/trybuf.rs` (new) | kernel heap on the page allocator: slabs (≤ 2 KiB), chunks (< 64 KiB), page runs; emergency heap; owner header; charge context, `scope`, `kernel`, `fallible`; `mem::try_vec`/`try_with_capacity`/`try_reserve`/`try_copy` |
| `src/groups/` (new: `mod`, `tree`, `pids`, `stats`, `policy`, `files`) | group tree, counters, explicit limits, dynamic rules, choice of the victim, the `cgroup2`-style file tree |
| `src/oom.rs` (new) | what happens on a refused page fault, the watch at scheduling points, end of a process over its limit |
| `src/pipe.rs` (new) | bounded pipe (moved out of `proc.rs`), `F_SETPIPE_SZ`, largest size |
| `src/paging.rs` | every mapping function can fail (`Option`/`Fault`); resident counter; at most 65530 reserved ranges; `unreserve` without a temporary copy |
| `src/proc.rs` | `Group.res`, `Proc.pids`, `Proc.io_done`; programs of the kernel start in `session`/`system`; fault path; context per system call; fallible loader, kernel stack, descriptor table, argument vectors; ELF address checks; `group_members`, `move_group` |
| `src/syscall.rs` | see "System calls" below |
| `src/fs.rs`, `src/ext4.rs` | RAM layer holds shared buffers (`Arc`), `store_file`/`grow`/`ram_usage`, quota through `isolation::tmpfs`; fallible file reads; `stat` of a RAM directory without building the listing; hooks for the group tree; `/sys` |
| `src/isolation/tmpfs.rs` | quota per tree (tmpfs and the host's RAM layer), `size=` |
| `src/isolation/{calls,rights,mount,pid}.rs` | `mount` data argument, type `cgroup2`; right `MANAGE_GROUPS`; initial namespaces charged to the kernel |
| `src/socket.rs` | socket buffers charged to the creator's group and fallible; per-group share of the socket table; bounded send queue and datagram backlog; network tables charged to the kernel |
| `src/procfs.rs`, `src/shell.rs`, `src/signal.rs`, `src/percpu.rs`, `src/main.rs`, `src/selftest.rs` | `/proc/ultar/gruppen`, `/proc/<pid>/cgroup`, `/proc/sys/fs/pipe-max-size`; console command `gruppen`; partial write and signals; per-processor context; modules; `grenztest` in the hardware self test |
| `user/grenztest*.c/.h` (new), `user/Makefile`, `scripts/build-kern.sh` | the tests (130 checks); `grenztest` and the reviewer's `ausbruch` are built and copied to the image |
| `user/zelltest.h`, `tools/kern-test/attacks/ausbruch.c` | `R_ALL` is `0xff` now (the new right; `0x3ff` since R3-K1); nothing else |
| `tools/kern-test/ci.sh` | step 6: `grenztest` with 512 MiB and 2 GiB (`GRENZ_MEM`, `GRENZ_LOG`) |

## System calls: added / changed behaviour

General: a call that fails after the kernel refused it memory returns **`ENOMEM`** (also where the code's own
error would have been `EFAULT`/`ENOENT`/`ENOEXEC`). A **page fault** that cannot get a page ends the process
with status 9 (as `SIGKILL`) or – when the machine as a whole is short – waits until the kernel has ended the
victim (design §4, §6). Nothing below panics.

| Call | Change |
|---|---|
| `read`, `pread64`, `readv` | no kernel buffer of the caller's size for files, `/dev/zero`, `/dev/urandom`; pipe: `ENOMEM` possible |
| `write`, `writev`, `sendto`, `sendmsg`, `sendmmsg` on a pipe / Unix socket | bounded buffer (64 KiB pipe, 256 KiB per socket direction). Blocking: waits for room, delivers everything (`writev`/`sendmsg`: a short count when the buffer fills). `O_NONBLOCK`: `EAGAIN` when full, short count when only part fits. ≤ 4096 bytes: all or nothing. `EPIPE` after part was delivered returns the count. `sendmsg`: one message is at most 1 MiB in total |
| `write`, `pwrite64`, `ftruncate` on a file | `ENOSPC`: quota of the tmpfs / RAM layer; `ENOMEM`: group limit or memory; `EFBIG` (64 MiB per file) as before. A buffer shared with the stored file is copied on the first write |
| `write` to a file of the group tree or `/proc/sys/fs/pipe-max-size` | takes effect at once and reports its error: `EPERM` (no right), `EINVAL`, `EBUSY`, `ESRCH`, `EAGAIN`, `EACCES` (read-only file) |
| `open`, `openat` | `EMFILE` beyond 65536 descriptors; `ENOMEM` if the file cannot be held; `O_CREAT`: `ENOSPC` if the RAM tree is full, `EPERM` below `/proc` and in the group tree; opening a group file or `pipe-max-size` for writing without `MANAGE_GROUPS`: `EACCES` |
| `close` | writes back without copying; a file whose tree has no room is not stored (cannot happen for a single writer: `write` reserves the room) |
| `pipe`, `pipe2`, `socketpair`, `eventfd`, `timerfd_create`, `epoll_create`, `signalfd`, `dup`, `dup2`, `dup3`, `fcntl(F_DUPFD*)`, `accept` | `EMFILE` (65536) / `ENOMEM` from the descriptor table instead of unbounded growth |
| `fcntl` | new: `F_SETPIPE_SZ` (1031; rounded up to a page; `EBUSY` if more is buffered; above the largest size `EPERM` without `MANAGE_GROUPS`), `F_GETPIPE_SZ` (1032) |
| `poll`, `select`, `epoll_wait` | a pipe / Unix socket is writable only with ≥ 4096 bytes of room |
| `epoll_ctl(ADD)` | `ENOMEM` beyond 65536 entries |
| `brk` | growth that is refused leaves the break unchanged (glibc: `ENOMEM`) |
| `mmap` | `ENOMEM`: more than 65530 reserved ranges, or (file mappings, populated at once) a refused page – what the call mapped is taken back. Anonymous mappings stay lazy |
| `munmap` | `ENOMEM` if a range would have to be split beyond 65530 |
| `fork`, `vfork`, `clone` | `EAGAIN`: `pids.max` (explicit or dynamic) of the group or an ancestor; `ENOMEM`: no memory for the copy (nothing stays behind); with `CLONE_NEWNS`/`CLONE_NEWPID` the child gets its own group (`ENOSPC` if no group slot is free) |
| `execve` | `ENOMEM` if the image cannot be loaded (the caller keeps running); `E2BIG` for more than 192 KiB per vector; `ENOEXEC` for segment addresses outside the user area |
| `mount` | 5th argument is read: tmpfs option `size=<bytes>[k|m|g]` or `size=<n>%`, `EINVAL` if malformed; new type `cgroup2` (the group tree, inside a mount namespace) |
| `mkdir`, `rmdir`, `unlink` | in the group tree: create / remove a group (`EPERM`, `EEXIST`, `EINVAL`, `EBUSY`, `ENOSPC`); in a full tmpfs: `ENOSPC` |
| `symlink`, `symlinkat`, `rename*` | `ENOSPC` when the tmpfs is full (`rename` no longer copies the data) |
| `sendfile` | at most 16 MiB per call; short count / `EAGAIN` / waits on a full pipe; the input position moves by what was written |
| `getdents64` | at most 64 KiB per call; `ENOMEM` possible |
| `statfs`, `fstatfs` | a path in a RAM tree reports that tree's quota and use (`f_type` tmpfs) |
| `socket` | `ENFILE`: 4096 sockets in all, 512 for a group below `apps`/`cells` |
| `sendto`/`write` (UDP, TCP), `recv*` | UDP payload over 65507 bytes: `EMSGSIZE`; TCP: `ENOBUFS` when the send copy is refused; `recv`: `ENOMEM`; incoming data the owner's group has no memory for is dropped |
| `prctl` | `PR_ULTAR_RIGHTS_GET` returns `0xff` for all rights; new bit `0x80` = `MANAGE_GROUPS`; `PR_CAPBSET_READ/DROP` know capability 24 (`CAP_SYS_RESOURCE`). Since R3-K1 all rights are `0x3ff` (`SET_ID`, `FILE_ADMIN`: `kernel-identities.md`) |
| `getrandom` | `ENOMEM` possible |
| `sysinfo`, `/proc/meminfo` | total = memory managed by the page allocator (slightly less than before: without the page records and the emergency heap); free now includes what the former fixed heap held unused |

New files: `/sys/fs/cgroup/**` (below), `/proc/ultar/gruppen` (tree + rules + counters; host readers only),
`/proc/<pid>/cgroup` (`0::<path>`; a reader inside a pid namespace sees `0::/`), `/proc/sys/fs/pipe-max-size`.
`/` lists `sys`.

### The group tree (`/sys/fs/cgroup`)

Directories `system`, `session`, `apps`, `cells`; `mkdir`/`rmdir` below them. Files per group:
`cgroup.procs` (read: pids; write a pid or `0` = self to move), `cgroup.controllers`, `cgroup.events`,
`memory.current`, `memory.peak`, `memory.max` (write bytes, `K`/`M`/`G`, or `max`), `memory.min` (read: the
protected minimum of the dynamic rules), `memory.events` (`max`, `oom_kill`), `pids.current`, `pids.peak`,
`pids.max` (write a number or `max`), `pids.events`, `ultar.pids.effective`, `ultar.memory.pressure`.
Not Linux: `memory.max` below the current use → `EBUSY`; no process directly in `/`, `apps`, `cells` → `EBUSY`;
no `cgroup.subtree_control` (both controllers are always on), no `memory.high`/`memory.low`/`swap`, no CPU or IO files.

## How to wire it

* Kernel log lines of this area are codes, not sentences (rule `text` of `ultar-check`): `[mem] oom-kill group|largest-of <path> free=… procs=… used=…`
  and `[mem] oom-end <name> pid=… group=… reason=group-limit|over-share|no-memory|over-limit`.
* A launcher of a cell: `clone(CLONE_NEWNS|CLONE_NEWPID)` as before – the child is in `cells/cell-<pid>`
  automatically. Set limits from outside through that directory, and **drop rights with `~0UL`**, not `0x7f`
  (a launcher that drops the old mask keeps `MANAGE_GROUPS`).
* `ultar-init` (group `system`) should put the surface into `session` and each app into `apps/<id>`
  (`mkdir` + write the pid to `cgroup.procs` before `execve`); until it does, its children stay in `system`,
  which the dynamic rules never refuse or end.
* Kernel code that holds a table shared by all processes and grows it inside a system call: wrap the growth in
  `heap::kernel(…)`; code that builds a buffer whose size a caller chooses: `mem::try_*` and return the error.
* `BUSCK=<file>` for `ci.sh` as before when building outside the workspace.

## Gate (final tree)

Build: `IMG=1 bash scripts/build-kern.sh` → 41 compiler warnings, the same count as the untouched tree (no new
kind of warning; `logs/gate-N.build.log`).

`tools/kern-test/ci.sh` three times with `SMP=2`, once with `SMP=4` (`logs/gate-summary.txt`, full logs
`logs/gate-N*.log`):

```
== gate run 1 (SMP=2)
ok    alle Geräte haben einen Treiber
ok    Ergebnis: ALLES OK
ok    BUS SELFCHECK OK
ok    ZELLTEST: ALLES OK
[kern] bereit nach 158 ms
ok    ZELLTEST-EXT4: ALLES OK
ok    GRENZTEST: ALLES OK (512 MiB, 130 checks)
ok    GRENZTEST: ALLES OK (2048 MiB, 130 checks)
== gate run 2 (SMP=2)   … identical, bereit nach 158 ms
== gate run 3 (SMP=2)   … identical, bereit nach 157 ms
== gate run 4 (SMP=4)   … identical, bereit nach 223 ms
```

Every run: `zelltest` 129 `ok`, `grenztest` 130 `ok` at each size, 0 `FEHLER`, no panic, emergency heap used
0 times, 0 bad frees, 5 kills by the kernel (all intended by the tests). The kernel's own charged memory before
and after `grenztest`: 259 920 → 261 792 bytes (512 MiB), 259 920 → 265 136 bytes (2 GiB).
Additionally once with 6 GiB (`-m 6144`, memory above 4 GiB in use): `kerntest`, `zelltest`,
`grenztest positiv`, `grenztest grenzen` all OK.

Excerpt (run 3, 512 MiB):

```
ok     Grenze: Prozess wird an der Grenze beendet (SIGKILL), nicht der Rechner
ok     Grenze: genau bis zur Grenze – 31 MiB bekommen, das 32. nicht mehr ganz
ok     Grenze: memory.current und memory.peak nie über memory.max
ok     Grenze pids.max = 6: genau 5 Kinder neben dem Prozess selbst, das 6. -> EAGAIN
DOS mem: Zelle ohne Rechte (Rechte=0) beginnt
[mem] oom-end ausbruch pid=354 group=/cells/cell-354 reason=over-share
DOS mem: Wirt lebt noch, Zelle endete mit Status 0x9
ok     Angriff Speicher (3 GiB berühren): endet von selbst – abgewiesen oder beendet
ok     Angriff Speicher (3 GiB berühren): Wirt lebt, ein normales Programm startet und endet mit Erfolg
ok     Angriff Speicher (3 GiB berühren): Speicher ist wieder frei, `cells` beim alten Stand, kein Rest
  tmpfs: write endete nach 60 MB mit No space left on device
  fork: endete nach 56 Prozessen mit Cannot allocate memory
[mem] oom-kill group /cells/cell-444 free=5260KiB procs=1 used=52605KiB
[mem] oom-kill group /cells/cell-443 free=4096KiB procs=1 used=27981KiB
[mem] oom-kill largest-of /session free=4096KiB procs=1 used=19660KiB
ok     Reihenfolge: zuerst die grössere Zelle, dann die kleinere, zuletzt das Sitzungsprogramm
ok     Reihenfolge: der Systemdienst, der den Speicher nahm, wurde nicht beendet
ok     Abrechnung: der Kern selbst hält am Ende nicht mehr als am Anfang (± 64 KiB), apps und cells sind leer
GRENZTEST: ALLES OK
```

## What the tests cover (`grenztest`, sections `positiv | grenzen | angriffe | dynamik`)

* **Normal work is not disturbed**: 40 % of free memory (≤ 512 MiB) allocated, checked, freed, and free again;
  200 processes in sequence, 3 × 40 at once; pipe rules (64 KiB, `EAGAIN`, atomic ≤ 4096, short counts,
  `F_SETPIPE_SZ`), content through a pipe in odd pieces, 24 MiB through a pipe and 8 MiB through a socket pair
  with a reader; tmpfs with `size=4m` (inside, over → `ENOSPC`, room again after delete, empty entries and
  links count) and with the default quota; the host's RAM layer.
* **Explicit limits**: `memory.max` 32 MiB – 31 MiB granted, the process ended at the limit, `memory.peak` ≤ max;
  8 MiB – `brk` and file writes get `ENOMEM`, small calls and `fork` go on; `pids.max` 6 – exactly 5 children,
  then `EAGAIN`; after the processes end `memory.current` = 0 and `pids.current` = 0; the tree's error cases;
  everything refused without `MANAGE_GROUPS`; a cell's own group appears, counts its memory and disappears.
* **Attacks by a cell without rights**: the reviewer's four (`mem`, `tmpfs`, `pipe`, `fork`), many 16-MiB files,
  floods of symbolic links, 2000 full pipes, descriptors up to the table's end, 65530 mappings. After each: a
  normal program runs, free memory is back (within 8 MiB – heap blocks kept for reuse), `cells` is charged
  exactly what it was before, no removed group has memory left.
* **Dynamic rules**: a cell is refused exactly where the protected minimum of `system`/`session` begins and a
  session program still gets that minimum; with the minimums in use the largest cell is refused at the low
  watermark while a small one still gets memory; at the critical line the kernel ends the larger cell, then
  the smaller, then the largest session program, never the `system` process that took the memory.
* At the end: no bad free in the page allocator, the kernel's own memory as at the start.

## Not done / not tested (honest list)

* **CPU weight** is not implemented: the scheduler is a round-robin without weights; no CPU files in the tree.
  IO, network and GPU limits of §3.7: not part of this stage.
* **Not every large allocation is fallible.** Converted are the paths a process drives by a size it chooses
  (design §4). Still forced (served from the reserve / emergency heap, a panic only if those are exhausted or
  too fragmented for the block): copy of the epoll list in `epoll_wait` (≤ 1 MiB), frame-buffer reads
  (≤ 16 MiB, needs `RAW_DEVICE`), `/proc` listings and the scheduler's wake-up table (grow with the number of
  threads), the process table, FAT/ext4 metadata and directory listings of the host's disks, the dynamic
  loader's file. The panic handler itself is unchanged.
* Memory is **not moved** with a process between groups (charged where it was allocated, like Linux).
  `unshare()` of namespaces does not create a group – only `clone` does.
* `memory.max` cannot be set below the current use (`EBUSY`); there is no reclaim, no swap, no
  `memory.high`. At an explicit limit the kernel ends the process that faults, not "the largest of the group".
* A file written by a process that is **not confined** into the host's RAM layer stays charged to the writer's
  group after that group is removed (shown as `reste` in `/proc/ultar/gruppen`); a confined cell cannot do that.
* The quota of the host's RAM layer is `T/4` for all hosts' writers together; no per-directory quota.
* Sockets: the per-group share is fixed at 512; socket buffers follow the group's memory rules but have no own
  file in the tree. `SO_SNDBUF`/`SO_RCVBUF` unchanged (ignored).
* A blocked pipe writer wakes on every pipe/network event and looks again (as readers already did).
* The first 256 MiB of RAM stay unmanaged (window for fixed-address programs) – unchanged.
* The dynamic numbers (shares `T/64`, `T/16`, `T/8`, `G/8`, 512 sockets) are first values, not tuned on real
  workloads; they are constants in `groups/policy.rs` / `mem.rs`.
* Not tested: real hardware (QEMU TCG only, no KVM here); more than 4 processors; memory above 4 GiB beyond one
  boot with 6 GiB; nested cells (group below the starter's group – code path only); `cgroup2` mounted inside a
  cell; threads (`CLONE_VM`) against `pids.max` (only processes); TCP under memory refusal; a group tree with
  all 1024 slots in use; ext4 as writable root together with the quota.
* `selftest::run` on real hardware now runs `grenztest`, which fills memory on purpose (minutes on machines
  with much RAM).
* RULES.md items that do not fit a `no_std` kernel binary, as in stage 1: no `cargo test`/clippy gate (the
  checks are the ring-3 programs in QEMU), no i18n keys (kernel log and test output are German like the rest
  of the kernel), existing German identifiers stay; `syscall.rs`/`proc.rs` were over the size rules and grew.
