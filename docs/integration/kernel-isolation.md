# Ultar kernel – isolation stage 1: integration notes

Mount namespaces + pid namespaces + foundation of per-process rights (PLAN.md §3.7, phase P2c without the
group tree), with escape tests. Design and escape argument: `DESIGN-isolation.md`. Patch against the repo:
`CHANGES.patch` (`patch -p1` in the repo root; checked: applies cleanly to the originals and reproduces `work/`).

## What changed

| File | Change |
|---|---|
| `kernel/ultar-kern/src/isolation/` (new, 8 files, 1283 lines, English, no `unsafe`, no `unwrap`) | `rights.rs` bit set · `pid.rs` pid namespaces · `signals.rs` who may signal whom · `mount.rs` mount table · `tmpfs.rs` private RAM trees + symbolic links · `path.rs` path walk inside a view · `calls.rs` the new system calls · `mod.rs` `Isolation`, `ViewTag`, `NsRef` |
| `src/proc.rs` | `Group.iso`, `Group.exe_host`, `Proc.link`; `FdKind::Dir` remembers view path + view tag; `FdKind::Ns`; `load_elf` gets a reader for the dynamic loader; end of a namespace's pid 1 kills the namespace; `infos()` = reader's pid view |
| `src/syscall.rs` | `resolve` → `locate` (every path call), pid translation, right checks, new dispatch entries, `sys_kill`/`sys_tgkill` via `isolation::signals`, `execve` through the view |
| `src/fs.rs`, `src/ext4.rs` | private prefix routed to `isolation::tmpfs`; `disk_link` / `Ext4::link_at` (read a link without following) |
| `src/procfs.rs` | reader's pid namespace, `/proc/<pid>/ns/{mnt,pid}`, `/proc/mounts` of the reader's namespace, `/proc/ultar` only for the initial pid namespace |
| `src/signal.rs` | unused `send` removed (its two callers now go through the permission check) |
| `src/selftest.rs`, `user/Makefile`, `scripts/build-kern.sh` | `zelltest` built, copied to the image, part of the hardware self test |
| `user/zelltest.c`, `zelltest-zelle.c`, `zelltest.h` (new) | the escape tests (129 checks) |
| `tools/kern-test/ci.sh` | adds `/bin/zelltest` + `ZELLTEST: ALLES OK`, the ext4 link test, `BUSCK=` for a prebuilt bus selfcheck |
| `tools/kern-test/zell-ext4.sh` (new) | boots with an ext4 disk and runs `zelltest --ext4` |
| `tools/kern-test/qtest.py` | **harness fix, see "Gate"**: incremental UTF-8 decoding of the serial log |

## System calls added / changed

Added: `unshare` (272), `setns` (308), `pivot_root` (155), `chroot` (161), `symlink` (88), `symlinkat` (266)
– the last two only on tmpfs mounts, elsewhere `EPERM` as before.

Changed:
* `clone` (56): `CLONE_NEWNS`, `CLONE_NEWPID`; other `CLONE_NEW*` now `EINVAL` (were silently ignored).
* `mount` (165), `umount2` (166): real inside a non-initial mount namespace (tmpfs, bind, proc, devtmpfs,
  read-only, remount, `MNT_DETACH`); **in the initial namespace still "return 0, no effect"** as before, plus
  the `MOUNT` right check.
* `prctl` (157): `PR_CAPBSET_READ`/`PR_CAPBSET_DROP` for the modelled capabilities, `PR_ULTAR_RIGHTS_GET`
  (0x554c5200), `PR_ULTAR_RIGHTS_DROP` (0x554c5201); everything else returns 0 as before.
* Path calls now resolve through the caller's view: `open`, `openat`, `stat`, `lstat` (now really no-follow,
  only matters where links exist), `newfstatat`, `access`, `faccessat`, `mkdir(at)`, `unlink(at)`, `rmdir`,
  `rename(at)`, `readlink(at)`, `chdir`, `execve` (program, `#!` interpreter, ld.so), `bind`/`connect` of
  Unix sockets. `open` honours `O_NOFOLLOW`. `rename` into `/proc` or `/dev` is `EPERM`.
* Pid calls translate numbers: `getpid`, `gettid`, `getppid`, `getpgrp`/`getpgid`, `setsid`/`getsid`,
  `set_tid_address`, `fork`/`vfork`/`clone` results and `*_SETTID`, `wait4`, `kill`, `tkill`, `tgkill`.
  `tgkill(.., 0)` now returns 0 for an existing thread (was `ESRCH`).
