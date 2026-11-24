# System services behind the desktop shell and the Settings app (w7/svc, "Bau Dienst")

Patch: `agents/w7/svc/CHANGES.patch` against `main` = `f675fcc` (`patch -p1` in the repository root). It contains
the display and account services of `agents/w5/svc` (ported from `ce287d2` to `f675fcc` unchanged in behaviour) and
eleven new services, a shared service frame (`ultar-svckit`), generic kernel backends (`ultar-hal-linux`), the
power call of the service manager (`init.power`), accessibility settings and the end-to-end run
`tools/kern-test/dienste-e2e.sh` (`DIENSTE: ALLES OK`, wired into `tools/kern-test/ci.sh` as step 7b).

## Open points (first)

1. **Wi-Fi networks – done in round 3 (w8/net), see section 3:** scan, join open and WPA2-Personal networks,
   DHCP, DNS, saved networks, automatic reconnection. **Not tested on Wi-Fi hardware** (this host's kernel has no
   cfg80211, so neither `mac80211_hwsim` nor hostapd could be used): the handshake is tested against an
   independent recorded reference, nl80211 against the uapi header, DHCP against a real dnsmasq. Not done:
   WPA3-only (SAE: no reviewed pure-Rust implementation – reported `Unsupported`), enterprise, hidden networks,
   PMF-required networks, IPv6 on Wi-Fi, DHCP on the wired link.
2. **Bluetooth devices: own host stack** – see section 4 (w8/bt). Linux: HCI user channel + `/dev/uinput`; Ultar
   kernel: USB controllers through `/dev/ultar/hciN` and input through `/dev/ultar/uinput` (section 4a, w8/kbt).
   Audio devices are listed but not connected (A2DP needs SBC + audio routing).
3. **VPN – done in round 3 (w8/net), see section 3:** WireGuard (boringtun) over TUN, `.conf` import, keys
   0600, `vpn.set` for `qs.vpn` and Settings. It runs **inside `ultar-netd`** (`vpn` is a second bus name of
   uid 826): busd and logd already hold the Ultar kernel's maximum of 16 supplementary groups, so a new service
   user cannot get its own bus/log socket. The Ultar kernel has no TUN: `vpn.state.available = false, NoTun`.
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
10. **Display-service race – fixed in round 3 (w8/net):** the compositor reports its displays one event at a
    time; `displayd` settled after the first batch, sent a setup naming only the first display, and the
    compositor refused it (`UnknownOutput`). That refusal arrived after the second display had been seen and set
    `held` for good – no later change (Super+P, a mode) ever reached the compositor. A person clicking right
    after login hit the same. Fix in `platform/ultar-displayd/src/core.rs`: a refusal of a setup planned for
    another display list than the current one is stale – it is dropped and the next settle plans again (no
    hold, no revert); a new waiting change of the person also releases an old hold. New unit test
    `a_setup_planned_while_the_display_list_still_arrived_is_planned_again` (fails without the fix); the 7
    process tests 20× green in a row (previously 0–4 of 7 failed per run; a run now takes 2 s instead of 22 s).
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
| network | `system/services/ultar-netd` | 826 | `net`, `vpn` | wired link + speed, Wi-Fi radio, airplane mode (rfkill), wish restored; Wi-Fi networks (WPA2-Personal supplicant, DHCP, DNS, saved, reconnect) and the WireGuard VPN: section 3 | `net_admin`, group 831 |
| Bluetooth | `system/services/ultar-btd` | 827 | `bluetooth` | presence, radio switch (rfkill), wish restored; own host stack: scan, pair (system dialog), bonds, connect, HID keyboards/mice (section 3) | `net_admin`, group 831 |
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
hardware switch, wish survives **kill -9**), btd 28 + 1 (+ 6 stack end-to-end/trace, section 4), volumed 2 + 1, init +1 process (`init.power`), contracts
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

## 3. Network: Wi-Fi networks, DHCP/DNS, VPN (w8/net, round 3)

Patch `agents/w8/net/CHANGES.patch` against `wip/system-services` = `b54fda3`.

### Open points (first)

