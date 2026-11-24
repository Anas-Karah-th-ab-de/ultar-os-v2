# Review round 2 – App launcher (wip/app-launcher 5876846 as carried by wip/desktop-shell 27e9e2f, in integ/wave8)

Role: Prüfer (independent), integration engineer. Tree: `/home/claude/agents/w8/integ/repo`, branch
`integ/wave8` (origin/main 7744e78 + desktop-shell 27e9e2f (with network) + phone + installer-boot + files + kernel-bt + web). The launcher
code in 4cfe800 equals 5876846 except `ultar-sys` `MAX_GROUPS` 16 → 32 (reviewed in integration-review.md §3). Round 1: `/home/claude/agents/w5/review/launcher-runde-1.md`.

## 0. What I ran

| Check | Result |
|---|---|
| Round-1 `TESTS.patch` (`review_attacks.rs`, verbatim, as root, umask 002) | 3 of 5 pass (L-1, L-1b, L-4). The two exec/fd tests fail at `install(...).unwrap()` with `Package(Publisher)`: their app `evil` is **store**-signed without a publisher prefix, which L-1's fix now refuses (by design). The branch's copy signs them with the system key; both pass there. |
| Branch tests `review_attacks.rs` (5), `review_fixes.rs` (7) as root | all pass |
| New `tests/review_round2.rs` (5, see §2) | all pass **after** fix R2-1; `a_revoke_whose_state_cannot_be_saved_takes_the_right_off_file` fails on 5876846 (right still on file) |
| clippy `-D warnings` (CI strict set + all new crates), fmt | clean |
| Workspace tests, kernel gate | see `integration-review.md` §4 |

## 1. Round-1 findings

