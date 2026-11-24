# System services behind the desktop shell and the Settings app (w7/svc, "Bau Dienst")

Patch: `agents/w7/svc/CHANGES.patch` against `main` = `f675fcc` (`patch -p1` in the repository root). It contains
the display and account services of `agents/w5/svc` (ported from `ce287d2` to `f675fcc` unchanged in behaviour) and
eleven new services, a shared service frame (`ultar-svckit`), generic kernel backends (`ultar-hal-linux`), the
power call of the service manager (`init.power`), accessibility settings and the end-to-end run
`tools/kern-test/dienste-e2e.sh` (`DIENSTE: ALLES OK`, wired into `tools/kern-test/ci.sh` as step 7b).

## Open points (first)

1. **Not done – Wi-Fi networks:** `net.scan` / `net.connect` answer `Unsupported`, `net.state.networks_supported`
   is `false`. Joining WPA networks needs a supplicant (nl80211 over generic netlink + the EAPOL 4-way handshake);
   not built in this round. Wi-Fi radio on/off, airplane mode and the wired link are real.
2. **Not done – Bluetooth devices:** `bluetooth.devices` / `connect` / `disconnect` answer `Unsupported`,
   `devices_supported: false` (needs an own HCI host stack; the Ultar kernel has no HCI socket). Presence and
   the radio switch are real.
3. **Not done – VPN:** no `vpn` service (WireGuard needs the kernel module's generic-netlink configuration and
   key handling; the legacy `system/services/ultar-vpn` shells out to `wg`). `qs.vpn` stays disabled with its reason.
4. **Not in my list, still disabled:** `qs.mesh` (Verbund), `lock.sos` (telephony), `qs.assistant`, mobile data
   (`modem.*`).
5. **Contract changes the UI branches must follow** (all in `ultar_bus::messages`):
   * `notify::Post` **version 2** has `actions: Vec<Action>` (≤ 3, ids `[a-z0-9_]{1,32}`); `Posted` (v2) has
     `sender` (bus identity, set by the service) – the shell's `model/notes.rs` test builds `Post {..}` and needs
     `actions: vec![]`; `Posted {..}` needs `sender`. New: `notify.invoke/dismiss/list/take_invoked`, topics
     `notify.dismissed`, `notify.invoked` (hint naming the sender only).
   * `power`: `power.actions` (retained: `suspend/restart/power_off/needs_admin`) – the shell enables the power
     menu entries from it; calls `power.suspend/restart/off` answer `ActionReply{Done|Unsupported|Cancelled|
     Refused|Busy|Failed}`. Lock = `lock.lock`, sign out = `session.sign_out` (not in `power`).
   * `session`: the sign-in screen reports a sign-in with `session.signed_in{user}` (pseudo method
     `session.signin`, only `system.lock`) **instead of** `account.session` – the session service tells the account
     service itself. `switch_user` locks and leaves the person signed in; `sign_out` locks first.
   * Shell proposals made concrete: `audio.state` has `present` + `has_volume`; `net.state` has `wifi_present`,
     `wifi_switch_off`, `airplane_possible`, `networks_supported`; `bluetooth.state` has `present`, `switch_off`,
     `devices_supported`; `calendar.day{date: Date}` → `{events: [{id,start_minute,duration_minutes,title}],
     more}`; `capture.screenshot` → `ShotOutcome::Saved(path)|NoUser|NoDisplay|Busy|Failed`; `media.state.now:
     Option<NowPlaying>` (None = no card); `volumes.state.mounted: [{id,name,uri}]`.
   * `lock.rs` is a byte-identical copy of the shell work's file; `auth.rs` / `sysdialog.rs` too (keep one copy
     when merging; merge the `messages/mod.rs` hunks).
