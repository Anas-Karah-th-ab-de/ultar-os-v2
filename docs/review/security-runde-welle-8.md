# Security review round (Prüfer Sicherheit, w8) – 2026-11-24

Read-only review of the diffs (repo `/home/claude/ultar-os-v2`, after `git fetch`):

| Branch | Head | Base |
|---|---|---|
| wip/desktop-shell | 27e9e2f | f675fcc |
| wip/kernel-bt | 9f7ed27 | b54fda3 |
| wip/installer-boot | 5f99011 | fda77ad |
| wip/phone | 4b5aecc | f675fcc |

Severity: **blocking** (fix before merge, patch supplied), **major**, **minor**.
The patches were applied to throw-away clones and tested locally (crate tests only, no build server needed):
`fix-1` → `cargo test -p ultar-desktop --lib lock:: --test clicks_lock --test access` ok (the `screens` test
fails the same way without the patch: reference PNGs missing in the tree); `fix-2` → `cargo test -p ultar-netd
--lib wifi::` 8/8 ok, the new test fails without the fix ("not the twin"), clippy `-D warnings` clean; `fix-3` →
`cargo test -p ultar-surface`, `-p ultar-compositor --test call_screen` ok. `cargo fmt --check` clean for all three.

## Blocking

### S-1 blocking – Any account's password lifts the lock of someone else's session (wip/desktop-shell)
`platform/ultar-desktop/src/lock/mod.rs:117` (`choose_user`), `:194-205` (`verified` → `Unlock` for whoever was
checked), `platform/ultar-desktop/src/app/lock.rs:247-250, 311-318`; `rootfs/etc/ultar/services/lock.toml` passes no
`--user`. There is one compositor session; the sessions of `ultar-sessiond` are only bookkeeping (`sessions.rs:53`).
Repro: A is signed in and locks. On the lock screen pick B's card (any account, e.g. a non-admin guest), type B's
password → `auth.verify` Granted → `Request::Unlock` → B sees and uses A's desktop, A's open apps and windows.
Also: `choose_user` is allowed during `Stage::Checking`, so the user shown at the answer may differ from the user
whose secret was checked.
Fix (`fix-1.patch`): `LockState.owner` (from `--user`, else the first account that signed in) – only the owner's
Granted unlocks, another account gets `SignInError::OtherUser` (`lock.error.other_user`, ar/de/en); the checked
account is remembered in `checking`; no card switch while checking. Test `lock::tests::only_the_sessions_owner_unlocks`.
Consequence to know: user switching through the lock (`session.switch_user`) is refused until each person has an
own desktop; the owner is kept until `ultar-lock` restarts (sign-out should restart it or clear `owner` – follow-up).

### S-2 blocking – Saved WPA2 network is joined through an open "evil twin" (wip/desktop-shell)
`system/services/ultar-netd/src/wifi/join.rs:169` picks the strongest BSS of the SSID regardless of its security,
`:148` `Security::Open => Ok(None)` even when a key is saved or a password was given; auto-join
(`join.rs:139`, `core.rs:374`) only checks `known` (= SSID saved).
Repro: device saved "home" (WPA2). Attacker runs an open AP named "home" with a stronger signal → on the next
reconnect/start netd associates to the open AP, DHCP + DNS from the attacker, all traffic unencrypted. Same for a
manual join with the password.
Fix (`fix-2.patch`): `secured(j)` = password given or saved entry with PMK; `select` skips open BSSs of a secured
join, `key_for` refuses `Open` for it. Test `an_open_twin_of_a_secured_network_is_never_joined` (twin at −10 dBm vs
−40 dBm; join with password, restart auto-join, only-twin-left → no link).

### S-3 blocking (merge) – wip/phone reuses the right bit and opcode of wip/desktop-shell
`platform/ultar-surface/src/proto/types.rs:318` (phone) `CALL_SCREEN = Rights(64)` vs desktop-shell
`types.rs:310` `OUTPUT_CONFIG = Rights(64)`; phone `request.rs:268` `ClientHeader => 30` vs desktop-shell
`request.rs:342` `ShellFocus => 30`.
Repro (after a "keep both" conflict resolution): displayd (817, `ui.output-config`) could create a `CallScreen`
above the lock screen and take its pointer input; the phone app (`ui.call-screen`) could reconfigure outputs;
opcode 30 decodes as two different requests → the shell's focus request reaches the client-header path or vice versa.
Fix (`fix-3.patch`, against wip/phone): `CALL_SCREEN = 128`, `CLIENT_HEADER = 256`, `ALL = 447` (511 with
`OUTPUT_CONFIG` after the merge – set that in the conflict), `ClientHeader` opcode 42 (desktop-shell uses 1…41);
docs updated; test asserts the numbers and that bit 64 is unused. Role 8 (`CallScreen`) and events do not collide.