1. **No Wi-Fi hardware test.** This host's kernel has no cfg80211 (no `mac80211_hwsim`, so hostapd could not
   be used). What is tested instead: the 4-way and group-key handshake byte for byte against a recorded
   reference made independently (`ultar-netproto/tests/vectors/gen_handshake.py`: Python hashlib/hmac and the
   `cryptography` package's RFC 3394 key wrap) plus the IEEE 802.11 PSK (J.4) and PRF test vectors; every
   nl80211 constant against `linux/nl80211.h` (printed by a C program); the whole join/reconnect logic against an
   authenticator and a DHCP server in the test; DHCP and addresses/routes against a real `dnsmasq` over veth
   between network namespaces. First run on a laptop with a real card is still to do.
2. **Not done (Wi-Fi):** WPA3-only networks (SAE – no reviewed pure-Rust SAE; listed as `Wpa3Personal`,
   `joinable: false`, join answers `Unsupported`; transition-mode networks are joined with WPA2), enterprise
   (802.1X), WEP/WPA1 (refused), networks that require management-frame protection (PMF/AKM 6), PTK rekeying
   inside a connection (the access point then disconnects and the service reconnects), hidden networks (no
   directed probe), IPv6 on Wi-Fi (no SLAAC/DHCPv6), the ARP check of an offered address (RFC 5227), DHCP on the
   wired link (wired is still only reported). Drivers without `CONTROL_PORT_OVER_NL80211` (some FullMAC) cannot
   run the handshake: join answers `Failed`.
3. **Not done (VPN):** one peer per tunnel (as client configs have), no kill switch, server names are resolved
   once at switch-on (std resolver), no `wg`-style live reconfiguration. The Ultar kernel has no TUN device
   (`vpn.state.unavailable = NoTun`); TUN in the kernel is the next step for VPN on the device.
4. **Image:** `/etc/resolv.conf` must be a link to `/run/ultar/net/resolv.conf` (written by netd, 0644, VPN
   servers first, at most three). netd now asks for the kernel right `net_admin` (service file). On a plain
   Linux host netd would need CAP_NET_ADMIN, CAP_NET_RAW (`SO_BINDTODEVICE`) and CAP_NET_BIND_SERVICE (UDP 68).
5. **Dependency:** boringtun 0.7 (default features off: only its `noise` protocol module) pulls `ring` (C and
   assembly, built static by cc – already in the lock file for other crates), `x25519-dalek`, `chacha20poly1305`,
   `blake2`, `hmac`. Pure-Rust replacements for ring inside boringtun do not exist upstream.

### Contract changes for the UI branches (all additive; `ultar_bus::messages::net` and new `vpn`)

* `net.scan` / `net.connect` (unchanged messages) are real now. **Outcome change:** where no Wi-Fi device is
  reachable they answer `NetOutcome::NoDevice` (was `Unsupported`); `net.state.networks_supported` is `true`
  only with a Wi-Fi interface driven by the service, `net.state.ssid` / `signal_percent` are filled.
  `net.connect` joins and remembers; `Failed` covers wrong password, not found, timeout (use `net.join` for the
  reason). `net.scan` lists one entry per name (strongest access point), hidden networks left out.
* New calls (version 1): `net.networks{}` → `NetworksReply{outcome, networks: [NetworkInfo{ssid, bssid,
  signal_percent, frequency_mhz, security: WifiSecurity, known, joinable, connected}]}` (every access point,
  strongest first); `net.join{ssid, secret: Option<Secret>, remember}` → `JoinReply{outcome: JoinOutcome}` with
  `Connected | NoDevice | RadioOff | NotFound | PasswordNeeded | BadPassword | WrongPassword | Unsupported |
  Refused | Timeout | Busy | Failed`; `net.disconnect{}` → `NetReply` (no automatic reconnection until the next
  join); `net.forget{ssid}` → `NetReply` (`Failed` when not saved); `net.known{}` → `KnownReply{ssids}` (most
  recent first). **Call timeout for `net.join` / `net.connect`: 30 s** (scan ≤ 10 s, association ≤ 8 s,
  handshake ≤ 8 s); `net.scan` / `net.networks`: 15 s.
* New retained topic `net.wifi`: `WifiLink{phase: NoDevice | Off | Disconnected | Scanning | Associating |
  Authenticating | ObtainingAddress | Connected, ssid, signal_percent, address "a.b.c.d/n", router, dns}`.
* New module `vpn` (bus name `vpn`, served by netd): retained `vpn.state` = `VpnState{available, unavailable:
  Option<NoTun | NoRight>, profiles: [VpnProfile{name, endpoint, addresses, full_tunnel}], active, status: Off |
  Connecting | Up | Failed, handshake_age_secs, rx_bytes, tx_bytes, dns}`; `vpn.import{name, config: Secret}` →
  `ImportReply{outcome: Done | BadName | Exists | TooMany | Invalid{problem: ConfProblem, line} | Unavailable |
  Failed}`; `vpn.set{on, name: Option}` → `VpnReply{outcome: Done | Unavailable | NotFound | Failed}` (name
  `None`: last used, or the only one) – **this is `qs.vpn`'s toggle**; `vpn.remove{name}` → `VpnReply`.
  `ConfProblem`: `Unreadable | Syntax | UnknownKey | Duplicate | Missing | BadKey | BadAddress | BadEndpoint |
  BadNumber | PeerCount` – show the line number; `UnknownKey` covers `PostUp`/`PreUp`/`Table`/… (never run).
* Policy (`rootfs/etc/ultar/bus-policy.toml`): `system.shell` and `app.settings` got `net.networks`,
  `net.join`, `net.disconnect`, `net.forget`, `net.known`, `vpn.set` (Settings also `vpn.import`, `vpn.remove`)
  and subscribe `net.wifi`, `vpn.state`; `system.netd` registers `net` and `vpn`, publishes `net.wifi` and
  `vpn.state`, subscribes `vpn.state`.
* `ultar-svckit`: `Bus::poster()` → `Poster<S>` (work posted to the loop from a service-owned thread) and
  `main_pair(program, options, rights, build → (A, B))` (one program, two bus services, each its own connection).

### Also fixed while gating (load of other agents on the machine exposed them)

* `ultar-powerd` and `ultar-captured`: the answer to a power action / screenshot was sent from the helper
  thread, but "busy" was cleared only afterwards on the service loop – a caller asking again right after the
  answer got `Busy` (a second press of the screenshot key, suspend after a failed suspend). "busy" is now an
  atomic flag cleared on the helper before the answer (`src/service.rs` of both). The process tests
  `state_actions_and_sleep_after_locking` (uid 1000: 2 of 3 rounds failed) and
  `the_screenshot_lands_in_the_pictures_of_the_person_in_front` (5 of 6 failed under load) are green again.
* Process tests of netd, calendard and captured: right after `kill -9` the broker can still route a call to the
  dead connection and answers `ServiceGone`; the tests' call helper now asks again on `ServiceGone` as it did on
  `UnknownService` (test code only – a UI should treat `ServiceGone` the same way).

### What exists

| Part | Crate / file | What |
|---|---|---|
| Protocols without sockets | `system/core/ultar-netproto` (new) | `wpa`: PMK (PBKDF2-HMAC-SHA1 4096), PRF, PTK, EAPOL-Key frames (MIC HMAC-SHA1-128, constant-time check), RSN element, supplicant state machine (4-way + group key; replay counter, ANonce and RSN-element downgrade checks, no key installed twice – KRACK), keys zeroized; `ie`: SSID and security of scan results; `dhcp`: messages and lease machine (selecting, requesting, bound, renew T1, rebind T2, expiry, NAK, backoff 4–64 s) |
| Kernel network interfaces | `system/hal/ultar-hal-netlink` (new) | netlink codec and socket (requests, ACKs, dumps, events kept bounded), generic netlink family lookup, nl80211 (interfaces, scan, connect with `SOCKET_OWNER` + `CONTROL_PORT_OVER_NL80211`, EAPOL tx/rx, keys, port authorisation, signal), rtnetlink (links, MTU, up, addresses, routes), TUN, the DHCP socket (UDP 68 bound to the interface, broadcast); `unsafe` only in `sys.rs` (two socket options, `TUNSETIFF`) |
| Wi-Fi in the service | `system/services/ultar-netd/src/wifi/` | `core` + `join`: scan, choose the strongest access point, associate, handshake, keys, DHCP, signal every 10 s, reconnect with saved keys (5 s doubling to 5 min; after `disconnect` none); `saved`: name + PMK (never the password), MRU, ≤ 64, checked file 0600; `worker`: own thread polling nl80211, the DHCP socket and a wake-up socket; `calls.rs`: the bus calls, answered from the worker (the service loop never waits); `dns.rs`: `resolv.conf` |
| VPN | `system/services/ultar-vpnd` (new library, run by `ultar-netd`) | `conf`: strict `.conf` reader (one `[Interface]`, one `[Peer]`, known keys only, refusals with line); `tunnel`: TUN `ultar-vpn0`, addresses, MTU (1420), routes (full tunnel: `0/1` + `128/1` and a host route to the server through the old gateway, removed again), boringtun `Tunn` on its own thread, incoming packets checked against `AllowedIPs`, roaming on authenticated packets; `service`: `vpn.*`, tunnels in `<state>/vpn/tunnels/<name>` (0600, dir 0700), wish restored at start |

### Tests

`ultar-netproto` 22 (recorded handshake byte for byte, wrong password → MIC failure, replays, repeated message
3 answered without keys, downgrade, nonce change, fuzz of frames/elements/DHCP with fixed seeds, DHCP state
machine incl. renew/rebind/expiry/NAK), `ultar-hal-netlink` 14 (codec + fuzz, real rtnetlink and genl on the
host, nl80211 absent → `Unavailable`), `ultar-netd` 12 unit (Wi-Fi core: no device, scan, open + DHCP, WPA2
keys = PTK derived in the test, port opened, rekey + replay, wrong password not saved, WPA3/not found/
password needed/bad password/busy/radio off, reconnect after loss and after restart, disconnect stops it,
forget) + `dhcp_netns` (real dnsmasq over veth between namespaces: lease, default route, removal; root) +
`process` (host without Wi-Fi: `NoDevice` everywhere, radios, kill -9) + `vpn_process` (no TUN → `NoTun`; as root
in `unshare -n`: import with refusals and line numbers, file 0600 without the key text, switch on, VPN DNS first
in `resolv.conf`, kill -9 → tunnel restored, off, remove), `ultar-vpnd` 5 unit + `tunnel_netns` (two tunnels in
two namespaces over veth, full tunnel on one side: split routes, server host route, UDP echo through the tunnel,
handshake and byte counters, host route removed afterwards; root). `dienstetest` checks `net.wifi`/`net.join`
(`NoDevice`) and `vpn.state`/`vpn.set` (`NoTun`/`Unavailable`) on the host and in QEMU on the Ultar kernel.

Gate (`agents/w8/net/gate.sh`, `gate.log`): fmt ok; clippy `-D warnings` on 21 crates ok; `ultar-check all` ok
(no new baseline entry; `tools/check-layers.toml`: services may use `ultar-hal-netlink`, netd may use the
`ultar-vpnd` library); `cargo test` of 21 crates three rounds as root, umask 002: 574 passed, 0 failed each;
the test binaries of the 8 touched crates as uid 1000, umask 002: 273 passed, 0 failed each (root-only network
tests say "skipped"); displayd process tests 20/20 green. Kernel image built from this tree
(`scripts/build-kern.sh`); QEMU on the Ultar kernel `dienste-e2e.sh` 4× (3 before the last refactor and the
powerd/captured fix, 1 after): `DIENSTE: ALLES OK`, every service ended with code 0
(`agents/w8/net/dienste-qemu-*.log`).

## 4. Bluetooth host stack (agents/w8/bt)

### Bluetooth: open points (first)

1. **Ultar kernel transport: done (w8/kbt, section 3a)** – `/dev/ultar/hciN` per USB controller, `ultar-btd`
   uses it when no Linux user channel exists. Tested against an emulated controller only (QEMU 8.2 has no
   `usb-bt-dongle`; `tools/kern-test/bt-usb.py` behind `usb-redir`), **not with a real dongle**.
2. **Ultar kernel input for services: done (w8/kbt, section 3a)** – `/dev/ultar/uinput` (Linux `uinput`
   interface); btd's sink picks `dev/uinput` (Linux) or `dev/ultar/uinput`. A full air-to-seat run (a paired
   Bluetooth keyboard on Ultar) needs a real controller.