6. **Credential store:** `system/services/ultar-auth` (authd, from `wip/desktop-shell`) is **in this patch** with
   two new calls `auth.update_user` / `auth.remove_user` (`src/manage.rs`, administrators only, the last
   administrator stays) – the account service needs them. Take this crate's version when merging the shell
   branch (it is the branch's crate + `manage.rs` + two arms in `server.rs` + `pub(crate)` fields).
7. **Policy for the shell's own identities:** `system.shell`, `system.lock` (now also `register = ["lock"]`),
   `system.sysdialog` (rule copied from the shell branch), `app.settings`, `app.calendar`, `app.*` got the calls and
   subscriptions listed below. The launcher (`system.launcher`, shell branch) should subscribe `session.state` and
   end a signed-out person's apps; the sign-in screen should offer `session.users`.
8. **Image:** create `/home` (root 0755); device groups: `audio` 830 (`/dev/snd/controlC*`), `radio` 831
   (`/dev/rfkill`), `disk` 832 (removable block devices) – `ultar-devd-seat` sets only seat nodes today;
   `/sys/power/state` must be writable by `powerd` (root:powerd 0660) for sleep. The `ultar-check` violation
   `apps/ultar-settings → ultar-surface` belongs to the settings branch, not this patch.
9. **Settings schema count:** this patch adds four `accessibility.*` flags (panel Appearance, 111 → 115 keys);
   the settings branch adds `display.auto_brightness` – merge both counts in `seed/tests.rs`.
10. **Gate findings not fixed here:** after the port to `f675fcc` two of the seven display-service process tests
    fail every run (`a_hung_settings_service_cannot_keep_a_change`: Super+P "this device only" is not applied;
    `a_mode_change_switches_the_display_and_waits_too`: the mode of the second output stays) – they pass on
    `ce287d2` in `agents/w5/svc`; `main` changed the compositor between the two (130 lines), cause not found in
    this round. The two tests are **flaky, not deterministic** (round 2: 0–2 of 7 fail per run, also serially and with
    umask 022; alone they pass) – a timing race in the display service around `display.apply`, not yet found.
    Fixed in round 2: the `ultar-busd` `kernel_credentials…` test and 14 `ultar-logd` `daemon` tests failed under
    umask 002 because the tests wrote the trust files (uid map, policy) group-writable and the services
    correctly refuse such files; the tests now write them 0644.
11. **Not tested on real hardware:** sound cards (ALSA control ioctls; unit-tested decoding, no card here or in
    QEMU), mounting real sticks (detection unit-tested; the process test shows the kernel refusing a non-device),
    rfkill on real radios, sleep on real hardware (tested with a prepared `/sys/power/state` only), plus the
    display-service points below.

## The services

| Service | Crate (bin) | uid | Bus name | What is real | Kernel right / group |
|---|---|---|---|---|---|
| power | `system/services/ultar-powerd` | 819 | `power` | battery/charger/temperature (`power.state`), sleep only where the kernel offers `mem` (screen locked first, fail closed), restart/power off through `init.power` (ordered shutdown), administrator dialog when others are signed in | – |
| session | `system/services/ultar-sessiond` | 820 | `session` | signed-in people, person in front, switch (lock first), sign out (lock first), kept in `/run` across crashes, drops removed accounts | – |
| notifications | `system/services/ultar-notifyd` | 821 | `notify` | numbered notifications with ≤ 3 buttons, sender identity, buttons back to the sender only, limits (64 open, 30/min/sender) | – |
| calendar | `system/services/ultar-calendard` | 822 | `calendar` | events per person (checked file, atomic), day view for the shell, add/remove for `app.calendar` | – |
| audio | `system/services/ultar-audiod` | 823 | `audio` | volume/mute/output of ALSA cards via control ioctls, restored at start, hot-plug; honest "no device" | group 830 |
| media | `system/services/ultar-mediad` | 824 | `media` | media session (heartbeat 30 s), lock-screen buttons to the player | – |
| screenshot | `platform/ultar-captured` | 825 | `capture` | reads an output from the compositor (`ui.capture`), PNG into the person's `Pictures`, owned by them, links never followed | `file_admin` |
| network | `system/services/ultar-netd` | 826 | `net` | wired link + speed, Wi-Fi radio, airplane mode (rfkill), wish restored; networks: open point 1 | group 831 |
| Bluetooth | `system/services/ultar-btd` | 827 | `bluetooth` | presence, radio switch (rfkill), wish restored; devices: open point 2 | group 831 |
| removable drives | `system/services/ultar-volumed` | 828 | `volumes` | FAT/exFAT/ext4 of removable disks mounted for the person in front (`nosuid,nodev,noexec`, FAT files owner-only), eject (refused while busy), pulled drives detached | `mount`, group 832 |
| display | `platform/ultar-displayd` | 817 | `display` | see below (w5/svc) | – |
| accounts | `system/services/ultar-accountd` | 818 | `account` | see below (w5/svc) | `file_admin` |
| credentials | `system/services/ultar-auth` (`ultar-authd`) | 813 | `auth` | shell work + `update_user`/`remove_user` | – |

Accessibility (`login.access`): settings keys `accessibility.large_text`, `.high_contrast`, `.screen_reader`,
`.on_screen_keyboard` (flags, default off, panel Appearance, ar/en/de texts) – read/written with `settings.*`,
followed via `settings.changed`.

Every new service: own crate, own user and group (`rootfs/etc/ultar/ids.toml`), service file
`rootfs/etc/ultar/services/<name>.toml` for ultar-init (`notify = true`, `restart = "always"`, `[cgroup]`
memory/pids/cpu limits = memory group), its own bus socket (`services/bus.toml`, 0660, its group) and log socket
(`services/logd.toml`), a bus-policy rule (default deny), hardening by `ultar-service` (no kernel rights unless
listed), log records with fixed keys only (`svc.ready`, `<name>.<event>` + words), no secret anywhere.

**Who may call what** (broker policy, then the service itself): system components get the pseudo method
`<service>.system` (`power.system`, `session.system`, `notify.system`, `media.system`) – an app given a call by
mistake is still refused by the service (`Refused`/`Unsupported`/`PermissionDenied`); the sign-in pseudo method
`session.signin` only for `system.lock`; `init.power` needs `init.shutdown` too (only `system.powerd` and `init.*`
administrators). Tested in every process test with an app that the broker lets through.

## Shared parts

* `system/core/ultar-svckit`: command line (`--bus --log-socket --token-file --allow-root` + own options),
  `main()` (hardening, kernel rights, logger, readiness, exit codes), the bus loop (one thread for the service,
  feeders for requests and subscriptions, ticks, `Bus::later` for slow work on ≤ 4 helpers, **stop grace 2 s**
  so an answer begun on a helper is delivered before the connection closes), checked state files
  (`ULTARSTATE1`, length + CRC-32, atomic temp+fsync+rename+dir fsync, every cut/changed byte detected),
  `Access` (pseudo methods), console texts ar/en/de, catalogue checker; feature `testkit`: in-process broker with
  token identities, service binaries as children (`--allow-root` as root), stand-ins.
* `system/hal/ultar-hal-linux`: `power` (power_supply, thermal, `sys/power/state`), `sound` (ALSA control:
  card info, element list/info/read/write – the only `unsafe`, four ioctls on pointer-free fixed-size buffers),
  `net` (`sys/class/net`), `rfkill`, `block` (removable partitions, FAT/exFAT/ext4 superblocks, `mount(2)`).
  Every backend takes a root directory; absent interfaces are reported absent.
* `ultar-init`: bus call `init.power{PowerOff|Reboot}` (`control.rs`): checked against `init.shutdown`, then the
  ordered shutdown; not PID 1 → init exits after it (that is how the e2e run ends).

## Runtime wiring

Ids 813 authd, 817 displayd, 818 accountd, 819–828 as above; groups 830 audio, 831 radio, 832 disk. The shipped
policy (`rootfs/etc/ultar/bus-policy.toml`) gives `system.shell` the calls of the shell table (power, session,
notify, calendar.day, audio, media, capture, net, bluetooth, volumes, display, accounts), `system.lock` sign-in,
power, session users and media buttons, `app.settings` display/account/audio/net/bluetooth, `app.calendar` the
calendar, `app.*` `notify.post/dismiss/take_invoked`, `media.update/release` and the hint topics. `captured` has
`825 = "ui.capture"` in `compositor-rights.toml`.

## Tests

Per crate (`cargo test -p <crate>`; process tests start the real binary against an in-process broker; root and a
plain user with umask 002): svckit 13, hal-linux 14, auth +1 (manage) , powerd 4 + 4 process (sleep after lock,
lock refusal stops sleep, restart/off through init, admin dialog with two people, cancelled dialog, apps refused,
missing init → Failed, hanging dialog → Busy for others, **kill -9 during the dialog** leaves nothing half done),
sessiond 2 + 2 (only the sign-in screen signs in, unknown accounts, switch/sign-out with and without lock screen,
**crash keeps sessions**, removed account drops, damaged state file → nobody), notifyd 4 (incl. fuzz) + 2 (buttons
to sender only, refusals, flood limit, crash empties), calendard 3 + 1 + 1 (per person, read-only shell, apps
refused, restart keeps, damaged file never overwritten), audiod 2 + 1 (fake cards: volume/mute/output/restore/
hot-plug; no device), mediad 1 + 1, captured 5 + 1 (**real compositor, real PNG of 64×48**), netd 3 + 1 (switches,
hardware switch, wish survives **kill -9**), btd 1 + 1, volumed 2 + 1, init +1 process (`init.power`), contracts
+11. End to end: `dienstetest` (host with `--fake-root`, and **QEMU on the Ultar kernel** via `dienste-e2e.sh`).

Gate (`agents/w7/svc/gate.sh`, log `gate.log`): fmt ok; clippy `-D warnings` on all touched crates ok (after one
fix); `ultar-check all` ok (no new baseline entry; one layer rule added, see above); `cargo test` of 24 crates
three rounds as root, umask 002: 923–924 passed, failures only those of open point 10; the test binaries of the
new services and display/account as uid 1000, umask 002: 142–143 passed per round, failures only displayd (open
point 10). Kernel image built from `f675fcc` (`scripts/build-kern.sh`); QEMU on the Ultar kernel (static release builds,
`dienste-e2e.sh` three times, `svc-e2e.sh` once): `DIENSTE: ALLES OK` ×3 and `SVC: ALLES OK`, every service ended
with code 0 (`agents/w7/svc/dienste-qemu-*.log`, `svc-qemu.log`). In QEMU the kernel offers no sound card, no
`sys/power/state`, no radio switches and no `sys/class/net`: the services report exactly that. The full `tools/kern-test/ci.sh` (A/B start chain,
memory-shortage runs …) was not run in this round; its new step 7b is `dienste-e2e.sh`.

## 2. Display and account services (agents/w5/svc, ported)

### Display and account services: open points (from w5/svc, still valid)

1. **Authentication service (now in this patch, see "Credential store" above).** The account service keeps *no*
   secret: every password goes to `ultar-authd` (`auth.*`), the single credential store the lock screen and the
   system dialog use too. Needed from the shell work and not done here (its crate is not in this tree):
   * `ultar-authd` must answer the two new management calls `auth.update_user` and `auth.remove_user`
     (`ultar-bus/src/messages/auth_admin.rs`, administrators only = identities with `auth.admin`). Without them
     creating a user rolls back (`credentials_unavailable`), kind/display-name changes and removal are refused.
     Suggested: two arms in `ultar-auth/src/server.rs`, `Auth::update(user, display, admin)` /
     `Auth::remove(user)` in `service.rs` with the same "last administrator stays" rule (`ManageOutcome::Rejected`).
   * `messages/auth.rs` and `messages/sysdialog.rs` are **byte-identical copies** of the shell work's files (so this
     patch builds alone); when both patches are applied keep one copy (they are the same) and merge the three
     `messages/mod.rs` hunks.
   * `ultar-sysdialog` asks `sysdialog.authenticate` with `app_name = app_id = <bus identity of the caller>` (no
     manifest name service exists); the action text is `accountd.action.<op>` with the user name as argument –
     the dialog must translate it from `accountd`'s catalog (`system/services/ultar-accountd/locales/`).