| # | Status | Evidence |
|---|---|---|
| L-1 blocking (id takeover) | **fixed** | `install.rs`: `id_fits_signer` (system ids have no `.`, store ids `<key-file-name>.<app>`, manifest `publisher` must match), `may_replace` (same signer key id, no downgrade) also for uninstalled ids with kept uid/data; signer stored per app (state format 2). Tests: `review_attacks::{a_store_package_cannot_take_over_a_system_apps_id, kept_data_does_not_go_to_another_publisher}`, `review_fixes::{an_app_is_its_id_and_its_signer, store_ids_carry_their_publisher}`, `review_round2::{the_publisher_prefix_cannot_be_dressed_up, a_second_key_of_the_same_publisher_name_is_another_signer}`. |
| L-2 major (/tmp exec) | **fixed on Linux** | `ultar-sys::cell::mount_tmpfs` now `NOSUID|NODEV|NOEXEC` (cell root, `/tmp`, `/dev` tmpfs). `review_attacks::an_app_cannot_execute_what_it_wrote` (system app) and `review_round2::a_store_app_cannot_execute_what_it_wrote` (store app) pass. Open: the Ultar kernel ignores `noexec` (documented in code) – there `/tmp` still runs code. Interpreters (`sh /tmp/x`) are inherent. |
| L-3 major (untracked cells) | **fixed** | stage 1 and stage 2 `PR_SET_PDEATHSIG` (set again after the id switch), handshake loss → `abort()` (SIGUSR1 → stage 1 SIGKILLs pid 1 = whole cell), stop escalation ends to `end_group` (memory group), `Launcher::open` ends `apps/*` leftovers (`cleanup::end_leftovers`). Restarts run on their own thread (stage 1's PDEATHSIG is bound to the starting thread: both starter threads – bus serve loop, restarter – live as long as the launcher). Test `review_fixes::a_restarted_launcher_ends_the_cells_of_its_previous_run`. |
| L-4 major (revoke fail-open) | **fixed, one hole left → R2-1 fixed here** | revoke stops the app first; `write_rights_or_withdraw` removes the file on failure; compositor treats absent/untrusted file as "no rights". Tests `review_attacks::a_revoke_takes_effect…`, `review_fixes::a_revoke_withdraws…`. |
| L-5 major (pid reuse) | **fixed** | only stage 1 (launcher's own child) is signalled; `ended()` clears `helper` before `child.wait()` reaps it. Test `review_fixes::a_reused_process_number_is_never_signalled`. Residual: R2-3. |
| L-6 minor | fixed | runs < `SHORT_RUN` (10 s) count as failures also with code 0; restarts on the restart thread, not the timer. |
| L-7 minor | fixed | `uninstall` waits (bounded) for the instances to end before deleting data; `Busy` otherwise. Test `uninstall_ends_the_app_before_its_data_goes`. |
| L-8 minor | fixed | compositor opens `O_NOFOLLOW|O_NONBLOCK`, checks and reads the same descriptor. |
| L-9 minor | fixed | stage 2 drops the status pipe right after `READY` (stage 1 reports the end). Ultar-kernel /proc behaviour still untested in QEMU, but the descriptor is now gone. |
| L-10 minor | fixed | `MAX_APPS` = 4096 on both sides (list ≈ 0.5 MB < 1 MiB frame). |
| L-11 minor | partly | new `an_app_cannot_signal_another_apps_process_without_a_pid_namespace`; cgroup enforcement still only in QEMU. |
| L-12 | open (acknowledged) | ultar-pkg native payload to `/`, devtmpfs/no netns/no seccomp on the Ultar kernel, no per-app bus identity. |

## 2. New findings (round 2)

### R2-1 major – FIXED in integ/wave8 (16aaddb): revoke whose state save fails leaves the right on file
`launcher.rs` `set_permission`: `store.save().and_then(write_rights_or_withdraw)` – when the save failed
the compositor file was not touched, so a revoked `ui.*` right stayed on file while the in-memory state
said "revoked"; the app got the right again at its next start. Now the file always follows the in-memory
state (written or withdrawn), a failed grant is rolled back in memory and file.
Test: `review_round2::a_revoke_whose_state_cannot_be_saved_takes_the_right_off_file` (failed before:
`20000 = "ui.capture"` still on file).

### R2-2 minor – the signer is a 64-bit truncated key hash
`ultar_pkg::key_id` = first 16 hex digits of SHA-256(pubkey); `may_replace` compares only that. Only
trusted (admin-installed) keys can sign, so an attacker needs a trusted store key whose hash prefix equals
the victim's (2^64 second preimage) – not practical today, but store keys come from publishers. Store the
full 32-byte key (or SHA-256) as the signer in state format 3.

### R2-3 minor – group kill on kernels without `cgroup.kill`, and its scope
`cleanup::end_group`: without `cgroup.kill` (Ultar kernel) it reads `cgroup.procs` and SIGKILLs each number
– the L-5 race again (a member that exits between read and kill frees its number; the launcher has
`kill_any`). Only reached at escalation step 2 (stage 1 did not end within the grace) and at start-up.
Also: the group is per **app** (`apps/<id>`), not per role, so a hard end of a hung Ui also ends the app's
Service; and `end_group` waits up to 5 s inside `tick()` holding the core lock (all launcher calls stall).
Fix later: kernel-side group kill on the Ultar kernel, per-role groups, wait outside the lock.

### R2-4 minor (integration) – the phone's "Phone settings" can never work
`ultar_phone::launch` asks for `org.ultar.settings` with `settings://apps/org.ultar.phone`. That is no
valid `AppId` (three segments; system ids may not contain `.` at all), and `app.phone` has no
`launcher.start` in `bus-policy.toml`. It fails closed (toast `phone.error.launch`). The shell branch uses
the same `org.ultar.*` ids: the id convention (APP-RULES `camera` / `<publisher>.<app>`) vs. the shell's
reverse-DNS ids needs one decision before settings/desktop-shell are integrated. Likewise the launcher's
permission list has no `ui.call-screen` / `ui.client-header` (phone) or `ui.output-config`: an installed
phone app cannot be given the call screen through the launcher.

### R2-5 minor – one slow start blocks the launcher's bus service
`service::serve` answers one request at a time; `start` waits up to `START_DEADLINE` (10 s) for the
handshake, so a hanging cell start delays `stop`/`list`/`revoke` from the shell by up to 10 s.

### Checked without finding
* Publisher prefix: case, Cyrillic look-alikes, zero-width space, `..`, empty parts, 3 segments, `/`,
  prefix-of-prefix (`stor`, `store-x`), key files named `Acme`, `ac_me`, `x--y` – all refused (`AppId` is
  ASCII `[a-z0-9-]`, ≤ 2 segments, prefix compared for equality). A second key file `store.old.ed25519`
  yields the same publisher name but another signer → `Signer`.
* Version order: unparsable or overflowing numbers → `Downgrade` (fail closed); equal versions reinstall.
* URI: C0/C1/DEL/NUL refused, scheme ASCII lower-case only, must be declared, ≤ 2048 bytes, Ui role only,
  passed as `ULTAR_OPEN_URI` (not argv). Bidi/format characters (U+202E) pass – harmless as data, relevant
  only if an app shows the URI.
* v1/v2: `launcher.start` v1 is decoded as `StartV1` and answered with `StartReplyV1`; v2 via
  `respond::<Start>`; other versions refused by the bus layer. The phone now uses the real v2 type.
* State format 2 refuses format-1 files (no silent signer-less entries).

## 3. Verdict

L-1…L-11 fixed (L-2 on Linux; Ultar kernel `noexec` open), one new major hole (R2-1) fixed in the
integration branch (commit 16aaddb) with a test. No blocking finding left. Launcher: **merge-ready**.
