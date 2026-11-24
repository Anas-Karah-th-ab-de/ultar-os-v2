# Desktop shell, lock screen, system dialogs – integration

Round 2: built on `main` `f675fcc` (corrected toolkit, Compact*/AppGrid/WindowMenu/Toasts boards,
`ultar-brand`), with the app launcher version 2 (`launcher.start {uri, new_window}`) in the same
patch. Every element of every board covered here is listed in
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

Round 2 (overview, board Overview): requests 34 `WindowThumbnail {window, max_width, max_height}`,
35 `SwitchWorkspace {index}`, 36 `MoveToWorkspace {window, index}`; events 36 `WindowThumbnail
{window, width, height, data}`, 37 `Workspaces {current, count}`; `ShellWindow.workspace`
(`ALL_WORKSPACES` = 255). Rights check (`state/workspaces.rs`): only `ui.shell`, refused
(`Permission`) while locked; system dialogs are on every workspace and never copied; a copy is at
most 512 × 512 (1 MiB), at most 32 per client and second (`Limit`), scaled from the window's last
committed buffer (no decorations, nothing of other surfaces); at most 8 workspaces, empty ones at
the end go, a window opens on the shown one, `Activate` shows its workspace, Alt+Tab stays on
the shown one. Tests: `tests/shell_workspaces.rs` (4), `state/workspaces.rs` (fit), proto
round trips and bounds.

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

## The app launcher (`launcher.start` version 2)

`ports/bus.rs` calls `launcher.start {app, role: Ui, activation_token, uri, new_window}` and
`launcher.list`. `shell::Opening` says what to open: Trash `trash:///` (Files), Settings pages
`settings://network` (qs.wired), `settings://power` (qs.battery), `settings://keyboard`,
`settings://<panel>` (search results), `settings://apps/<id>` (App settings of `shell.appmenu`), the
Store's app page `store://app/<id>` (apps.details, About) and its removal dialog
`store://app/<id>?action=remove` (apps.uninstall; base apps cannot be removed), and "New window"
(`new_window = true`). A refusal of the launcher (scheme not declared, `12 no open` for a running
instance) shows the shell's own banner `desk.launch.refused` and keeps the launcher usable; an
unreachable launcher disables the app controls with their reason. The apps must declare their
schemes (`[open] schemes`): Files `trash`, Settings `settings`, Store `store` – owners of those apps.

## System services (round 3: wired to docs/integration/system-services.md)

The shell follows the retained topics `power.actions`, `session.state`, `audio.state`, `net.state`,
`bluetooth.state`, `volumes.state`, `media.state`, `notify.dismissed`, `calendar.changed`
(`app/desktop/topics.rs` → `model::services::Services`) and makes each control's call as
`Effect::Service(Call)` through `ports::services::ServicesPort` (bus implementation
`ports/bus_services.rs`; lock screen `LockCall`). A control whose service never reported stays
disabled with its reason; an answer other than done becomes the shell's banner
(`desk.service.failed`), a saved screenshot `desk.service.shot_saved`.

