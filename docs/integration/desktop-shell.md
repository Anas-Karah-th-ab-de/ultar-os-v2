# Desktop shell, lock screen, system dialogs – integration

Built against HEAD `ce287d2` plus three patches that were applied first in the same tree:
the UI audit's design-conformance suite (`agents/w5/uiaudit/CHANGES.patch`), the new boards
(`agents/w5/boards/CHANGES.patch`) and the app launcher (`agents/w5/launch/CHANGES.patch`, its
contract `launcher.*` is used directly). Every element of every board covered here is listed in
[desktop-shell-matrix.md](desktop-shell-matrix.md) (board → element → action → status → code →
test); this file gives the architecture, the security properties and the open points.

## Programs

Three compositor clients, each its own process and kernel user (rootfs/etc/ultar/ids.toml), each
with one compositor right (compositor-rights.toml) and its own bus socket and policy rule
(services/bus.toml, bus-policy.toml):

| Program | User | Compositor right | Bus identity | May call / offers |
|---|---|---|---|---|
| `ultar-authd` (system/services/ultar-auth) | 813 authd | – | `system.authd` | offers `auth` (users, verify, set_secret) |
| `ultar-lock` (platform/ultar-desktop) | 814 lock | `ui.lock-screen` | `system.lock` | offers `lock`; calls `auth.users/verify`; counts `notify.posted` |
| `ultar-sysdialog` | 815 sysdialog | `ui.system-dialog` | `system.sysdialog` | offers `sysdialog` (permission, authenticate); calls `auth.*`, `settings.confirm/revert` |
| `ultar-desktop` | 816 desktop | `ui.shell` | `system.shell` | calls `launcher.*`, `settings.get/set/set_displays`, `lock.lock`, `sysdialog.permission` |

`ultar-lock` runs with `restart = "essential"` (new in ultar-init, `spec.rs` + `supervisor_exit.rs`,
test `an_essential_service_is_never_given_up`): a crash loop never ends its restarts, because a
locked session without a lock screen could never be unlocked; it stays locked meanwhile (the
compositor keeps the lock, §8a). The UI services are `manual = true` like the compositor.

## Compositor protocol additions (ultar-surface, ultar-compositor `state/shell.rs`)

Requests 30–33 (`ShellFocus`, `WindowAction`, `SetDisplayMode`, `SetKeyboardLayout`), events
32–35 (`ShellKey`, `WindowsChanged`, `KeyboardLayouts`, `ShellFocusLost`); popups of the shell's
panel; Super alone / Super+key go to the shell (never while locked); Caps Lock in modifiers. A
lock screen that attaches to an already locked session now gets `SessionLocked{true}` at once
(`shell_welcome`), so a restarted lock screen shows itself. Tests: `tests/shell_protocol.rs`.
**Overlap:** the launcher patch also edits `platform/ultar-compositor/src/state/mod.rs`
(app rights); both apply cleanly together in this tree.

## Lock screen security (tests/e2e.rs, ultar-auth tests, ultar-ui text_field tests)

- Fail closed: the compositor owns "locked"; no input reaches apps while locked; a dying shell or
  lock screen does not unlock; a new lock screen takes over (e2e, real compositor + bus).
- Passwords: Argon2id (m = 19 MiB, t = 2), per-user exponential waits 1 s·2ⁿ up to 300 s,
  persisted; a damaged file makes everyone wait the maximum. Unknown users cost a dummy hash.
- The typed password lives in one pre-reserved buffer that is zeroed on deletion and drop
  (`TextField` password mode, `widgets/text_field/secret.rs`); the bus `Secret` type wipes itself
  and prints `<hidden>`; no secret in logs, Debug output or command lines (`ultar-authd passwd`
  reads stdin).
- The lock screen shows per app only the number of notifications, never their content.

## The app launcher

`ports/bus.rs` calls `launcher.start {app, role: Ui, activation_token}` and `launcher.list` (the
installed apps replace the built-in list at start). The launcher has no start arguments, so
"Trash" opens Files, "Settings > Network" opens Settings, and "New window" starts or raises
(single instance per role) – recorded as DIFFERENT in the matrix.

## Tests

`cargo test -p ultar-desktop`: clicks (`clicks.rs` bar/sheets/overview/projection,
`clicks_lock.rs` lock/login/keep dialog, `clicks_grid.rs` app grid and banners – every CLICKS.md
entry of these boards has its effect, is disabled with a reason, or is listed as missing), access
(keyboard-only paths, screen reader tree), boards (sizes/positions/colours against the board HTML),
screens (snapshots of 18 surfaces × ar/en × dark/light in `tests/snapshots`), e2e (compositor + bus
on real sockets). Side-by-side renders: `agents/w5/shell/sbs/<Board>.png` (board | implementation
in the board's variant, below the four variants) for Main, DesktopBottom, QuickSettings,
CalendarSheet, Overview, AppGrid, Toasts, Lock, Login, Projection, SettingsDisplay, Menus.

## Open points (the matrix has the per-element list)

1. **Not built:** phone-size (compact) layouts – CompactHome/Apps/Quick/Lock are entirely missing
   at phone size; DesktopTop (top bar + dock, `shell.appmenu`); the window menu (Alt+Space,
   `win.menu.*`) belongs to the compositor's title bar; desktop icons (`desk.icon`) belong to
   Files.
2. **No service on the bus, so disabled with the reason or absent:** drive tile (`shell.drive`, no
   volume service), notification actions (`note.action`, `notify.posted` has none), calendar
   events (`cal.event`), media card (`lock.media`), Wi-Fi/Bluetooth/VPN/airplane/volume/output/
   screenshot tiles, power menu entries, emergency call/power-off, accessibility, user switching
   (other accounts on the Login board are shown but disabled), assistant, mesh.
3. **Toolkit differences** visible in the renders (owned by the toolkit agent): menu widths
   (T11), focus colour of text fields (T3), the eye button inside the password field, title-bar
   geometry (T17). The urgent banner has the "important" badge but not the accent edge (Card has
   no edge colour).
4. Overview: one workspace (the compositor has none); windows are titled cards without
   thumbnails (no capture for the shell).
5. Banners do not pause while the pointer rests on them.
6. `design/conformance/states.tsv` no longer matches the patched States boards (the boards patch
   moved the error column onto the artboard); `run.sh` stops there until the toolkit owner
   regenerates the manifest and its known differences.
7. QEMU end-to-end on the Ultar kernel was not run for the shell (only the Linux e2e).