3. **Audio sinks disabled** (`Device.audio_unsupported`, `pair`/`connect` of an audio device answer
   `Unsupported`). A2DP needs: SDP (record search for the A2DP sink), AVDTP signalling (discover, get
   capabilities, set configuration, open, start; PSM 0x19) and its media channel, an **SBC encoder** (A2DP
   mandatory codec; a reviewed crate or a port – not written here), AVRCP for the buttons (optional), and audio
   routing: `ultar-audiod` must offer the Bluetooth sink as an output and hand PCM (44.1/48 kHz, 16 bit) to
   btd, with latency/buffer limits. HFP (headset microphone) additionally needs SCO/eSCO over HCI or the
   controller's PCM path.
4. **Shell agent: answer `sysdialog.bt_pair`** (contract below). Until the system dialog implements it, every
   pairing ends "Failed" after the call fails (safe: no answer = no). The settings app should call
   `bluetooth.scan` / `devices` / `pair` / `connect` / `disconnect` / `forget` and follow `bluetooth.state`.
5. **Not tested with a real controller** (none here; the build machine's kernel has no Bluetooth, so neither
   `/dev/vhci` nor `btvirt` exist): the HCI user channel and uinput are compiled and unit-tested (layouts), the
   protocol stack is tested against the specification's sample data, hand-written traces and two hosts over a
   **simulated** air (below). First run on hardware: `hciconfig hci0 down` (the user channel needs the
   controller down and `CAP_NET_ADMIN`), then start `ultar-btd --root / --state DIR`.