## Major

### S-4 major – Ultar kernel: uinput devices look like hardware to the compositor (wip/kernel-bt)
`system/hal/ultar-hal-gfx/src/evdev_devices.rs:60` decides "software device" by the sysfs class **link** containing
`/devices/virtual/`; the Ultar kernel serves `/sys/class/input/eventN` as a directory (`kernel/.../devices/sysfs.rs:124`),
so `read_link` fails → every `/dev/ultar/uinput` device is admitted as hardware, `--allow-virtual-input` is void.
`svcdev/uinput.rs:122` (`UI_DEV_SETUP`) and the legacy record let the holder pick name, bus, vendor, product (spoof
a USB keyboard or the allowed `ultar-osk` name – also on Linux).
What a malicious holder (btd parses hostile radio input) can do: type into the focused window of any session,
secure-attention/Super/shell keys, guess the lock password (throttled by authd), ≤ 8 devices × 1000 events/s.
Lifetime/limits themselves are fine (gone with the last close, 256/write, burst 256, ENOSPC at 8, EBUSY on hciN).
Fix: kernel marks virtual devices unforgeably (class link `../../devices/virtual/input/inputN` or a fixed
`BUS_VIRTUAL`, not overridable by `UI_DEV_SETUP`); compositor admits btd's devices by policy but takes the secure
attention keys only from hardware.