| Control (CLICKS id) | Contract | State |
|---|---|---|
| `qs.wifi`, airplane (`qs.toggle`), `qs.bluetooth` | `net.set_wifi`, `net.set_airplane`, `bluetooth.set` | works; tile hidden without the radio, disabled by the hardware switch / airplane mode |
| `qs.bluetooth.list` | bluetooth v2: `devices`, `scan`, `pair`, `connect`, `disconnect`, `forget` | works: devices page (`view/bluetooth.rs`), refreshed on `bluetooth.state`; pairing asks through `sysdialog.bt_pair` |
| `sysdialog.bt_pair` (system dialog) | `PairConfirm {device_name, address, kind, question: Consent / Compare(n) / Show(n)}` → `PairConfirmReply{accept}` | works: trusted frame, number as six digits, Show closes when `bluetooth.state.pairing` leaves Running; unanswered → `accept: false` |
| `qs.wifi.list`, join dialog | `net.networks/known/join/disconnect/forget`, topic `net.wifi` | works (round 3): networks page + join dialog (`view/wifi.rs`, `shell/network.rs`), also in the phone's sheet; `net.networks` (15 s), `net.join` (30 s) and `vpn.set` run on a worker (`app/desktop/slow.rs`), the loop never waits; disabled with the reason without a Wi-Fi device the service drives |
| `qs.volume`, `qs.output` | `audio.set_volume`, `audio.outputs` + `set_output` (menu) | works; disabled without a device / volume control |
| `qs.shot` | `capture.screenshot` | works |
| `qs.power` | `power.suspend/restart/off` (from `power.actions`), `session.sign_out`; switch user = lock screen | works |
| `note.action`, `note.dismiss` | `notify.posted` v2 `actions` → `notify.invoke`; `notify.dismiss`, topic `notify.dismissed` | works |
| `cal.event` | `calendar.day` (on open, day change, `calendar.changed`) → opens `calendar://event/<id>` in `org.ultar.calendar` | works |
| `shell.drive` | `volumes.state` tiles → opens the uri in Files; menu `volumes.eject` | works |
| `lock.media`, `lock.power` | `media.previous/play_pause/next`, `power.off` (when `power.actions` allows it without an administrator) | works |
| `login.user` | every account; after the password check `session.signed_in {user}` (not `account.session`) | works |
| `login.access` | `settings.accessibility`, `settings.set_accessibility` (round 3, lead decision) | works: menu of four switches (`lock/access.rs`); the narrow right accepts only `accessibility.*` with the schema's types, every other key `PermissionDenied` (settingsd `server/accessibility.rs`, test `tests/accessibility.rs`); `system.lock` has no `settings.set`/`get` |
| `qs.vpn`, `qs.vpn.list` | `vpn.state`, `vpn.set` | works (round 3): last used on/off, none set up → `settings://network/vpn`, the connections page; disabled with the reason without TUN (`NoTun`, the Ultar kernel today) |
| `lock.sos`, `qs.mobile`, `qs.mesh`, `qs.assistant` | telephony, modem, mesh, assistant | no service yet: disabled with the reason (or absent) |

`bus-policy.toml`: the duplicate rules of `system.shell`, `system.lock`, `system.sysdialog` and
`system.authd` (one from each branch) were merged into one each; `system.sysdialog` subscribes
`bluetooth.state`; `account.session` was taken out
of `system.lock` (the session service tells the account service). Protocol opcodes: the display
service took requests 34–37 / events 36–38, so the overview's are requests 38–40, events 39–40.

