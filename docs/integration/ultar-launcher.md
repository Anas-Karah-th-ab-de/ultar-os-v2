# ultar-launcher – every app as its own user in its own cell (P5 app isolation)

PLAN.md §3.7 (namespaces, groups `system/session/apps/cells`, rights, profiles) and §8 P5 ("apps as own
processes in a cell with rights from the manifest"); APP-RULES §1.2 / §3. Closes compositor findings
**N7/K9** (all apps shared one uid). Patch: `CHANGES.patch` against repo commit `ce287d2` (`patch -p1`).

## 1. What exists

| Where | What |
|---|---|
| `system/core/ultar-launcher` (new; lib + bins `ultar-launcher`, `ulaunch`; test helper `ultar-launcher-testapp`; example `launchtest`) | the launcher. In **system core** (not `system/services`) because it needs the HAL crate `ultar-sys` like `ultar-init` (the layer table allows `ultar-sys` to core only) and reuses `ultar-init`'s output forwarder and group tree |
| `system/hal/ultar-sys` | module `cell`: `unshare` (namespace flags only), private mounts, tmpfs, read-only/no-exec binds, proc, devtmpfs, `pivot_root`, detach, `send_signal`, `reap_any`; the device nodes a cell may see. `rustix` feature `mount` |
| `system/core/ultar-bus` | contract `messages::launcher` (calls below, error codes 1–11) |
| `system/core/ultar-pkg` | `verify_manifest_bytes` (the **signed** manifest bytes); `permissions` may be the `[permissions]` table of the app rules (packing/signing such manifests failed before) |
| `platform/ultar-compositor` | `app_rights::AppRights`: the launcher's file for app uids (`--app-rights FILE --app-uids FIRST-LAST --app-rights-owner UID`), re-read when it changed at every `Hello` |
| `rootfs/etc/ultar/launcher.toml`, `services/launcher.toml` (new) | configuration; service file (user 805, five rights, each justified) |
| `rootfs/etc/ultar/{ids.toml,bus-policy.toml,services/{bus,logd,compositor}.toml}` | user `launcher = 805`, `[ranges] apps = "20000-49999"`; bus/log sockets of the launcher; `system.launcher` registers `launcher`, `system.shell` may start/stop/list/permissions/grant/revoke, `system.admin` `launcher.*` (incl. `launcher.admin`); the compositor reads the app rights file |
| `tools/kern-test/launcher-e2e.sh` (new), `ci.sh` | QEMU run on the Ultar kernel, verdict `LAUNCHER: ALLES OK` |
| tests of others adjusted | `ultar-busd/src/service_files_tests.rs` (sixth socket, id 805), `ultar-logd/tests/service_file.rs` (two more listeners) – only the counts/ids, no rule weakened |

Kernel: **not touched** (image built from the unchanged `kernel/` of `ce287d2`).

## 2. What every app gets

| | How | Where |
|---|---|---|
| own user and group id | from `uid_first..uid_last` (20000–49999), allocated at installation, **kept for ever**: uninstall marks the entry, a reinstall gets the same id, the counter only grows – never given to another app | `store.rs` (state file `apps.state`, atomic, strict parser, fuzzed) |
| own data directory | `<data_root>/<app>`, `0700`, owner = app uid, checked (no link, no foreign owner); removed at uninstall unless `keep_data` | `datadir.rs` |
| own memory group | `<group_root>/apps/<app>`: `memory.max`, `pids.max` from the manifest's `[limits]`, filled and capped by the profile; stage 1 moves itself in before anything else | `plan.rs` (via `ultar_init::groups`) |
| cell | mount + pid namespaces (+ net without the `network` permission, + ipc where the kernel has it); a 1 MiB tmpfs root holding only mount points; `system_view` read-only (nosuid, nodev); data dir at `/data` (rw, **noexec**); private `/tmp` (tmpfs, `0700`, owned by the app, size from the profile); own `/proc`; `/dev`: only `null zero full random urandom` bound (Linux) or the kernel's device tree (Ultar: raw devices need `raw_device`); `shares` read-only (socket dirs, e.g. the compositor); `pivot_root`, old root detached, root remounted read-only | `cell/view.rs` |
| identity and rights | `setgroups([])`, gid, uid (read back); `RLIMIT_NPROC` = `pids`, no core dumps, umask 077, `no_new_privs`; **all** Ultar kernel rights dropped (profile allows none; read back); Linux: uid ≠ 0 clears capabilities | `cell/stage.rs` |
| permissions | exactly what the signed manifest declares ∩ what the profile may grant ∩ what was granted | `perm.rs`, `profile.rs` |

Profiles (decided by **who signed the package**, never by the app): key in `system_keys` → `app.system` (all
permissions grantable, granted at installation; memory ≤ 4 GiB, pids ≤ 1024); key in `store_keys` →
`app.standard` (only `network`, `background`; nothing granted without the user; memory ≤ 1 GiB, pids ≤ 256).
Kernel rights: none for both (manifest `kernel = [...]` is refused at installation). A manifest that declares
more than its profile allows is refused at installation.

Enforced permissions: `network` (cell without own network namespace), `background` (the `[service]` program
starts only with it), `ui.shell | ui.input-method | ui.lock-screen | ui.capture | ui.overlay | ui.system-dialog`
(compositor, §5). `[permissions] bus` and `files` are recorded and reported (`launcher.permissions`), not granted
here – see "Open".

## 3. How a start works

`ultar-launcher` never confines itself; it executes itself as a helper (no `unsafe`, no `pre_exec`):

1. **stage 1** `--cell …` (outside): writes its pid to `apps/<app>/cgroup.procs`, `unshare(NEWNS|NEWPID[|NEWNET][|NEWIPC])`,
   mounts private, starts stage 2, reports `CELL <pid>` on the status pipe, waits, reports the end.
2. **stage 2** `--cell-init …` (pid 1 of the cell): builds the view, switches root, takes the app's ids, drops
   rights, then **stays pid 1** (as the app's user, without rights) and starts the app as pid 2: a pid 1 gets no
   default signal actions – an app must (its own `abort()`, `SIGTERM`). It passes `SIGTERM` on, reaps every
   child, reports `EXIT n` / `SIGNAL n` and ends with the app; the kernel ends the rest of the cell.
3. Every failing step ends the stage with its own code before the app ran (`FAIL n`): 110 arguments, 111 group,
   112 namespaces, 113 mounts, 114 root switch, 115 ids, 116 rights, 117 spawn, 118 setup, 127 exec
   (`Exit::Setup(n)` in the contract).

Status pipe: number in `ULTAR_CELL_STATUS_FD`, lines `CELL`, `READY`, `FAIL`, `EXIT`, `SIGNAL` (first end wins).
App environment (nothing else of the launcher's): `PATH=/usr/bin:/bin`, `HOME=/data`, `TMPDIR=/tmp`,
`ULTAR_APP_ID`, and `ULTAR_ACTIVATION_TOKEN` (window role, first start only).

Platform (detected at start by `--probe`, never assumed; record `launcher.platform`): `ids` (root on Linux,
`set_id` on Ultar), `cell`, `net`, `groups`, file binds, Ultar rights. A missing `ids|cell|groups|net` refuses
the start (`ERROR_PLATFORM`, nothing runs; without ids not even the installation) unless the operator lists it
in `accept_missing` (shipped: empty).

## 4. Bus API (`ultar_bus::messages::launcher`, service `launcher`)

| Call | Who (shipped policy) | Reply |
|---|---|---|
| `list {}` | shell, admin | `apps: [{app, uid, profile, version, instances: [{role, state, pid, restarts, last_exit}]}]` (≤ 256) |
| `start {app, role: Ui\|Service, activation_token}` | shell, admin | `{state, pid, already}` – single instance per app and role; replies after `READY` (≤ 10 s) |
| `stop {app, role}` | shell, admin | `{state}` – `SIGTERM`, after `stop_grace_ms` `SIGKILL` (ends the cell) |
| `permissions {app}` | shell, admin | `{declared: [{name, granted}], bus, files}` |
| `grant / revoke {app, permission}` | shell, admin | `{granted, stopped}` – revoking stops the app's running instances |
| `install {package}` | admin (+ `launcher.admin`) | `{app, uid, profile, declared}` – verifies the `.uap` signature itself |
| `uninstall {app, keep_data}` | admin (+ `launcher.admin`) | `{stopped}` |

States: `Starting Running Stopping Backoff Stopped Crashed Failed`. Ends: `Code(n)`, `Signal(n)`, `Setup(n)`.
Windows are never restarted (a crash is `Crashed`); a `[service]` follows `restart = never|on-failure|always`,
back-off 200 ms·2ⁿ ≤ 30 s, 5 failures within 60 s → `Failed` (until started on request). Errors
`Service(code)`: 1 unknown app, 2 not declared, 3 not grantable, 4 package refused, 5 platform, 6 limit,
7 start failed, 8 shutting down, 9 storage, 10 not granted, 11 invalid.

Permission prompts are system dialogs of the shell (`Role::SystemDialog`): the shell shows the dialog and calls
`grant`/`revoke`. Until the shell exists: `ulaunch [--bus PATH] [--lang ar|en|de] list | start APP [--service]
[--token N] | stop APP [--service] | permissions APP | grant APP PERM | revoke APP PERM | install FILE.uap |
uninstall APP [--keep-data]` (messages ar/en/de; exit 0 / 1 refused / 2 usage).

Logs: the apps' standard output and error go to the log service's output channel
`/run/ultar/log/launcher-out.sock` as `app-<id>` (`.` → `_`; ids ≤ 28 bytes so the name fits), bounded ring
while the log service is away (`ultar_init::output::Forwarder`); the launcher's own records
(`launcher.started|ended|installed|uninstalled|granted|revoked|refused|platform`) go to
`/run/ultar/log/launcher.sock` as `system.launcher`.

## 5. Compositor rights (closes N7/K9) – the choice

**File, not a bus lookup.** The compositor already identifies clients by kernel uid (`SO_PEERCRED`) and keeps
per-uid limits; with one uid per app those limits are now per app (N7/K9 closed by construction). For `ui.*`
rights the launcher writes `compositor_rights` (`/run/ultar/launcher/compositor-apps.toml`, `uid = "names"`,
atomic, `0644`, owner 805) after every install/uninstall/grant/revoke; the compositor (`--app-rights … --app-uids
20000-49999 --app-rights-owner 805`) re-reads it when its stamp (dev, inode, size, mtime) changed, at each
`Hello`. Trusted only if a regular file of that owner, not group/other writable, ≤ 256 KiB, every line valid and
every uid in the range – otherwise no app has any right (fail closed). The static rights file wins for its own
uids. Why not a bus lookup: no round trip and no dependency of the compositor on the bus/launcher at every
connect, no failure mode where the compositor waits on a hung launcher; rights change only at grant/revoke,
and a revoke stops the app (a connected client keeps the rights of its `Hello`).

## 6. Wiring

1. Apply the patch; `system/core/ultar-launcher` is a workspace member (root `Cargo.toml`), `Cargo.lock` updated.
2. Image: install `ultar-launcher`, `ulaunch` to `/usr/bin`; ship `/etc/ultar/launcher.toml`; create
   `/etc/ultar/trusted-keys/system/` with KM Code Craft's key (store keys stay in `/etc/ultar/trusted-keys/`).
3. Installer: after `upkg`/`uapp` put the payload in place, call `launcher.install {package}` (path readable by
   805). Programs named in `exec` must lie in a `system_view` directory.
4. Shell: `launcher.start{app, Ui, activation_token}` with the compositor token of the click; `list` for the
   app list; permission dialogs → `grant`/`revoke`.
5. Running as user 805 needs the Ultar kernel's rights; on Linux (development) run it as root with
   `--allow-root` (launchtest does that) – the rights list is not enforced on Linux.

## 7. Tests and gate

Run on x86_64 Linux 6.18 (`-j1`, debug, `CARGO_INCREMENTAL=0`), three times each:
`cargo test -p ultar-launcher -p ultar-sys -p ultar-bus -p ultar-pkg -p ultar-compositor -p ultar-busd -p ultar-logd
-p ultar-init -p ultar-sprache` as root: **537 passed, 0 failed, 5 ignored** (×3); the `ultar-launcher` and
`ultar-sys` test binaries as uid 1000 with umask 002 (`setpriv --reuid 1000 --regid 1000 --clear-groups`):
**76 passed, 0 failed** (×3; the root-only cell tests say "skipped"). `cargo fmt --all --check` clean;
`cargo clippy --all-targets -D warnings` clean for the nine crates; `ultar-check all`: ok, 5550 baselined,
147 warnings – identical to `ce287d2` (no new baseline entry). One earlier run hit a load flake in
`ultar-bus/tests/gate.rs::silent_connections_…` (counts threads of the whole test process); not reproduced in
6 further runs, not touched.

* unit (`src/**`): app ids, manifest (every rule's breach, sizes, fixed-seed fuzz 6000 inputs), profiles, permissions,
  store (round trip, 17 damaged files, fuzz 2000), uid allocation, data dirs (links, modes), atomic writes,
  config (shipped file fails closed; app range reserved in `ids.toml`, no fixed id in it, compositor uses the same
  range and owner), plan/status-line round trips, restart policy (back-off, crash loop) with injected time,
  rights file, platform decoding, error codes, messages ar/en/de.
* process (`tests/process.rs`, real helper and app processes): single instance, stop → `SIGTERM` reaches the app
  (`Signal(15)`), output to the log channel as `app-good` with token and identity (root: uid 20000, `cwd=/data`,
  read-only root, no rights), crash `Signal(6)`, exit code, service restart → `Failed` after 4 restarts,
  `background` needed, ids stable across reinstall/restart and never reused, data removed/kept, package checks
  (signature, profile, limits, kernel rights, not native, legacy manifest), grants → compositor file, revoke stops
  the app, fail closed (no ids / no group tree / tree without limit files).
* cell (`tests/cell.rs`, root): **malicious app** – other app's data (also via `..`), `kill` of the other app,
  `setuid(0)`, Ultar rights, `unshare`, `mount`, writes to `/usr` and `/`, a root `0600` socket in a shared dir
  and one outside the view, the host's loopback, fork bomb (stops at 14 of 64 with `pids = 16`) – all blocked,
  the victim runs on, its secret unchanged, limits in the group; network only with the permission.
* bus (`tests/bus.rs`): `ulaunch` as administrator and as shell; the shell cannot install/uninstall even with a
  policy line for `launcher.install` (no `launcher.admin`).
* compositor: `app_rights` (owner, mode, range, refused lines, change detection, static table wins), command line.
* Ultar kernel in QEMU (`tools/kern-test/launcher-e2e.sh`, kernel of `ce287d2` unchanged, via the existing
  `ultar-init` with the shipped logd, bus and launcher definitions, `--groups`): **3 of 3 runs `LAUNCHER: ALLES OK`**.
  Launcher runs as user 805 with its five rights; app `good` reports `uid=20000 gid=20000 pid=2 rights=0 cwd=/data
  root_readonly=true token=5`; the malicious app's 12 attacks blocked (forks stopped at 13 with `pids.max=16`,
  memory hog ended by the kernel: `[mem] oom-end … group=/apps/evil reason=group-limit`); `apps/evil` has
  `memory.max=67108864 pids.max=16`; stop → `Signal(15)`; reinstall gets uid 20000 back, a new app 20003; every
  service exits 0 at shutdown. `ci.sh` runs it after `init-e2e.sh` (the whole `ci.sh` was not run: hours).

## 8. Not done / open (honest list)

* **Bus identity per app**: apps cannot reach the bus yet (no socket in their cell); the manifest's `bus` list is
  recorded, not enforced. Needed: a credential listener (`--listen-cred … --uid-map`) fed from the launcher's ids,
  per-app policy rules generated from `bus` ∩ grants, and the socket dir in `shares`.
* **Named file areas** (`media:*`, `documents`, `share`): recorded only; no file service, nothing mounted.
* **Ultar kernel gaps** (stop the start unless accepted): no network namespace (an app reaches the network and
  the host's loopback – `launchtest` accepts `net` explicitly there), no single-file bind (whole `devtmpfs`;
  `/dev/console` and `/dev/tty` are `0666` there – kernel default to tighten), no CPU limit, no
  `SCM_CREDENTIALS`, no seccomp-like call filter on either kernel.
* Linux without a cgroup2 memory controller (this machine): memory limit not testable here (refused unless
  `groups` accepted); `RLIMIT_NPROC` bounds processes per app uid there. The memory attack runs in QEMU only.
* Stop signals go to the cell's pid 1 by number: a pid reused in the instant between the app's end and the
  launcher processing it could receive a `SIGTERM` (no pidfd on the Ultar kernel).
* Bus requests are answered one at a time (a start waits ≤ 10 s for its cell).
* Only `kind = "native"`; compat kinds (deb/exe/apk) keep their own runners (`cell.*` profiles not done).
* No per-app CPU weight, IO, GPU limits; no freezing of background apps; `[limits] cpu` is accepted, not applied.
* The installer (`ultar-pkg`/`ultar-apps`) does not call `launcher.install` yet, and `ultar-pkg` still extracts
  native payloads to `/` (legacy) – a store package can write anywhere at installation.
* The shell, lock screen, keyboard as own users with lines in the static rights file: not part of this work.
* CLI texts are tables in `src/msg.rs` (like `uinit`), not `locales/*.ftl`.
