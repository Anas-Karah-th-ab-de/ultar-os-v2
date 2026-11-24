# Review round 1 – App launcher (CHANGES.patch, part B)

Role: Prüfer Sicherheit + Kern (independent). Base `ce287d2`, patch `/home/claude/agents/w5/launch/CHANGES.patch`
applied with `patch -p1` to a fresh clone (`/home/claude/agents/w5/review/launch`). Scope: `system/core/ultar-launcher/**`,
`system/hal/ultar-sys/src/{cell,raw}.rs`, `ultar-bus` contract, `ultar-pkg` change, compositor `--app-rights`,
`rootfs/etc/ultar/**`, `tools/kern-test/launcher-e2e.sh`, `docs/integration/ultar-launcher.md`.

## 0. What I ran

| Check | Result |
|---|---|
| `cargo test` of the nine crates as **root** ×3 | **537 passed, 0 failed, 5 ignored** (×3) |
| `ultar-launcher` + `ultar-sys` test binaries as **uid 1000, umask 002** ×3 | **76 passed, 0 failed** (×3) (+ reviewer tests, see below) |
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets -D warnings` (nine crates) | clean |
| `ultar-check all` | ok, 5550 baselined, 147 warnings (= base) |
| QEMU `launcher-e2e.sh` (kernel of `ce287d2`, built here) | see §3 |
| Reviewer attack tests (`TESTS.patch`: `tests/review_attacks.rs`, 5 tests) | root: **4 fail = findings L-1, L-1b, L-2, L-4**, 1 passes (fd/pid-1 check); uid 1000: L-1, L-1b, L-4 fail, the two root-only tests skip |

The authors' own `gate.log` shows two flakes they ran past: `ultar-bus/tests/gate.rs:33` (round 1) and
`ultar-launcher/tests/process.rs:37 start_is_single_instance_and_stop_ends_the_cell` (round 3); the doc mentions only
the first. Neither reproduced in my 3 root rounds.

## 1. Findings

Severity: **blocking** (must be fixed before merge), **major**, **minor**.

### L-1 blocking – Any signed package can take over another app's user id and data by naming its id
`src/launcher.rs:181-236` (`install`), `src/store.rs:238-251` (`uid_for`), `src/datadir.rs:77-99`.
`install` verifies *a* signature (system **or** store key), then `uid_for(&manifest.id)` returns the **existing** uid of
that id, `ensure_app_dir` hands it the existing data directory, the manifest is replaced and the profile becomes the
new signer's. Nothing compares the new package with the installed one: not the signing key, not the profile, not the
`publisher`, not the version. A store-signed package with `id = "camera"` (or the id of another publisher's app)
becomes that app: same uid (→ same compositor per-uid limits/rights file line), same `/data`, kept grants that are still
declared. Same through `uninstall --keep-data`: the next package with that id, from anyone, gets the uid and the kept data.
APP-RULES §2/§3 require `<publisher>.<app>` ids for third-party apps and increasing store versions; neither is enforced
(`manifest.rs` parses `publisher` but nothing uses it; a downgrade is accepted).
Repro: `review_attacks::a_store_package_cannot_take_over_a_system_apps_id` →
`Ok(InstallReply { app: "good", uid: 20000, profile: "app.standard", .. })` after the system app `good` had uid 20000;
`review_attacks::kept_data_does_not_go_to_another_publisher` → store package gets uid 20000 and the kept secret.
Fix: record the signer key (fingerprint) per app id at first install and refuse updates signed by another key (or at
least another profile); bind store ids to a publisher prefix derived from the key; refuse version downgrades.

### L-2 major – The cell's `/tmp` is executable; `/data noexec` protects nothing
`src/cell/view.rs:45-51, 86`, `system/hal/ultar-sys/src/cell.rs:84-94` (`mount_tmpfs`: `NOSUID|NODEV` only).
The doc and `BindMode::WritableNoExec` say the app's writable storage must not run code ("nothing may be executed from
it"), yet the private `/tmp` (rw, owned by the app) allows exec, so an app downloads/writes a binary to `/tmp` and runs it.
Repro: `review_attacks::an_app_cannot_execute_what_it_wrote` (root): `/data/t: Permission denied` but `TMP-EXEC-OK`.
Fix: mount the cell `/tmp` with `NOEXEC` (and the cell root tmpfs as well, it is read-only afterwards anyway).
If running self-written code is *intended*, drop the `noexec` claim instead – but decide it explicitly.

### L-3 major – Lost handshake and launcher restart leave untracked cells running
`src/spawn.rs:146` (`self.child.kill()` on timeout/lost), `src/cell/stage.rs:61-79`, `src/instances.rs:205-217`.
`child` is **stage 1**; the cell (stage 2 = pid 1 of the new pid namespace, and the app) is stage 1's child and is not
killed: no `PR_SET_PDEATHSIG`, no kill of the pid-namespace init, no `cgroup.kill`. After a 10 s handshake timeout the
instance is marked ended while the app keeps running orphaned; the next `start` creates a **second** instance
(single-instance broken), `stop` cannot reach the first, its group stays charged. The same happens when the launcher
itself dies (`restart = "always"` in its service file): on restart `instances` is empty, every running cell is
untracked, and starts duplicate them. Fix: stage 1 sets `PDEATHSIG=SIGKILL` on stage 2 (it is pid 1 of the namespace,
so the whole cell ends), the launcher kills the pid-ns init (or uses the app group's `cgroup.kill` / `cgroup.procs`)
on timeout, and at start-up empties `apps/*` groups left from a previous run.

### L-4 major – Revoke is not fail-closed
`src/launcher.rs:277-286`. `set_permission(revoke)` persists the new state, then `write_rights()?` returns early on
error – **before** `stop_all_locked`. Result: the request fails, the state says "not granted", the app keeps running
and the compositor's file still lists the right (the compositor keeps using the old, trusted file). Same ordering in
`uninstall` (`launcher.rs:318-319`: an uninstalled uid keeps its `ui.*` line until the next successful write).
Repro: `review_attacks::a_revoke_takes_effect_even_if_the_rights_file_cannot_be_written` →
`revoke failed (Err(Storage(Io(IsADirectory)))) and the app still runs (Some(Running))`.
Fix: stop the instances first / regardless, and on a write failure remove the rights file (compositor: absent = no
app rights) instead of leaving the old one.

### L-5 major – Signals by stored pid can hit a reused pid (SIGKILL included)
`src/instances.rs:63-77, 251`, `src/timer.rs:55-58`, `src/spawn.rs:127`.
The stored pid is **stage 2** (the launcher's grandchild), which stage 1 reaps. Between that reap (pid free) and the
watcher's `ended()` clearing `pid`, `stop`, `uninstall` (hard), `revoke`, the grace deadline and `running()` send
SIGTERM **or SIGKILL** to the number. The launcher holds `kill_any` on the Ultar kernel and is root on Linux, so a
reused pid can be any process. The doc's "open" list mentions only SIGTERM by pid number. Fix: signal stage 1 – the
launcher's own child, whose pid cannot be reused before the watcher reaps it – and let stage 1 forward (or kill its
namespace), or use pidfds where available.

### L-6 minor – No start-rate limit for `restart = "always"`; restarts block the timer
`src/policy.rs:63-98`: an exit with code 0 is not a failure, `in_a_row` resets, so a service that exits 0 at once is
restarted every 200 ms for ever (a new cell, namespaces, mounts and log lines 5×/s) – the crash-loop rule never
triggers. `src/timer.rs:74-76`: the timer thread runs `start_key` (up to the 10 s handshake) inline, so SIGKILL
deadlines of other instances wait behind a slow restart. Needs the `background` grant (user-given).

### L-7 minor – Uninstall deletes data while the cell may still run
`src/launcher.rs:307-312`: `stop_all_locked(.., hard)` only *sends* SIGKILL; `remove_app_dir` runs immediately. The
dying app can still create files (→ `remove_dir_all` fails, "a failure leaves the app installed" – but its instance is
already killed) or race the deletion. Wait for the instances to reach `Stopped/Crashed` (bounded) before deleting.

### L-8 minor – Compositor reads the rights file by path after `lstat` (TOCTOU)
`platform/ultar-compositor/src/app_rights.rs:80-104`: `symlink_metadata` checks type/owner/mode, then
`read_to_string(&self.path)` opens by path (follows a symlink swapped in between); the stamp belongs to the checked
inode, the content may come from another. Only uid 805 can write `/run/ultar/launcher`, so this is defence in depth:
`open(O_NOFOLLOW)` + `fstat` on the same descriptor.

### L-9 minor – pid 1 of the cell runs as the app's user and keeps the status pipe
`src/cell/stage.rs:116-147, 162-196`. After `switch_identity`, stage 2 (same uid as the app) holds the write end of the
launcher's status pipe for the app's whole life. On Linux the app cannot reach it because the setuid made pid 1
non-dumpable – verified: `ls /proc/1/fd` → `Permission denied`, write to `/proc/1/fd/3` refused, `CapEff 0`
(`review_attacks::no_launcher_descriptor_reaches_the_app`, passes; the app sees only fds 0,1,2). On the **Ultar kernel**
this depends on its `/proc` and ptrace rules and is not tested in the QEMU run. If reachable, an app forges
`EXIT 0`/`SIGNAL` lines (evades crash-loop detection, changes restart decisions). Pid 1 does not need the pipe after
`READY`: close it there and let stage 1 report the end (it already does).

### L-10 minor – `list` silently truncates
`src/instances.rs:335` `.take(MAX_APPS)` (256) while the store keeps up to 4096 apps (`store.rs:30`): installed apps
beyond 256 are invisible to the shell, no "truncated" flag.

### L-11 minor – Test quality
* `kill_other_app` (`tests/helpers/attack.rs:38-43`, `tests/cell.rs:46-47`) passes the victim's **host** pid into the
  attacker's own pid namespace, where that number does not exist: it proves pid-namespace separation, not that a
  foreign uid cannot be signalled (e.g. Ui and Service of different apps without pid ns under `accept_missing`).
* Root cell tests use a fake group tree of plain files (`tests/common/mod.rs:128-140`): writing `cgroup.procs` and the
  limits always "works"; memory/pids enforcement is only covered by the QEMU run.
* No test for: handshake timeout cleanup (L-3), launcher restart with running cells, pid reuse (L-5), signer change
  (L-1), `/tmp` exec (L-2), rights-file write failure (L-4), uninstall while the app writes (L-7).
* Positive: uid allocation is under one lock, `next` only grows and the state parser refuses duplicate/out-of-range
  uids and `uid >= next`; app ids are `[a-z0-9-]` (no `/`, `.`-segments only as publisher separator) → no path
  traversal in data/cell/manifest/group paths; data dirs are created under 0700 launcher dirs and checked with
  `lstat` (no symlink following on the final component; `remove_dir_all` does not follow links); the cell gets no
  launcher fd (`take_inherited` + `FD_CLOEXEC`), binds are non-recursive and read-only/nosuid/nodev, old root detached,
  root remounted RO; `unshare` wrapper refuses `CLONE_NEWUSER/FILES/FS`; install/uninstall need `launcher.admin` in
  addition to the policy line.

### L-12 minor – Acknowledged gaps that matter for the security claim (doc §8)
Not new, listed so they are not lost: `ultar-pkg` still extracts native payloads to `/` (a store package can write
anywhere at installation – together with L-1 the isolation claim does not hold for store apps yet); Ultar kernel: whole
`devtmpfs` in the cell (`/dev/console`, `/dev/tty` 0666), no network namespace (apps reach the host loopback), no
seccomp. Bus identity per app not implemented.

## 2. Root requirement / rights

On Linux the launcher needs root (`--allow-root`); on the Ultar kernel it runs as 805 with
`mount set_id file_admin manage_groups kill_any`. `file_admin` + `kill_any` make L-5 and the `install {package}` path
argument (reads any file the admin names; only signed manifests are accepted, so no disclosure) worth keeping small.

## 3. QEMU (`tools/kern-test/launcher-e2e.sh`)

Kernel image built here from the patched tree (`kernel/` unchanged from `ce287d2`), static release binaries built by
the script, `SMP=2 MEM=1024`: **3 of 3 runs `LAUNCHER: ALLES OK`** (identical lines: `good` uid 20000, pid 2, rights 0,
root read-only; 10 attacks + `fill_memory` blocked, forks stop at 13 with `pids.max=16`, memory hog killed by the
group limit, reinstall keeps uid 20000, new app 20003). The `network` attack is not run there (accepted missing).
The QEMU run does not cover L-1…L-5 or L-9 (status pipe vs. pid 1 on the Ultar kernel). Logs:
`/home/claude/agents/w5/review/logs/launch-e2e.out` (the per-run QEMU logs lived in the script's temp dir).

## 4. Verdict

**Blocking: L-1** (identity/data takeover by id through any signed package). **Major: L-2, L-3, L-4, L-5.**
