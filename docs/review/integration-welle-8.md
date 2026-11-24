# Integration wave 8 – merge record and integration review

The deep security review of all branches is done separately (agents/w8/review/security-runde.md); its blocking fixes fix-1/fix-2 are applied here.

Branch `integ/wave8` in `/home/claude/agents/w8/integ/repo`, base **origin/main 7744e78**.

## 1. History (first parent, oldest first)

| Commit | What |
|---|---|
| e04ed44 merge wip/desktop-shell 27e9e2f | shell round 3; carries launcher 5876846 (+ MAX_GROUPS 32), services b54fda3, Bluetooth v2, wip/network ce2316c. Clean on 7744e78. |
| b23b30b merge wip/phone 4b5aecc | clashes of §2; the phone's launcher copy removed. |
| 16aaddb launcher R2-1 fix + `tests/review_round2.rs` | launcher-runde-2.md |
| d2b1a98 cherry-pick def3ceb | nvidia-display manifest readable, driver tests follow the catalogue (failed on main 7744e78). 3f4cb1e not taken (old settings/installer state). |
| 75d76cf | `/etc/resolv.conf → /run/ultar/net/resolv.conf` in `build-initramfs.sh` (was missing). |
| f8f91f8 | busd service-file test knows calld + ril's root socket; vpnd note. |
| d5d8f56, c33d452 | desktop-e2e.sh / launcher-e2e.sh returned 1 after printing ok (`[ VERBOSE = 1 ] && …` as last command): ci.sh (set -e) stopped, vmware-like.sh said FEHLER. |
| a6c1f93 | compositor `Window` grouped title-bar fields (`TitleBar`): phone's `client_header` made 16 fields (ultar-check limit 15). |
| fcd7c77 merge wip/installer-boot 5f99011 | settings + installer round 2 + installer boot mode; committed test binaries removed (built by make -B now; ended the stale prebuilt rechtetest/fenstertest failures); installer uid 840; ui toolkit merged (card border box, radio cards with the phone's Choices); settingsd seed 116 keys; make-vmware handles `manual = true` and `[profiles]`; phone-bricks references re-recorded (1 px). |
| e8a5890 | desktoptest started no compositor any more: it only dropped `manual` lines, the compositor now says `[profiles] system = "manual"` (DESKTOP failed). |
| 05356aa merge wip/files fa2598b | files' mirrored launcher/volumes contract replaced by `ultar_bus::messages`; its `Thumbnail` → `Preview` (the shell owns `Thumbnail`); sidebar conformance 183 (border box, as the board). |
| 54f06c3 merge wip/kernel-bt 9f7ed27 | merged three-way on base b54fda3: /dev/ultar lists service nodes + part + installer nodes; one powerd busy fix (atomic swap); btd Ultar backend; bt-e2e + BTTEST-UINPUT in ci.sh. |
| b152970 merge wip/web ec75324 | web's `Picture` widget → `PixelArea`; CardTone Region/Chip; `paint_bar(fill: Option)`. |
| b69fbe2 | shell snapshot references (never committed by the shell branch) recorded on the integrated toolkit; phone.rs favourites geometry 16+1+8 as board CompactHome. |
| bf46cc5 | setup-builder.sh links the system lld as rust-lld (kernel gate on fresh builders). |
| 5abe6a3 | installtest lost its static recipe in the kernel-bt merge (dynamic → "ld.so not found": INSTALLER-DISK failed); installer-disk.sh no longer dies on its own diagnostics. |
| a315c24 | `describe_widget` back to 60 lines (ultar-check size). |
| 07689e3 (S-6) | authd: PIN locked after 10 failures until the password is used; unreadable counter file = global wait (tests). |
| 734498e (S-5 part) | compositor: unlocked call screen ranks below system dialogs (test). |
| 1691463, 404857f | reference PNGs: shell's stack-corner (never committed), compositor ui_app_en (toolkit border box). |
| 01648c1, 82966a1 | clippy in the S-1 patch; busd service-file test knows `[profiles]` and the installer socket (840). |
| 2e71bb8 | **launcher timer lost wake-up**: a shutdown between tick and wait slept up to 1 h – `World::close` hung the process tests on B1/B2 (30+ min); now ≤ 1 s idle, 20 ms while stopping. |
| ba4fb81 | ultar-cloud tests: private temp dirs under umask 002 (w9/cloud FIX.patch). |
| 35b47a5 | web headless test probes the surface beside the (moved) title. |
| 60ebc9c, b4a291f (S-1, S-2) | security reviewer's fix-1.patch (lock screen: only the session owner unlocks) and fix-2.patch (netd: no open evil twin of a secured network), applied unchanged. |

Not taken: `claude/wip-integration-2` (pre-review launcher, old shell/settings).

## 2. Clashes found and fixed (blocking)

| # | Clash | Fix |
|---|---|---|
| I-1 | **uid/gid 813** = `authd` (shell) and `calld` (phone): the call service would have run as the authentication service's user (read the password hashes, own `auth.sock` group). | `calld = 829` (ids.toml, call.toml, bus.toml group + broker groups, docs); busd test lists it. |
| I-2 | **Rights bit 64** = `OUTPUT_CONFIG` (display service) and `CALL_SCREEN` (phone): an app given the call screen would also have been allowed to rearrange/identify outputs (and vice versa). | `CALL_SCREEN = 128`, `CLIENT_HEADER = 256`, `ALL = 511`; names (not bits) in rights files, so no file changes. |
| I-3 | **Request opcodes 30/41** = shell `ShellFocus`/`SetPhoneMode` and phone `ClientHeader` (30): two meanings for one wire number. | `ClientHeader = 42` (encode + decode, test, docs). |
| I-4 | Duplicate launcher contract (`ultar_phone::launch` mirror). | removed; phone + `tools/phone-e2e` use `ultar_bus::messages::launcher`; `serde` dropped from ultar-phone. |
| I-5 | busd groups 22 (805, 813–829, 840) – needs the shell's limit raise; calld socket bound 8 → 4. | see §3. |
| I-6 | ui widget names: shell `Thumbnail`/`Picture` vs files `Thumbnail` vs web `Picture`. | files → `Preview`, web → `PixelArea`. |
| I-7 | kernel rights bits 10/11 (`bluetooth`, `input_inject`, kernel-bt). | no clash; launcher and init drop rights by complement, so new bits never reach app cells. |

## 3. Kernel change of the shell branch (reviewed explicitly)

`MAX_GROUPS` 16 → 32 in `kernel/ultar-kern/src/cred.rs` (fixed array in `Cred`, `setgroups` refuses
`size > 32` with `EINVAL` before reading user memory – the read is bounded by `n * 4 ≤ 128` bytes), in
`ultar-sys` (`switch_identity` refuses more, test with 33) and ultar-init (`Bound("groups")`, test follows the
constant). `rechtetest.c` now checks that 33 are refused. Cost: +64 bytes per credential. busd uses 22, logd
17. Bus `pids.max` 384: bounded by `the_sum_of_the_connection_bounds_fits_the_limits` (sum of all socket
bounds ×2 threads + reserve ≤ pids.max, and ≤ 1024 fds/threads). **Accepted: bounded and tested**
(RECHTETEST in ci.sh, see §5).

## 4. Security review: services, network, phone

Blocking: I-1, I-2, I-3 (fixed above). No blocking finding left.

* **Bus policy, system-only methods.** Pseudo methods `<svc>.system` / `.admin` (notify, power, session,
  media, launcher, auth) are checked by the services through `ultar-svckit::Access` against the same policy
  file; no `app.*` rule lists one. `app.*` gets only `update.status/check/download/history/get_policy`,
  `notify.post/dismiss/take_invoked` (notifyd checks the sender for dismiss), `media.update/release`.
  `modem.*` only `system.calld`; `call.*` only `app.phone`. Note: there is no per-app bus identity yet
  (launcher L-12), so the `app.*` rules are not reachable today; when it comes, the identity must be the
  launcher's signer-bound id (store apps cannot take system ids like `settings`/`phone` – launcher L-1).
  `notify.invoked` (all apps) names the sender app of an invoked notification – minor information leak.
* **uids**: all fixed ids unique after I-1 (801–805, 810–829, groups 812, 830–832); ranges people
  1000–19999, apps 20000–49999 hold no fixed id.
* **Secrets/logs**: `Secret` Debug is `<hidden>` and zeroed on drop; netd Wi-Fi keys and VPN tunnels via the
  svckit store (0600, temp + rename) under a 0700 directory; `.conf` commands (`PostUp` …) refused; calld
  history 0600 ≤ 128 KiB; captured writes PNGs 0600 into a 0700 `Pictures`, links not followed; no log record
  with passwords/keys/numbers found (ril prints SIM-role numbers only).
* **Rights (minor, design)**: netd now has `net_admin` and parses EAPOL/DHCP/WireGuard (boringtun + ring
  C/asm) from the network in one process with the `vpn` service – the largest remote attack surface of the
  system services; accountd and captured hold `file_admin` (all file permissions) for home directories;
  ril runs as root with `net_admin`; volumed `mount`. Candidates for splitting later, not blocking.
* **Phone**: “Phone settings” can never work (R2-4 in launcher-runde-2.md: id `org.ultar.settings` is not a
  launcher id and `app.phone` has no `launcher.start`); fails closed with a toast. The launcher cannot grant
  `ui.call-screen` / `ui.client-header` / `ui.output-config` (not in its permission list).
* **boringtun / ring**: x86_64 static (`crt-static`) build of ultar-netd is part of `dienste-e2e.sh` (§5).
  aarch64 **not verified here** (no aarch64 std target and no `aarch64-linux-gnu-gcc` on this machine; ring
  needs a C cross compiler for aarch64) – must be checked on the phone build host like the updater.
* **/etc/resolv.conf** link added to the image (§1). `netd.toml` has `rights = ["net_admin"]`,
  `public_dirs = ["/run/ultar/net"]`.

## 4a. Open points (not blocking, not fixed – single review round)

0. Security review (separate reviewer, docs/review/security-runde-welle-8.md): S-1, S-2, S-3, S-6, S-16 fixed here; S-5 fixed in part (unlocked: below system dialogs; locked: still full screen over the PIN entry); open: S-4, S-5 (locked), S-7, S-8, minors S-9…S-15 – listed in docs/review/OFFENE-BEFUNDE.md.

1. Launcher R2-2…R2-5 (launcher-runde-2.md): 64-bit signer id, group kill without `cgroup.kill` (pid race, per-app not per-role group, 5 s under the core lock), the phone's settings link (`org.ultar.settings` is no launcher id, `app.phone` has no `launcher.start`), one slow start blocks the launcher's bus thread.
2. The launcher cannot grant `ui.call-screen`, `ui.client-header`, `ui.output-config`.
3. `ultar-web` "Show in folder" starts `ultar-files --select <file>`, but ultar-files has no `--select` option yet (it opens URIs via the launcher, `files://…`): the handoff fails as "missing/start error" until Files gets `--select` or web uses `launcher.start` with the file's `files://` URI.
4. boringtun/ring on aarch64 not verified (no aarch64 target or cross compiler on any builder).
5. ultar-check on main 7744e78 already fails with 20 findings (kernel main.rs/selftest.rs hard-coded texts, pci.rs unsafe without SAFETY) – not from this integration; integ adds none.
6. Workspace clippy (`--workspace`, beyond the CI's strict set) fails on lints in untouched legacy crates (ultar-bt, -cloud, -ime, -modem, -net, -office, -secure, -verbund, -zellen); the CI's strict set plus every crate of this integration is clean.
7. ultar-cloud's two file-mode tests fail under umask 002 (directory group-writable) – untouched crate, environment of the test.
8. Phone-bricks snapshot re-recorded (field text 1 px over after the settings card border box) – phone author to confirm against the board.
9. netd holds `net_admin` and parses Wi-Fi/DHCP/WireGuard input in one process; accountd/captured `file_admin` (see §4).
11. busd size: the release build (`opt-level = "s"`, LTO, strip) is 500,840 bytes here, inside the 700,000 budget of tools/budgets.toml; the reported 1.47 MB is the static (`crt-static`) e2e build of the same binary, which `ultar-check budget` does not measure – no budget change.
12. AB-TEST check "installation in the A/B layout" (abtest.py:152): the installer is driven by fixed key presses; the line `[installation] fertig: A/B layout … slot A active` is not seen as expected although the installed disk is correct – check under investigation (kept log on B2 /root/abkeep).
10. Installer e2e stays opt-in (INSTALLER_E2E=1, typed keys lost under QEMU TCG).

## 5. Gate (integ/wave8, builders B1/B2/B4; per-package tests with a 900 s limit)

* fmt clean · clippy `-D warnings` on the CI strict set + every crate of this wave: clean · workspace-wide clippy:
  only lints in untouched legacy crates (4a.6) · ultar-check: the 20 findings main 7744e78 already has, none
  added · design-conformance run.sh: ok · i18n: ok.
* Workspace tests (B2, 2e71bb8): **3058 passed, 4 failed, 21 ignored, no hang**. Fixed after that run: ultar-web
  probe (35b47a5), ultar-cloud (ba4fb81, locally green). Remaining, environment-only: ultar-ui `icon-set-normal`
  (19–21 px, B2 fonts – passes locally/B4, known), ultar-netd `radios_switch…` (B2 has a real radio: Disconnected
  vs NoDevice; passes in a clean container), ultar-service `retained_values…` (timing, passes locally).
* Kernel `tools/kern-test/ci.sh` (B2, rust-1.97 + RUSTC_BOOTSTRAP=1, system lld as rust-lld): intel-display 124,
  every marker ok – BUS SELFCHECK, ZELLTEST, RECHTETEST (32 groups), GERAETETEST (+ many devices, USB),
  WECKERTEST, SPAWNTEST, FENSTERTEST, BTTEST-UINPUT, **VMWARE-LIKE** (3 machines + DESKTOP), DEVD-QEMU,
  ZELLTEST/RECHTETEST-EXT4, EXT4TEST, GRENZTEST 512/2048, INIT, **LAUNCHER**, **DESKTOP**, SVC, **DIENSTE**,
  **TELEFON**, INSTALLER-DISK (installer e2e opt-in, skipped), BT. **AB-TEST: 43 of 44** – "installation in the A/B
  layout" fails reproducibly (twice on B2, seen once by the installer agent); the installation itself is fine
  (factory slot table, slot A start, offline update, power-loss and loader checks all pass) – see 4a.12.