### S-5 major – Call screen can cover the lock screen and system dialogs at any time (wip/phone)
`platform/ultar-compositor/src/state/layers.rs:58` (rank 7 above Lock 6 and SystemDialog 4), `:151` (shown while
locked), full screen, all pointer/touch input while locked. Nothing ties it to a ringing call.
Repro: the holder of `ui.call-screen` maps a full-screen CallScreen that draws a copy of the PIN pad → on a phone the
PIN is typed by touch → taps go to the call screen (PIN harvest); unlocked, it hides a permission dialog.
Fix: while locked, lay the call screen out only in a band (or below the lock's entry area) and give the lock's
entry area pointer priority; never stack it above `SystemDialog` when unlocked; ideally the compositor shows it only
after a `ril`/`call` "ringing" event.

### S-6 major – PIN brute force: 5-minute cap, no lockout (wip/desktop-shell, authd)
`system/services/ultar-auth/src/throttle.rs:12` `MAX_WAIT_MS = 300_000`, waits stop growing at try 10; a 4-digit PIN
(`valid_pin` 4–12) falls in ≤ 10⁴ × 5 min ≈ 35 days of unattended guessing through the lock screen or an injected
keyboard (S-4). `service.rs:69` `Throttle::load(..).unwrap_or_default()`: a read error other than "damaged" (EACCES,
EIO) starts with no back-off (fail open). Fix: after N PIN failures disable PIN (password only), cap failures per day;
treat any load error like "damaged" (global wait).

### S-7 major – A system app can be granted `ui.lock-screen` / `ui.shell` / `ui.system-dialog` (wip/desktop-shell)
`system/core/ultar-launcher/src/perm.rs:41-46` (all UI rights are app permissions), `profile.rs:92` (system profile:
every permission grantable), bus `system.shell` may call `launcher.grant`. One "Allow" in a permission prompt gives a
system-signed app `Unlock` (lock-screen right), the shell protocol or trusted-dialog rendering. Fix: the four
system-UI rights are never app permissions (only the static rights file); keep `ui.capture` / `ui.overlay`.

### S-8 major – `noexec` is not enforced on the Ultar kernel (L-2 follow-up, wip/desktop-shell)
`system/hal/ultar-sys/src/cell.rs:84-85`: "the Ultar kernel does not know `noexec` yet and ignores it". On Linux L-2
is fixed (cell `/tmp` and `/data` `NOEXEC`), on the target kernel an app still runs what it downloaded. Track as a
kernel task; until then the doc must not promise "nothing may be executed".

## Minor

- **S-9** launcher `cleanup.rs:22-45` (`end_group`, Ultar kernel without `cgroup.kill`): SIGKILL to pids read from
  `cgroup.procs` – a member that exits between read and kill frees its pid; the launcher (`kill_any`) may hit a
  reused pid. Small window; prefer a kernel group-kill.
- **S-10** installer-boot `platform/ultar-installer-ui/src/firstboot.rs:125-138`: `.users.tmp` in the authd-owned
  `/data/ultar/auth` is opened `create+truncate` without `O_NOFOLLOW|O_EXCL`, then `chown` (follows links) as root →
  a compromised authd (813) at first boot could plant a link and get a root file chowned to 813. Use
  `O_EXCL|O_NOFOLLOW` and `fchown` on the open fd.
- **S-11** kernel-bt `svcdev/hci.rs` `sys_write`: any HCI command incl. vendor OGF 0x3F (firmware patch RAM, reset to
  bootloader) – a compromised btd can brick or reprogram the controller. Allow-list vendor commands.
- **S-12** desktop-shell compositor `state/shell.rs:237` `window_action` is not refused while locked (the shell can
  close/activate windows under the lock; `shell_only` in `workspaces.rs:56` does check `locked()`).
- **S-13** `--input-ack` (compositor bin `:236`): absent from every production service file (checked
  `rootfs/etc/ultar/services/compositor.toml`); it writes with `fs::write` (follows links) and leaks the key-event
  count (password length). Gate it behind a test feature so it cannot be enabled in production.
- **S-14** installer-boot: `ultar.profile` comes from `kernel_arguments()`; on Ultar it is the kernel's own
  `/proc/cmdline` (only `installer` when the medium session runs – good), on Linux it is the boot loader line
  (anyone at GRUB can boot the installer profile; disk nodes do not exist there, so impact is low). `bus.toml`
  listens on `installer.sock` in the system profile too (group 840, no members) – harmless, could be profile-bound.
- **S-15** phone `ui.client-header`: without the compositor title bar an app can imitate trusted UI; grant only to
  system apps (it is not in the launcher's permission list today – static rights file only, keep it so).
- **S-16** stale comment `bus-policy.toml` (netd rule): "busd and logd are at the kernel's 16 groups" – now 32.

## Verified, no finding

- Launcher round 1: **L-1** fixed (`install.rs:46-73`: system ids without `.`, store ids `<key>.<app>`, signer
  pinned per id incl. kept data, downgrade refused); **L-2** fixed on Linux (`mount_tmpfs` and `/data` NOEXEC, see S-8);
  **L-3** fixed (stage 1 kills stage 2 = pid 1 of the namespace, `PDEATHSIG`, leftovers ended by memory group at start);
  **L-4** fixed (revoke stops first, `write_rights_or_withdraw` removes the file, compositor fails closed on
  absent/refused, `app_rights.rs:93-135`, O_NOFOLLOW + fd metadata); **L-5** fixed (only stage 1 signalled,
  `instances.rs:28-30`). `launcher.start` v2 URI: ≤ 2048 bytes, no control chars, lowercase scheme declared in the
  manifest, only for the `ui` role, passed as data (`open.rs:80-112`).
- `settings.set_accessibility`: key checked before the schema (`settingsd/src/server/accessibility.rs:21`), normal
  caller rights apply; only `system.lock` may call it.
- Compositor: thumbnails/workspaces 38–40 `shell_only` (SHELL right, refused while locked, rate-limited, secure windows
  skipped); edge gestures only to SHELL clients; output requests behind `ui.output-config`.
- Bus policy: `app.*` gets no `*.system`/admin method; `notify.dismiss` checks the sender; `auth.*` only for
  lock/sysdialog/accountd/admin; `vpn.import` only `app.settings`.
- authd: Argon2id, dummy hash for unknown users, `Secret` Debug `<hidden>` and wiped, no secrets logged.
- netd: WPA2 4-way (MIC, replay counter, ANonce, RSN IE match, no PTK/GTK reinstall), PMK only (no passphrase) 0600,
  key types with redacted Debug; VPN `.conf` strict (no `PostUp`/`Table`, sizes bounded), names `[a-z0-9_-]`, 0600.
- Kernel groups 16→32: `set_groups` bounds-checked, fixed array.
- kernel-bt: nodes gated by their right only (`svcdev/mod.rs:55-61`), one hciN opener, records bounded.
- installer-boot: disk nodes only with `VOM_INSTALLER` and exactly one `ULTAR-INSTALL` disk, medium never a target and
  read-only (EROFS), open needs `raw_device` **and** uid/gid 840; target disks are not mounted; firstboot never
  overwrites an existing account file and wipes the record.
- phone: call screen gets no keys and cannot `Unlock` (test `call_screen.rs`); `ClientHeader` refused for secure windows.