6. **Not in this round:** LE report-protocol HID (report maps; only boot protocol – every HID keyboard/mouse
   must support it), battery level, LE connection parameter updates (refused politely), our own IRK/privacy
   (we use the public address), classic HID devices that do not support boot protocol, more than one pairing
   at a time, passkey *entry* on our side (we display; an Ultar device has no "type the number" dialog).

### What exists

`system/services/ultar-btd`: `stack/` is a host stack **without I/O** (fed packets, wishes and the time; answers
packets and news), `runner.rs` the stack thread (transport, bonds file, input sink, snapshot), `service.rs` the
bus side. Reviewed primitives only: `p256` (ECDH, point validation), `aes` (AES-128), `cmac` (AES-CMAC),
`subtle` (constant-time compares), `zeroize`, `getrandom`; the protocol state machines are ours.

| Layer | File | What |
|---|---|---|
| HCI | `stack/hci.rs` | H4 framing, commands, events (strict parsing, lengths checked), EIR/AD (name cleaned: no control characters, ≤ 64 octets) |
| host | `stack/host.rs`, `events.rs` | controller setup (reset, address, buffers, event masks, SSP + SC host support, LE host, extended inquiry, page scan only with bonded classic devices, never discoverable), LE + BR/EDR scan (ends after 20 s), devices (≤ 64) with name/class/appearance/RSSI, one command in flight, ACL credits, time limits (connect 15 s, pairing 60 s) |
| L2CAP | `stack/l2cap.rs` | reassembly (frame ≤ 1024 octets), fixed LE channels, BR/EDR signalling (connect/configure/disconnect), ≤ 4 channels/link, incoming channels only HID of encrypted bonded devices |
| SMP | `stack/smp.rs` | **LE Secure Connections only**: Just Works, Numeric Comparison, Passkey (we display), both roles; refuses legacy pairing, keys < 128 bit, a reflected public key (CVE-2020-26558), points off the curve; Pairing Failed ends it, later PDUs dropped; IRK + identity address of the peer kept (private addresses resolved with `ah`) |
| SSP | `stack/events.rs` | classic Secure Simple Pairing: IO capability `DisplayYesNo` + MITM, numeric comparison / consent / passkey shown – the controller does the P-256 work; PIN (legacy) refused; only SSP link keys (types 4, 5, 7, 8) kept |
| GATT | `stack/att.rs` | HOGP client: HID service, characteristics, protocol mode → boot, CCCDs of boot keyboard/mouse input, notifications; our own GATT server is empty ("request not supported") |
| HID | `stack/hid.rs`, `link.rs` | boot keyboard (modifiers, 6 keys, rollover kept) and mouse (3 buttons, X/Y, wheel) → Linux input events; classic HID control (`SET_PROTOCOL boot`) + interrupt channels |
| bonds | `stack/bonds.rs` | `/data/ultar/bluetooth/bonds` (svckit state file: CRC, atomic, **0600**), ≤ 32, keys zeroed on drop, never in `Debug`/logs/bus |
| HAL | `ultar-hal-linux/src/bluetooth.rs`, `uinput.rs` | controller list (`sys/class/bluetooth`), HCI user channel (`sockaddr_hci`, one `unsafe impl`), uinput keyboard/mouse (five integer ioctls) |