**Supplementary groups:** the bus and the log service are members of one group per service with a
socket; with the session (814–816) and the system services (817–828) these are 20 groups, more than
the 16 the kernel and `ultar-sys` allowed. Both bounds are now 32 (`kernel/ultar-kern/src/cred.rs`,
`ultar-sys` `MAX_GROUPS`, `rechtetest`, init spec test) – **kernel owner: please confirm.** The bus's
`pids.max` is 384 (its sockets' connection bounds need 282 threads).

## Tests

`cargo test -p ultar-desktop`: clicks (`clicks.rs` bar/sheets/overview/projection,
`clicks_lock.rs` lock/login/keep dialog, `clicks_grid.rs` app grid and banners – every CLICKS.md
entry of these boards has its effect, is disabled with a reason, or is listed as missing), access
(keyboard-only paths, screen reader tree), boards (sizes/positions/colours against the board HTML),
screens (snapshots of 18 surfaces × ar/en × dark/light in `tests/snapshots`), screens_shapes (11 more:
phone home/apps/quick/search/lock/sign-in, top bar with app menu, quick settings, overview, tile menu, and the overview with window copies `overview-thumbs`),
phone (every `mobile.*`/`qs.mobile*` click, board geometry at 360 × 800), top (DesktopTop geometry,
`shell.appmenu`), e2e (compositor + bus on real sockets, incl. banners: hover pause, expiry, Esc; a wrong then the right password in the same lock screen). Side-by-side renders (round 2, toolkit of main): `agents/w7/shell/sbs/<Board>.png` (board | implementation
in the board's variant, below the four variants) for Main, DesktopBottom, QuickSettings,
CalendarSheet, Overview, AppGrid, Toasts, Lock, Login, Projection, SettingsDisplay, Menus, and (round 2)
DesktopTop, CompactHome, CompactApps, CompactQuick, CompactLock.

## Shapes of the shell (round 2)

`ultar-desktop` is one program with three shapes (`model::Shape`, `shell/shapes.rs`); state and
click handling are the same, only the panels and the place of popups differ.

| Shape | Chosen by | Surfaces |
|---|---|---|
| bar at the bottom (standard, §11) | default | panel bottom 56 + popups |
| top bar + dock (board DesktopTop) | setting `bar.layout = "top_bar_dock"` (followed live: the panels are reopened) or `--bar top` | panel top 32, translucent panel bottom 68 (dock 56 + 12), popups of the top panel |
| phone (boards CompactHome/Apps/Quick) | logical width of the primary output ≤ `layout.compact_max` (599), or `ULTAR_OBERFLAECHE=handy\|compact\|phone` / `--form`; `desktop` forces the desktop | panel top 32 (status bar), panel bottom 44 (gesture bar), `Role::Background` (home screen), popups of the status bar (all apps, search/recent apps, quick settings sheet over the screen above the gesture bar) |

`ultar-lock` shows the cover of board CompactLock on a compact screen (`lock/compact.rs`).
The bar follows the setting `bar.layout` (round 3). On a phone the compositor runs in phone mode
(round 3: no title bars, the whole work area, edge gestures).
New toolkit bricks (ultar-ui `widgets/brick_phone.rs`, `brick.rs`): `PhoneCell`, `PhoneStatus`,
`Handle`, `Scrim`, `BarButton`; `Card::top_sheet()` and `Card::dock()`.

## Banners in the running shell (round 2, found by `tests/e2e.rs`)

Two defects of round 1 were found by the new end-to-end test and fixed: the banners only waited
while the pointer was on their *close button* (now: anywhere on the banner surface,
`HostSurface::pointer_inside`), and "Esc closes the newest" could never happen because the bar
never has the keyboard (now: Esc in an open popup of the shell closes the newest banner first,
then the popup). With the keyboard in an app Esc stays with the app.

## Brand (docs/BRAND.md §3, §4)

Mark through the icon pipeline (`IconId::Mark`, `accent_text`, never mirrored; `BrickKind::Mark
{points}` for sizes other than 24): overview button 24 (bar, dock, phone), lock and login 48 at the
start of `brand.product` (weight 600, key `lock.brand` removed), label `brand.label.mark`; the
overview's empty state 96 + "no windows". No second mark in the overview.

## End to end on the Ultar kernel (round 2)

`tools/kern-test/desktop-e2e.sh` (in `ci.sh`, and in `vmware-like.sh` on pc + NVMe + e1000e + xHCI +
VMware SVGA II): `examples/desktoptest` sets a test account with `ultar-authd passwd` on stdin, starts
`ultar-init` with the shipped definitions of logd, bus, seat, authd, compositor, lock, sysdialog,
desktop (paths moved, `manual`/`editions` dropped), checks every one runs as its user (801/802/0/813/
811/814/815/816); `tools/kern-test/desktop-qemu.py` then judges screenshots: the session starts locked
(`--start-locked-in system`, docs/integration/vmware.md §6) → Space + the test password → desktop bar with the mark →
Super+L (virtual USB keyboard, `sendkey`) → lock cover → Space, a wrong password (still locked) →
the test password → the bar is back: `DESKTOP: ALLES OK`. Found and fixed by it: the lock screen
stayed disabled after the wait of a wrong password (`app/lock.rs`, regression test
`the_same_lock_screen_takes_the_password_after_the_wait`). The VMware image
(`tools/vm/make-vmware.sh`) now runs this session instead of the two demo windows; its account's
password comes from `VM_PASSWORD_FILE` or is random (`out/vmware/UltarOS-v2.password`).

## Round 3 (agents/w8/shell3)

Base: `wip/desktop-shell` 4cfe800 + the network work (`git diff b54fda3 ce2316c`, conflicts in
system-services.md and bus-policy.toml merged: both rule sets, network §3, Bluetooth §4).

* **Toolkit overlay layout** `Flex::stack()` (ultar-ui `ui/stack.rs`): children in one box, later
  ones on top and hit first; a growing child fills the box, the others sit by `main`/`cross`.
  Tests: `tests/layout.rs` (geometry ltr/rtl, hit), snapshot `stack-corner-normal-*`. Used by the
  overview's close button (inside the card's top end corner, inset 4).
* **Phone mode of the compositor** (`state/phone.rs`): request 41 `SetPhoneMode {on}` (right
  `ui.shell`), event 41 `EdgeSwipe {edge: Bottom | Side, held}` (shell clients only, never while
  locked), `WindowAction::Back` (Escape pressed and released in the window's app). In phone mode
  app windows have no title bar and fill the work area (maximise does nothing); a touch or a
  primary-button drag starting within 16 points of the bottom or a side edge is kept from the
  apps; 48 points make a swipe, a finger resting 400 ms at the end makes "held". The shell asks
  for it whenever its shape is the phone (`app/desktop/panels.rs`); `Shell::on_edge`: home,
  recent apps, back (the shell's sheet first). Tests: compositor `tests/phone_mode.rs` (4),
  desktop `tests/phone.rs`.
* **Arranging apps on the phone** (`model/arrange.rs`, `shell/arrange.rs`): drag on the home
  screen moves an app or puts it into the favourites row (the row keeps three); in all apps an
  app dropped onto another makes a folder (≤ 16 folders, ≤ 16 apps each; opens, takes more,
  "drag here to take it out"); the hint wraps. In memory like the pinned apps (see open points).
* **Wi-Fi and VPN** in the quick settings (both shapes), **login.access**: see the table above.
* **Bar position:** the shell follows `bar.layout` (`bottom_bar` | `top_bar_dock`), which the
  settings schema already has in panel "Desktop bar" – the panel the Settings app's area
  "desktop" shows (wip/settings-installer). A second key `desktop.bar_position` would let two
  settings contradict each other, so it was not added (lead decision deviated – see open points).
* **End to end:** the compositor takes `--input-ack <file>` (test only: the number of key events
  taken, never which key); `desktoptest` passes it and prints `EINGABE: <n>`;
  `desktop-qemu.py` sends each key once and waits for its acknowledgement, sending it again only
  when it never reached the compositor; Space is judged by the screen change.

## Open points (the matrix has the per-element list)

1. **Different / missing: none** (matrix). **Service missing:** mobile data, Mesh, emergency
   call, assistant – disabled with the reason.
2. **Apps must declare their schemes** (`[open] schemes`): Files `trash`, Settings `settings`,
   Store `store`, Calendar `calendar`. None of these apps is in this tree (Settings on
   wip/settings-installer, Store on wip/store-qwen, Files/Calendar nowhere): their owners add the
   line to their `ultar-app.toml` together with handling the URI; until then the launcher refuses
   and the shell shows its banner.
3. **Settings app (wip/settings-installer):** VPN import (`vpn.import {name, config: Secret}`, refusals
   with `ConfProblem` + line) and Settings > Network > VPN are the Settings owner's; its area
   "network" is still "no service". The bar position needs nothing there (generic panel editor).
4. **Not persisted:** the phone's arrangement (home order, favourites, folders) lives in memory like
   the pinned apps of the bar; a settings key for it is not in the schema.
5. **Accessibility settings take effect** only where something follows them: the toolkit does not
   render large text / high contrast yet, and there is no screen reader or on-screen keyboard
   service (toolkit / services owners).
5. Locale texts of round 2 and 3 were added by hand in ar/en/de (`locales/*.ftl`).