* Right gates: `reboot` (169), `init_module`/`delete_module`/`finit_module` (175/176/313),
  `settimeofday`/`clock_settime`/`adjtimex`/`clock_adjtime` (164/227/159/305) → `EPERM` without the right;
  with it unchanged (`reboot` works, the others are still `ENOSYS`). Opening `/dev/fb0`, `/dev/input/*` needs
  `RAW_DEVICE`, writing `/proc/ultar/firewall|verbund` needs `NET_ADMIN` (`EACCES`).

Defaults: every program started by the kernel has all rights, the initial namespaces and root `/`; for such a
process path resolution is the same lexical normalisation as before (no extra lookups).

## Gate (run on the final tree)

Build: `IMG=1 bash scripts/build-kern.sh` → 41 compiler warnings, the same 41 as the untouched tree (compared
line by line; one existing "never used" warning got shorter because `Shared::same` is used now).

`tools/kern-test/ci.sh` three times with `SMP=2`, once more with `SMP=4` (logs in `logs/`):

```
== gate run 1 (SMP=2)
ok    alle Geräte haben einen Treiber
ok    Ergebnis: ALLES OK
ok    BUS SELFCHECK OK
ok    ZELLTEST: ALLES OK
[kern] bereit nach 145 ms
ok    ZELLTEST-EXT4: ALLES OK
== gate run 2 (SMP=2)
ok    alle Geräte haben einen Treiber
ok    Ergebnis: ALLES OK
ok    BUS SELFCHECK OK
ok    ZELLTEST: ALLES OK
[kern] bereit nach 148 ms
ok    ZELLTEST-EXT4: ALLES OK
== gate run 3 (SMP=2)
ok    alle Geräte haben einen Treiber
ok    Ergebnis: ALLES OK
ok    BUS SELFCHECK OK
ok    ZELLTEST: ALLES OK
[kern] bereit nach 146 ms
ok    ZELLTEST-EXT4: ALLES OK
== extra run (SMP=4)
ok    alle Geräte haben einen Treiber
ok    Ergebnis: ALLES OK
ok    BUS SELFCHECK OK
ok    ZELLTEST: ALLES OK
[kern] bereit nach 161 ms
ok    ZELLTEST-EXT4: ALLES OK
```
Each run: 129 `ok` lines, 0 `FEHLER`, no panic. Additionally the whole `zelltest` and `kerntest` with an ext4
disk as `/` and the FAT disk as `/boot/efi` (`logs/run-ext4root.log`): `ZELLTEST: ALLES OK`, `Ergebnis: ALLES OK`.

**One failed run before these, and what it was:** in my first series of three, run 1 reported
`FAIL alle Geräte haben einen Treiber` although the kernel had printed that line. Cause: `qtest.py` decoded each
chunk read from QEMU's serial port separately, so a UTF-8 character split across two reads became `��` in the
log and `grep` no longer matched "Geräte" (in that run the whole boot text arrived byte-wise). This is in the
untouched harness too. I changed `qtest.py` to decode incrementally; the expected texts in `ci.sh` are
unchanged, no check was weakened. After that fix and two small kernel changes from my own review (`rename` onto a symbolic link replaces the
link instead of writing through it, with a new check; `/proc` name lookups skip the process scan for non-pid
names) the series above
was run from scratch.