**Default deny:** nothing pairs unless the person asked for that device (`bluetooth.pair`) and confirmed in the
system dialog (also Just Works: "pair with this device?"); an incoming pairing / IO capability request nobody
asked for is refused; incoming connections only from bonded classic devices (HID reconnect); a bonded LE device
whose key no longer works is disconnected, never re-paired silently. Service: kernel right `net_admin` (user
channel), group 831 (rfkill), bus calls only from `system.shell`/`app.settings` (policy) – apps are refused.

### Contract (`ultar_bus::messages::bluetooth`, version 2; `BluetoothState` version 3)

`bluetooth.state` (+ `scanning`, `pairing: Option<{id, phase: Running|Paired|Failed}>`, `found` – unpaired
devices of the running or last scan – and `devices_revision`, which changes with every change of the device list:
a shown list asks `bluetooth.devices` again when it changes, so found devices appear while the scan runs; the
service publishes at most twice a second), `bluetooth.devices`
(`Device` + `paired`, `kind`, `rssi`, `le`, `input`, `audio_unsupported`), calls `scan{on}`, `pair{id}`,
`connect{id}`, `disconnect{id}`, `forget{id}` → `BtReply{Done|NoDevice|SwitchOff|Unsupported|Failed|
UnknownDevice|Busy}`; ids are `AA:BB:CC:DD:EE:FF` (`/r` = LE random). `pair` answers when the pairing started;
its end shows in `bluetooth.state.pairing`.