2. **The shell's own display bridge must go.** `ultar-desktop` (w5/shell) reports displays to settingsd
   (`report_displays` → `settings.set_displays`) and sends `SetDisplayMode` itself (`apply_display_mode`). With
   `ultar-displayd` that is done twice and with different ids. The shell should call `display.apply
   {Projection}` for Super+P and follow `display.outputs`; its `SetDisplayMode` request can stay for a session
   without displayd. Its "Keep?" dialog can stay as is (it answers `settings.confirm/revert`; `display.keep` /
   `display.revert` do the same for the display keys only).
3. **Textual conflicts with the shell / launcher patches** (same files, different lines): protocol opcodes
   (`ultar-surface`: shell uses requests 30–33 / events 32–35; this patch 34–37 / 36–38 – no numeric clash), the
   end of the `Request`/`Event` enums and their match arms, `Rights` (this patch: `OUTPUT_CONFIG = 64`, `ALL = 127`),
   `state/mod.rs`, `rootfs/etc/ultar/{ids.toml,bus-policy.toml,compositor-rights.toml,services/{bus,logd}.toml}`
   (ids 817 displayd / 818 accountd; shell uses 805, 813–816; both add a `[ranges]` section – merge into one:
   `apps = "20000-49999"`, `people = "1000-19999"`), `tools/kern-test/ci.sh`, busd/logd service-file tests.