Tail of gate run 3 (`logs/gate-3.log`):
```
ok     Wirt: sieht danach den Baum der Zelle
ok     Wirt: setns mit falscher Art -> Invalid argument
ok     Wirt: private Bäume der Zelle erscheinen nicht in /
ok     Rechte: Zelle hält anfangs alle
ok     Rechte: Linux-Weg PR_CAPBSET_DROP(CAP_SYS_ADMIN)
ok     Rechte: alle abgeben
ok     Rechte: mount ohne Recht -> Operation not permitted
ok     Rechte: umount2 ohne Recht -> Operation not permitted
ok     Rechte: unshare(CLONE_NEWNS) ohne Recht -> Operation not permitted
ok     Rechte: clone(CLONE_NEWPID) ohne Recht -> Operation not permitted
ok     Rechte: chroot ohne Recht -> Operation not permitted
ok     Rechte: pivot_root ohne Recht -> Operation not permitted
ok     Rechte: setns in den geerbten Wirts-Namensraum -> Operation not permitted
ok     Rechte: reboot ohne Recht -> Operation not permitted
ok     Rechte: Uhr stellen ohne Recht -> Operation not permitted
ok     Rechte: Treiber laden ohne Recht -> Operation not permitted
ok     Rechte: Rohgerät /dev/fb0 ohne Recht -> Permission denied
ok     Zelle: /dev/null bleibt nutzbar
ok     Rechte: nichts zurückgewonnen (prctl-Versuche)
ok     Rechte: mount weiterhin verweigert -> Operation not permitted
ok     Rechte: Kind erbt die leere Menge
ok     Rechte: Kind darf nicht einhängen -> Operation not permitted
ok     Zelle: pid 1 hält seine Rechte (Abgabe wirkt nur im Kind)
ok     Wirt: Zelle beendet, alle Prüfungen in der Zelle bestanden
ok     Wirt: Ende von pid 1 beendet die übrigen Prozesse der Zelle
ok     Wirt: sechs Zellen gleichzeitig, jede sieht nur sich
ok     Wirt: /bin/kerntest und /liesmich.txt unverändert
ok     Wirt: eigene Datei in /tmp unverändert
ok     Wirt: Dateien der Zelle sind im Wirt nicht entstanden
ok     Wirt: Zuschauer-Prozess lebt (kill aus der Zelle traf ihn nicht)
ok     Wirt: Zuschauer beenden
ok     Wirt: Rechte und Namensraum des Wirts unverändert
ZELLTEST: ALLES OK
ultar-kern:/> 
```
Tail of gate run 1:
```
ok     Rechte: Kind erbt die leere Menge
ok     Rechte: Kind darf nicht einhängen -> Operation not permitted
ok     Zelle: pid 1 hält seine Rechte (Abgabe wirkt nur im Kind)
ok     Wirt: Zelle beendet, alle Prüfungen in der Zelle bestanden
ok     Wirt: Ende von pid 1 beendet die übrigen Prozesse der Zelle
ok     Wirt: sechs Zellen gleichzeitig, jede sieht nur sich
ok     Wirt: /bin/kerntest und /liesmich.txt unverändert
ok     Wirt: eigene Datei in /tmp unverändert
ok     Wirt: Dateien der Zelle sind im Wirt nicht entstanden
ok     Wirt: Zuschauer-Prozess lebt (kill aus der Zelle traf ihn nicht)
ok     Wirt: Zuschauer beenden
ok     Wirt: Rechte und Namensraum des Wirts unverändert
ZELLTEST: ALLES OK
ultar-kern:/> 
```
Tail of gate run 2:
```
ok     Rechte: Kind erbt die leere Menge
ok     Rechte: Kind darf nicht einhängen -> Operation not permitted
ok     Zelle: pid 1 hält seine Rechte (Abgabe wirkt nur im Kind)
ok     Wirt: Zelle beendet, alle Prüfungen in der Zelle bestanden
ok     Wirt: Ende von pid 1 beendet die übrigen Prozesse der Zelle
ok     Wirt: sechs Zellen gleichzeitig, jede sieht nur sich
ok     Wirt: /bin/kerntest und /liesmich.txt unverändert
ok     Wirt: eigene Datei in /tmp unverändert
ok     Wirt: Dateien der Zelle sind im Wirt nicht entstanden
ok     Wirt: Zuschauer-Prozess lebt (kill aus der Zelle traf ihn nicht)
ok     Wirt: Zuschauer beenden
ok     Wirt: Rechte und Namensraum des Wirts unverändert
ZELLTEST: ALLES OK
ultar-kern:/> 
```
ext4 link test (`VERBOSE=1 tools/kern-test/zell-ext4.sh`):
```
ZELLTEST: ext4-Verknüpfungen
ok     ext4: Wirt liest /etc/geheim über die Verknüpfung (Verhalten wie bisher)
ok     ext4: chroot /zelle
ok     ext4: Verknüpfung innerhalb der Wurzel wird gefolgt
ok     Ausbruch ext4: absolute Verknüpfung nach /etc/geheim -> No such file or directory
ok     Ausbruch ext4: relative Verknüpfung mit .. -> No such file or directory
ok     Ausbruch ext4: Verzeichnis-Verknüpfung nach /etc -> No such file or directory
ok     Ausbruch ext4: stat durch die Verknüpfung -> No such file or directory
ok     ext4: lstat und readlink zeigen die Verknüpfung selbst
ZELLTEST-EXT4: ALLES OK
```

## What the escape tests cover

1. Files outside the root: `..` chains (absolute, relative, from a sub-directory) → `ENOENT`; absolute and
   relative symbolic links, a link loop → `ENOENT` / `ELOOP`; a directory fd inherited from the host, one opened
   by the cell before `pivot_root`, one inherited into a chroot → `EXDEV` for `openat`, `fstatat`, `mkdirat`,
   `unlinkat`; `fchdir` fails; ext4 links (absolute, relative, directory link) inside a chroot → `ENOENT`.