**For the shell agent – `sysdialog.bt_pair`** (`PairConfirm`, defined in `messages/bluetooth.rs`, the one place):
`{device_name, address, kind, question: Consent | Compare(u32) | Show(u32)}` → `PairConfirmReply{accept}`.
Called only by `system.btd` (policy rule added: `call = ["sysdialog.bt_pair"]`), timeout `CALL_TIMEOUT_MS`;
unanswered within `ANSWER_WITHIN_MS` → `accept: false`. Show it in the trusted frame like `permission`:
Consent: "Pair with {name}?" [Pair] [Cancel]; Compare: the number as six digits (leading zeros) "Does {name}
show this number?" [Yes] [No]; Show: "Type {number} on {name}, then Enter" [Cancel] (the answer only cancels;
close the dialog when `bluetooth.state.pairing` leaves `Running`). The name is data from the air: plain text.

### Tests (x86_64 Linux, root, umask 002)

`cargo test -p ultar-btd`: 28 unit + 1 process + 4 simulated air + 2 classic trace:
* **Specification sample data:** f4, f5 (MacKey + LTK), f6, g2 (Core Vol 3 Part H App. D), ah (RPA resolution),
  the two P-256 samples (Core Vol 2 Part G 7.1.2: both public keys and the DHKey) – as the Linux kernel's self test
  carries them; points off the curve refused.
* **SMP:** two state machines against each other: numeric comparison accepted / refused on either side, Just
  Works with a display-less device, passkey (right and wrong number, 20 rounds), legacy/short keys/bad
  parameters/not pairable refused, reflected public key, tampered confirm value and DHKey check fail, 3000 hostile
  PDUs (fixed seed) never yield keys.
* **HCI/L2CAP/ATT/HID:** events in spec layout (hand-written – no controller to record from), 20 000 hostile
  events/ACL/AD records (fixed seed), a HOGP combo device trace, ATT refusals, 3000 hostile ATT sequences,
  keyboard/mouse report decoding, bond file 0600 / damaged file / limit / IRK resolution.