4. **Policy for the Settings app** (sysapps' area): `app.settings` gets `display.*` and `account.*` (but not
   `account.session` / `account.setup`) here. It still has no `settings.*` rule in the shipped policy, which its
   own doc asks for – the end-to-end run therefore reads settings through `system.admin`.
5. **Real hardware not run.** `Kms` now reads the connector's `EDID` blob, lists every mode and switches modes
   (new dumb buffer → `set_crtc` → old buffer freed only after success) – unit-tested parts only, no DRM device
   here. Orientation is stored and planned but the compositor draws only upright (`Transform::Normal`): a turned
   display is shown upright and logged `displayd.orientation_unsupported`. HDR, brightness (DDC/backlight) and
   colour profiles are reported `false`/empty in `display.outputs` (no service for them).
6. **Linux without capabilities:** accountd keeps the kernel right `file_admin` (enforced on the Ultar kernel);
   on Linux a non-root service has no `CAP_CHOWN`, so a new home cannot be handed to the new id and the creation
   is refused (`home_failed`, nothing left). The image must create `/home` (root, 0755) and `/data/ultar/accounts`
   comes from `dirs`. People's ids are not written to `/etc/passwd` (build-initramfs writes only `ids.toml`).
7. `ultar-check all` reports one violation that is **not from this patch**: `apps/ultar-settings` → `ultar-surface`
   (sysapps' open point 1). Everything of this patch is clean; no baseline entry added.


### Display and account services: what exists

| Part | Where |
|---|---|
| Display service (lib + bin `ultar-displayd`) | `platform/ultar-displayd/` – `ids.rs` (EDID → settings descriptor, `display.outputs` entry), `values.rs` (display settings followed), `plan.rs` (settings → arrangement), `changes.rs` (`display.apply` → settings), `core.rs` (state machine, the three revert layers), `service.rs` (calls), `link.rs` (compositor thread, `poll` on socket + wake-up, reconnect), `ports.rs`, `daemon.rs`, `msg.rs` (ar/en/de) |
| Account service (lib + bin `ultar-accountd`) | `system/services/ultar-accountd/` – `core.rs` (rules, checks, all-or-nothing changes), `store.rs` (account file + pictures, CRC, atomic), `ports.rs` (auth.*, sysdialog, homes), `names.rs` (names, `ids.toml`), `picture.rs` (PNG/JPEG check), `throttle.rs`, `refusal.rs` + `locales/{ar,en,de}.ftl` (30 keys), `link.rs` (Ultar account, offline), `daemon.rs`, `msg.rs` |
| Compositor output management | `ultar-surface/src/proto/outputs.rs` (types, codec), requests `ConfigureOutputs`/`ConfirmOutputs`/`IdentifyOutputs`/`DetectOutputs`, events `OutputDetails`/`OutputSetupDone`/`OutputsDetected`, right `ui.output-config`; `ultar-compositor/src/{output_setup.rs, state/output_config.rs, server/outputs.rs}`, `output.rs` (placement, details), `backend/*` (modes, identity, `set_mode`), `decor.rs`/`render.rs` (display numbers) |
| HAL | `ultar-hal-gfx/src/edid.rs` (parser, fuzzed), `kms.rs` (EDID blob, mode list, mode switch), `ultar-hal` `Display::identification` (default `None`) |
| Bus contracts | `display_service.rs` (+ `Apply{Change}`, `Keep{more}`, `Revert`, `Projection`, error `code`), `account_local.rs` (new: local users), `auth_admin.rs` (new), `auth.rs` + `sysdialog.rs` (copies, see open point 1), `mod.rs` table |
| settingsd | `core/write.rs`: setting the *waiting* value again restarts its 15 s ("Keep more", §13 no. 43 – before it was a no-op, so the shell's "more time" did not work); test `setting_the_waiting_value_again_gives_the_full_time_once_more` |
| Runtime files | `rootfs/etc/ultar/services/{displayd,accountd}.toml`, bus/logd sockets per group, `ids.toml` (817, 818, `[ranges] people`), `bus-policy.toml` (rules below), `compositor-rights.toml` (`817 = "ui.output-config"`) |
| End to end | `platform/ultar-displayd/examples/svctest.rs` (+ `svctest/{setup,peers}.rs`), `tools/kern-test/svc-e2e.sh`, one step in `tools/kern-test/ci.sh` |

### Display service

**Flow.** The compositor sends `OutputDetails` (connector, EDID identity, size, every mode) to clients with
`ui.output-config` – only the display service (uid 817). displayd turns them into `DisplayDescriptor`s
(manufacturer, product as 4 hex digits, serial) → `settings.set_displays` → ids like `del-a0c4-8`
(`settingsd` keys the stored values **per display combination of these ids**: the same monitor on another port
keeps its settings; a projector is another combination) → `settings.list{display}` → plan → `ConfigureOutputs`.
It publishes `display.outputs` (retained, `BusSession` republishes after a broker restart).

**Plan** (`plan.rs`): extend / mirror (`mirror_displays`, < 2 connected → all) / single (`single_display`, else the
main display); main display = `display.primary` if connected, else the built-in one, else the first (§13 no. 64 –
there is no "off"); per display resolution (`auto` = preferred), refresh (0 = preferred, else nearest offered),
scale (25 % steps, 100–300 %), position (overlapping stored positions → side by side from the main display).

**`display.apply`** (`Change::Projection` Super+P – ThisDeviceOnly = built-in, SecondOnly = first other external –,
`Primary`, `Arrangement` after dragging, `Mode`) sets the settings (all or nothing; values already in effect are
left alone) → settingsd makes them provisional (15 s). **`display.keep{more}`** confirms every waiting display key,
or with `more` sets them again (restarts the 15 s). **`display.revert`** takes them back. `identify` (numbers 5 s,
drawn by the compositor in the middle of each output), `detect` (rescan, answer after `OutputsDetected`, 3 s).
Errors: `Service(1 UNKNOWN_DISPLAY, 2 INVALID, 3 SETTINGS_UNAVAILABLE, 4 NOT_PENDING, 5 NO_COMPOSITOR)`.

**Keep-or-revert – three layers** (all tested with the real processes):
1. settingsd takes the change back after 15 s – also when the asking app ended; displayd follows `changed` /
   `pending_resolved` and sends the old arrangement as final.
2. While a display key waits, every setup is **provisional in the compositor** with the remaining time + 3 s
   (`GRACE_MS`): if displayd hangs or settingsd hangs/ends, the compositor goes back by itself; if displayd's
   connection ends (crash, kill -9), **at once**. displayd then holds (sends nothing) until settingsd resolves the
   change, so the compositor's safe step is not undone.
3. At start displayd takes back display changes still waiting in settingsd (it ended inside the window).
A changed display setting waits ≤ 200 ms for its `pending`/`pending_resolved` (separate topics) before the
compositor follows – a provisional change never reaches the compositor as a final one.

**Compositor side** (`ui.output-config`, checked per request; others get `Permission` and are dropped):
`ConfigureOutputs{Setup{serial, arrangement, provisional_ms ≤ 120 s, outputs}}` is checked as a whole (every
connected output exactly once, modes from its list, one included primary, scale steps, positions ≤ 32768, no overlap
when extended, upright only) → modes switched on the backend (all or none, rolled back) → applied. A second setup
of the same client while one waits keeps the first "before"; another client gets `Busy`. Deadline, owner gone,
hot-plug → previous arrangement and modes back (`RevertedTimeout`/`RevertedHotplug`). `ConfirmOutputs{serial}`.
At most 4 setups queued. The display service counts as a system client (reserved slots).

### Account service

**Calls** (`account.*`, `messages/account_local.rs`): `users_list`, `user_create{name, display_name, kind,
password, confirm}`, `user_rename`, `user_picture{Initials | Builtin(<24) | Image(PNG/JPEG ≤ 256 KiB)}`,
`user_password{user, current?, new, confirm}`, `user_kind`, `user_auto_login{user?}` (at most one),
`user_remove{user, Keep|Delete}`, `user_picture_get`, `session{user?}` (sign-in screen only); topic `account.users`
(retained). Each answers `UsersReply{Ok(LocalUsers) | Err(Refusal{reason: Text})}`.
The Ultar account (`account.status` …, §12.1) is offline first: always `SignedOut` with `error =
accountd.link.no_server`; sign-in/create/server/delete/devices are refused with that reason, services with
`accountd.link.needs_account` – nothing pretends to be online.

**Rules:** own display name, own picture and own password *with the current one* need no dialog; everything else
(another user, kind, auto sign-in, create, remove, a password without the current one) needs an administrator's
password in the **system dialog, asked by accountd itself** (an app cannot skip or fake it). Exception: the very
first administrator of an empty device, by an identity the policy gives the pseudo method `account.setup`
(installer). Always ≥ 1 administrator; the signed-in user cannot be removed; system names/ids from `ids.toml` are
never given; ids from `--uids` (1000–19999), a removed user's id is never given again (files may carry it).
Checks run *before* the dialog. One dialog at a time (others `busy`), on its own thread – listing keeps answering.

**Security:** Argon2id hashing, verification and its back-off are in `ultar-authd` (argon2 crate). accountd: new ==
confirm with `constant_time_eq`, passwords only in the self-wiping `Secret` (Debug `<hidden>`), never logged or
stored; log records carry fixed words only (`accountd.changed op=create`). Limits: 8 changes per caller then one per
2 s; wrong current password → 1 s·2ⁿ up to 5 min per account (on top of authd's); ≤ 64 users; pictures checked
(fuzzed). A damaged account file (CRC line, strict parser) → every change refused (`store_damaged`), file not
overwritten. Creation is all or nothing: home (0700, chown to the new id) → password in authd → admin flag →
account file; any failure undoes the earlier steps. Removal: authd first, then the home is moved to
`/home/.removed/<name>-<uid>` (0700) or deleted (links not followed, `remove_dir_all`).

### Wiring

```
ultar-displayd --bus /run/ultar/bus/displayd.sock --compositor /run/ultar/ui/compositor.sock --log-socket /run/ultar/log/displayd.sock
ultar-accountd --bus /run/ultar/bus/accountd.sock --dir /data/ultar/accounts --homes /home --ids /etc/ultar/ids.toml
               --policy /etc/ultar/bus-policy.toml --uids 1000-19999 --log-socket /run/ultar/log/accountd.sock
```
Users 817 displayd (no rights) / 818 accountd (right `file_admin`, declared in the service file and kept by the
runtime: `spec.hardening.rights`). Policy rules added: `system.displayd`, `system.accountd`, `app.settings`
(display/account calls), `system.shell` (Super+P, keep/revert, identify), `system.lock` (`account.session`),
`system.installer` (`account.setup`, `user_create`). Image: install both binaries to `/usr/bin`, the accountd
catalogs to the service catalog directory the Settings app reads (`<catalogs>/accountd/<lang>.ftl`), create `/home`.

### Tests (x86_64 Linux; root and a user without root, umask 002; Ultar kernel in QEMU)

| Crate / target | Tests | What |
|---|---|---|
| `ultar-displayd` lib | 23 | plan (modes, positions, single/mirror, upright), changes (Super+P targets, checks), core (provisional, keep, revert, start revert, refusals, compositor revert + hold, settings unavailable → retry, hot-plug, companion wait), ids, values (bounds), daemon args, msg |
| `ultar-displayd` `tests/process.rs` | 7 | real displayd + in-process settingsd + compositor: ids, Super+P + keep, keep more + revert, **app ends**, **displayd killed** (compositor back at once, settings back after restart), **settingsd stopped** (compositor deadline, no undo, settingsd back), identify/detect/hot-plug/permission, mode switch |
| `ultar-accountd` lib | 22 | rules, rollback, own vs other, wrong password waits, last admin, remove keep/delete, damaged file, store (every cut + every changed byte), homes (links not followed), throttle, pictures (fuzzed), catalogs complete ar/en/de, link |
| `ultar-accountd` `tests/process.rs` | 7 | real accountd + broker + stand-ins: first admin without dialog, dialog for the rest, **standard user cannot change others or escalate**, own password, broker refusals, removal keep/delete + last admin, **kill -9 while the dialog is open**, damaged file |
| `ultar-compositor` | +3 unit, `tests/output_config.rs` 10 | rights, details, final/provisional/confirm/second-setup, owner gone, busy, 10 wrong setups + backend refusal rollback, single/mirror, hot-plug revert + detect, numbers (incremental == full frame) |
| `ultar-surface`, `ultar-hal-gfx`, `ultar-bus`, `ultar-settingsd`, `ultar-busd`, `ultar-logd`, `ultar-init` | +2, +3, +4, +1, +1 (changed 2), (1 changed), shipped files | codecs, EDID (fuzzed), contracts, keep-more, shipped service files/sockets |

Gate results and the QEMU verdict are in the agent's report (`agents/w5/svc/gate*.log`, `svc-qemu-run*.log`).

### Not done / not tested

Real DRM hardware (EDID, mode switching), turned displays drawn turned, HDR/DDC brightness/colour profiles,
Wayland-style per-window scale migration (compositor's existing behaviour unchanged), the real `ultar-authd` /
`ultar-sysdialog` (stand-ins in the tests, see open point 1), picture decoding (only the container is checked),
child protection / per-user language (§8 "Benutzer" – no service exists for them), online Ultar account.