2. Processes outside: `kill(2..300, 0)`, `kill(-1)`, `tkill`, `tgkill`, `waitpid` on host numbers → `ESRCH` /
   `ECHILD`; `/proc` lists exactly pid 1; the host bystander is alive afterwards.
3. Mounting over the host tree: a tmpfs over `/bin` in the cell – the host checks at that moment that its
   `/bin` is intact; after the cell: no file, link or directory of the cell exists in the host tree and the
   private trees never appear in the host's `/` listing.
4. Rights: after dropping, `mount`, `umount2`, `unshare`, `clone(CLONE_NEWPID)`, `chroot`, `pivot_root`,
   `setns` (with an inherited handle of the host's namespace), `reboot`, set clock, load driver → `EPERM`,
   `/dev/fb0` → `EACCES`; `prctl` tricks, `fork` and `execve` do not bring a right back; without `KILL_ANY`
   a sibling and pid 1 cannot be signalled; without `NET_ADMIN` the firewall cannot be opened for writing.
5. `/proc` of the parent: entries and namespace handles of host pids → `ENOENT`; kernel log and firewall
   (`/proc/ultar/*`) → `ENOENT`.
Positive: files, links, rename, directories, read-only bind (`EROFS`), `openat` with an own directory fd,
fork/exec/kill/wait inside the cell (pid 2, parent 1), pid 1 shielded from `SIGKILL` from inside, end of pid 1
ends the rest of the cell, the host sees real pids and can `setns` into the cell, `unshare(CLONE_NEWPID)`,
six cells at once (20 rounds each of files, links and forks) see only themselves.

## How to wire it

* Apply `CHANGES.patch`. `work/` needs `assets/fonts` to build (the kernel includes the fonts from
  `../../../assets`); I used a symlink to the repo's `assets` while building and removed it from the delivery.
* `ci.sh` in the repo builds the bus selfcheck from the workspace as before; `BUSCK=` is only for running
  outside the workspace (I built it read-only from the repo into my own target directory).
* Launcher recipe for a cell: `DESIGN-isolation.md` §5.

## Not done / not tested (honest list)

* Out of scope by design (stage 2+): uts/ipc/user/dev namespaces, system-call filter, uid/gid, profiles.
  (Network namespaces and the "internet only" barrier came later – `zell-netz.md`.)
  (The group tree with memory and process limits came with stage 2 – `kernel-memory-groups.md`; a cell can
  no longer exhaust RAM. CPU limits are still missing.)
  A cell can still reach host services on 127.0.0.1 unless it is in a network namespace of its own
  (`zell-netz.md`).
* `mount`/`umount2` in the **initial** namespace are still the old no-op that returns 0 (kept on purpose so
  existing init programs and the kernel's own host-path users do not change; see design §2.2).
* No re-parenting of orphans to a namespace's pid 1 (the kernel has no re-parenting; orphans are reaped by
  the kernel as before).
* Bind mounts: not recursive; single files onto files since `docs/integration/zell-chown.md`; `MS_MOVE`, shared propagation → `EINVAL`; no device/disk
  file systems inside a namespace (`ENODEV`). A process that keeps `MOUNT` can remount a read-only bind
  read-write – a cell must drop `MOUNT` (the tests do).
* Symbolic links can only be created on tmpfs; hard links still `EPERM`.
* `/proc` is bound to the reader's pid namespace, not to the mounter's (design §2.3).
* `LOAD_DRIVER` and `SET_TIME` guard calls that do not exist yet (gate only, tested for `EPERM` vs `ENOSYS`).
* Not tested: nested pid namespaces deeper than one level and the 8-level limit; the 256-mount limit;
  `setns(CLONE_NEWPID)`; `kill` of a cell's pid 1 from the host while cell threads run on another core
  (the code path is the same as the tested normal exit, but there is no dedicated test); ext4 as a *writable*
  root (`ultar-compat`) with cells – tested was ext4 read-only root + FAT; real hardware (QEMU TCG only, no KVM here).
* Cost: in a confined view every path component is checked for a link (on ext4: one directory lookup per
  component, no cache yet). Unconfined processes are not affected. Pid lookup in a namespace is linear in the
  number of threads of that namespace.
* Existing code keeps its German identifiers; `sys_fork`/`sys_open` (already over the 60-line rule) grew.
* `RULES.md` items that do not fit a `no_std` kernel binary and were not done: `cargo test`/clippy gate, unit
  tests in the crate (the kernel has no host test harness; the checks are the ring-3 programs in QEMU),
  i18n keys (test output is German like the other test programs).