* **End to end (`tests/sim.rs`):** two complete hosts over two simulated LE controllers (commands answered as the
  specification says, ACL forwarded with a 27-octet buffer = fragmentation, encryption granted only if both sides
  hold the same LTK): scan finds the advertiser with name and RSSI, pair with numeric comparison (same number on
  both), same LTK on both sides, disconnect, reconnect with the bond and no question, forget; a "no" on either
  screen leaves no bond and drops the link; a non-pairable peer; a stale bond is not re-paired behind the
  person's back. `/dev/vhci` / `btvirt` are not available here (no Bluetooth in this kernel).
* **Classic trace (`tests/classic.rs`):** inquiry with EIR name, pair: link key negative reply, IO capability
  reply, numeric comparison through the dialog, link key kept, encryption, HID control + interrupt channels,
  `SET_PROTOCOL(boot)`, a key press reaches the input sink, disconnect removes it; IO capability / PIN / user
  confirmation / connection requests nobody asked for are refused, an unasked link key is not kept.
* `ultar-hal-linux` +2 (controller list, uinput record layouts; uinput absent → refused), `ultar-bus` contract
  round trip + names.


## 4a. Bluetooth and virtual input on the Ultar kernel (agents/w8/kbt)

### Open points (first)

1. **No real controller tested.** QEMU 8.2 has no `usb-bt-dongle`; the transport is tested with
   `tools/kern-test/bt-usb.py` (usbredir device side, full-speed class E0/01/01: commands answered, ACL loopback,
   events cut into 16-octet interrupt packets) behind `-device usb-redir` on qemu-xhci. First run on hardware:
   look for `[bt] hciN: USB Bluetooth controller` in the kernel log, then `bluetooth.state.devices_supported`.
   btd logs `btd.controller` (HCI version, manufacturer) and every command the controller refuses
   (`btd.hci_refused` opcode/status) – a controller that needs vendor firmware first (Intel, some Realtek) refuses
   the scan commands; a scan refused in both parts (inquiry and LE) ends at once.
   Scan: inquiry (GIAC, 10.24 s, results with RSSI in both layouts, extended inquiry results with names), remote
   name requests for the nameless ones after the inquiry (one at a time, 12 s each at most), LE scan active (names
   from scan responses) or passive (`Config::le_active`), duplicates reported so the RSSI follows; one entry per
   address, a new scan starts with a fresh list (paired and connected devices stay).
   A command the controller does not answer within 4 s is given up (`btd.hci_refused` with status 255) and the
   next one goes out; a controller that refuses the inquiry beside its LE scan (Command Disallowed) scans in
   turns: LE for 10 s, then the inquiry alone (7.68 s).
   Transport: the event endpoint is read one USB packet per transfer with up to 32 transfers queued (an event whose
   length is a multiple of the packet size has no zero-length packet after it and was held back until the next
   event with one long transfer); interrupt endpoints get their Max ESIT Payload.
2. SCO/eSCO (interface 1, isochronous) is not offered: HFP audio needs it (see open point 3 of section 3).
3. One controller node is opened by one descriptor at a time; more than four controllers are not bound.
4. The kernel's xHCI driver works by polling: an open `hciN` is pumped on `read`/`poll` (every 8 ms while a
   reader waits), events are not interrupt-driven.
5. **Gate:** one full `ci.sh` run on builder B2 (QEMU 11.0.3, also without `usb-bt-dongle`): every marker green incl.
   `BTTEST-UINPUT`, `BT`, `DIENSTE`; `AB-TEST` 43/44 – "installation in the A/B layout" (the serial-console
   driven installer's end text) missed while the following factory-table and start checks passed; not
   investigated (budget). `ab-test.sh` also never copied the kernel's path dependency `ultar-brand` into its
   panic-kernel build (failed on the base too) – fixed. The builders lack `rust-lld` (run with a `lld` link
   named `rust-lld` on `PATH`). The second gate run was not done.

### Kernel interface

| Node | Right (only gate) | What |
|---|---|---|
| `/dev/ultar/hciN` (char 10:240+N) | `bluetooth` (bit 10) | one H4 record per `write` (`01` command → control request 0x20/0 on EP0, `02` ACL → bulk OUT, ≤ 1021 data octets; else `EINVAL`, not taken `EIO`), one per `read` (`04` event from interrupt IN, `02` ACL from bulk IN; streams cut by their headers; buffer too small `EINVAL`, record kept); `O_NONBLOCK`/`poll`; second opener `EBUSY`; controller gone `ENODEV`; ≤ 256 records / 128 KiB queued (more dropped, logged at close) |
| `/dev/ultar/uinput` (char 10:223) | `input_inject` (bit 11) | Linux `uinput`: `UI_SET_EVBIT/KEYBIT/RELBIT/ABSBIT` (keys 1..0x2ff, rel 0..15, abs 0..63), `UI_DEV_SETUP`, `UI_ABS_SETUP`, legacy `uinput_user_dev` write, `UI_DEV_CREATE` → normal `/dev/input/eventN` (uevent, sysfs, `EVIOCG*` from the declaration), `UI_GET_SYSNAME` → `inputN`, `UI_GET_VERSION` 5, `UI_DEV_DESTROY`; events must be declared (else `EINVAL`, nothing delivered; abs held to range); ≤ 256 per write; rate 1000/s, burst 256 (`EAGAIN`); ≤ 8 devices (`ENOSPC`); gone with the last close / process end |

`RAW_DEVICE`, `FILE_ADMIN` and node owner/mode open neither node (default deny). New rights in
`isolation/rights.rs`, `ultar-service` (`Right::Bluetooth`, `Right::InputInject`) and `ultar-init` (`bluetooth`,
`input_inject`); `btd.toml` keeps `net_admin, bluetooth, input_inject`; `entwickler.toml` (remote control)
gains `input_inject`. Files: `kernel/ultar-kern/src/drivers/gen_xhci/bt.rs` (transport), `src/svcdev/`
(nodes, gate), `src/input/virt.rs` (declared capabilities). HAL: `ultar_hal_linux::bluetooth::{ultar_nodes,
RawNode}`, `ultar_hal_linux::uinput::{node, VirtualInput::create_at}`; btd: `runner::open` tries the Linux user
channel, then the Ultar nodes; the `Uinput` sink uses `uinput::node(root)`.

### Tests

* `bttest uinput` (first ci boot, `BTTEST-UINPUT: ALLES OK`, 38 checks): rights (827 with RAW_DEVICE+FILE_ADMIN,
  root without the right → `EACCES`; 827 with it opens and creates), keyboard seen by the seat (name, id, bits,
  sysfs, events in order, POLLIN), undeclared / wrong values refused, legacy setup record (btd's HAL sequence),
  rate limit, 257 events refused, destroy, tablet range + clamp, 8-device limit, empty declaration refused,
  device gone at close and when the process ends.
* `tools/kern-test/bt-e2e.sh` (`BT: ALLES OK`): `bttest hci` (22 checks: rights, EBUSY, EAGAIN, poll, Reset,
  Read BD_ADDR, a 258-octet event over 17 interrupt packets, ACL 300 and 1021 octets both ways, SCO/bad length/
  oversize refused, small buffer, dup/close) and `examples/btd-e2e` (shipped logd/bus/btd under ultar-init: btd
  as uid 827 sets the controller up – the controller log shows its Set Event Mask/SSP/LE host commands –,
  `bluetooth.state` present + devices supported, second opener `EBUSY`, btd's `Uinput` sink creates a keyboard
  the seat reads, removed with it; btd ends with code 0). The controller's scripted air: `bluetooth.scan` finds a
  phone (inquiry result with RSSI, name by remote name request), a keyboard (extended inquiry result, RSSI
  updated), a mouse (LE, 32-octet report) and a pen (LE, 16-octet report, named by its scan response), not the
  non-connectable beacon; `bluetooth.state` publishes them while scanning, `bluetooth.devices` lists each once,
  and the desktop's Bluetooth page built from that has a row for each.
* Kernel rights now 12 bits: `R_ALL` 0xfff in rechtetest/zelltest/grenztest/ausbruch. `scripts/build-kern.sh` now
  rebuilds the ring-3 test programs every time (`make -B`): the checked-in `fenstertest` binary was older than its
  source (same checkout time stamp), which failed `VMWARE-LIKE q35-ahci-usb` on the base commit as well.
* `restart_and_power_off…` (ultar-powerd) flake: the busy flag was cleared on the service loop *after* the worker
  had already answered, so an immediate next call raced and got `Busy` (~1 in 3). Now cleared in the worker before
  the answer (`Arc<AtomicBool>`); 40/40 runs green.
