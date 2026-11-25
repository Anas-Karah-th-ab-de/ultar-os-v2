# Settings – what takes effect, what waits, what is hidden

KM Code Craft · Ultar OS v2 · 2026-11-24

Rule of the owner: **a setting must take effect.** A switch that only stores a value is not offered.
The table of record is `apps/ultar-settings/src/wired.rs` (`WIRED`); the generic page
(`view/panel.rs`) shows every row of the 17 newer panels that is not in it disabled, with the note
"Not yet available in this version" (ar/en/de) in the row and as the tooltip of its control. The ten
panels of the first seed (display, appearance, desktop bar, windows, notifications, power, region,
date and time, privacy, account) are followed by the shell, the display service and the session as
before and are not part of this rule.

## 1. Wired (changing it has an effect)

| Page | Setting or action | Who follows it | How | Test |
|---|---|---|---|---|
| Mouse and touchpad | `pointer.speed` (−10…10), `pointer.natural_scroll` | compositor | displayd follows `settings.changed` and sends `Request::ConfigurePointer` (again after every reconnect); relative motion × 2^(speed/5), scroll deltas reversed | compositor `tests/output_config.rs`, displayd `tests/process.rs` (real settingsd + displayd + compositor), settings-e2e |
| Keyboard | `keyboard.repeat.enabled`, `.delay_ms`, `.per_second` | compositor | displayd sends `Request::ConfigureKeyRepeat`; the compositor thins out the driver's repeats (`state/key_repeat.rs`) | the same three |
| Accessibility | `accessibility.large_text`, `.high_contrast`, `.reduce_motion` | shell (bar, sheets, lock screen) and the Settings window | `Look::with_access`, `Ui::set_reduce_motion`; Settings: `App::follow_access` | desktop `link.rs` tests, settings-e2e |
| Search | `search.source.apps`, `.settings`, `.calculator`, `.commands`, `search.arabic_variants` | the shell's overview search | `model/search.rs` `Sources`, read at start and on `settings.changed` | desktop `model/search.rs`, `link.rs` tests, settings-e2e |
| Storage | "Space used": this device and every mounted drive, free of total with a bar | volume service | new call `volumes.usage` (statvfs of the data partition and of each mount of the person in front) | volumed unit + `tests/process.rs`, settings `tests/groups.rs`, settings-e2e (real volumed; `tools/settings-e2e`) |
| Apps | installed apps with their declared permissions; switching one off takes it back | launcher | `launcher.list`, `launcher.permissions`, `launcher.revoke` (granting stays with the system dialog) | settings `tests/groups.rs` (fake service) |
| Users | add, remove, change password | account service | `account.users_list`, `account.user_create`, `account.user_remove`, `account.user_password`; the account service asks for the administrator in the system dialog | settings `tests/groups.rs` (fake service) |
| Users | `users.list_on_login` | lock screen | read at start and on `settings.changed`; off: the sign-in shows only the session user's name field, no account cards (`lock/prefs.rs`) | desktop `lock/prefs.rs`, `lock/mod.rs` tests, settings-e2e |
| Users | `users.auto_login` | account service + lock screen | accountd keeps the per-user flag (`account.user_auto_login`) in sync with the device setting; at a fresh start (nobody signed in) the lock screen signs the named user in without a password – never into a locked session of somebody else, never outside the parental window | accountd `settings.rs`, `core_tests.rs`, `tests/process.rs`; desktop `app/lock_follow.rs`; settings-e2e |
| Users | `users.new_kind` | the Settings app's Users page | the add-user dialog says the configured kind and creates that kind; an invalid value means standard | settings `tests/users_kind.rs`, settings-e2e |
| Users | `users.parental`, `.parental.from`, `.parental.until` | lock screen + session service | standard users sign in only in the window (the lock screen refuses with the hours, sessiond refuses too); sessiond warns five minutes before the end (`notify.post`) and signs them out at it, locking the screen first when one of them is in front; administrators are never restricted; a window over midnight works | desktop `lock/prefs.rs`, `lock/mod.rs`; sessiond `parental.rs`, `tests/process.rs`; settings-e2e |
| Emergency | `emergency.on_lock_screen`, `.contact_name`, `.contact_phone`, `.medical` | lock screen | the cover's Emergency button opens a panel showing exactly the stored contact and medical details – nothing else – without unlocking; off: the button is disabled with the reason | desktop `lock/emergency.rs`, `lock/view.rs`; settings-e2e |
| Emergency | `emergency.number` | call service + lock screen | `call.emergency` carries no number: calld reads and validates it itself (digits, optional `+`, 3–16; invalid → 112) and dials it on the mobile network (slot 1, else 2); the lock screen shows the outcome | calld `emergency.rs`, `tests/service.rs`; settings-e2e |
| Emergency | `emergency.sos_light` | lock screen | while the emergency call is placed or active, the screen flashes SOS (··· −−− ···, 200 ms unit) – the screen is the light the lock screen owns; the flash ends when `call.state` no longer lists the call | desktop `lock/emergency.rs` tests, settings-e2e |
| Updates | "Check now", "Update from a drive", state line | update service | `update.check`, `update.offline`, `update.progress` | settings `tests/groups.rs` (fake service) |

Limits that are part of the truth:

- Key repeat is thinned out, not generated: the delay cannot be shorter and the rate not higher than
  what the keyboard driver reports (usually 250 ms and about 30 per second). The page offers 150–1000 ms
  and 5–60 per second; values beyond the driver's are capped by it until the input layer can set the
  driver's own repeat. A driver that reports no event times is passed through unchanged.
- Large text, high contrast and reduced motion reach the shell, the lock screen and the Settings
  window. Other apps follow once they read the settings (app bus access, branch build/appbus).
- The Apps, Users and Updates sections are tested against fake services only: the kernel run
  (settings-e2e) starts no launcher, account or update service.
- Automatic sign-in happens only at a fresh start (nobody signed in, the lock screen's first
  frame); a locked session always keeps its owner. The parental window and the SOS light use
  the system clock (UTC until `datetime.timezone.name` is wired) – the same basis in the lock
  screen and the session service. The SOS light is the screen: no torch service is shipped.

## 2. Shown disabled with the note "Not yet available in this version"

Nothing in the system follows these yet. Each line says what it waits for.

| Page | Settings | Waits for |
|---|---|---|
| Sharing | `sharing.device_name`, `.visible`, `.networks` | a discovery service that announces the device |
| Sharing | `sharing.files`, `.files.folders`, `.files.read_only`, `.files.password` | an SMB file-sharing service (none shipped) |
| Sharing | `sharing.media` | a media-sharing (DLNA) service (none shipped) |
| Sharing | `sharing.screen` | a remote-desktop service (none shipped) |
| Sharing | `sharing.ssh` | see "SSH" below |
| Printers and scanners | all (`printers.*`) | a printing and scanning service (none shipped) |
| Backup | all (`backup.*`) | a backup service (none shipped) |
| Colour | all (`colour.*`) | colour management in the compositor (ICC profiles) |
| Mobile network | all (`mobile.*`) | the modem service on the bus offers calls only (`modem.dial` …), no data, roaming or APN |
| Mesh | all (`mesh.*`) | `ultar-mesh` is on main (`mesh.set_enabled`, `mesh.set_rights`); this branch is based on build/appbus, which does not contain it yet – wire `mesh.enabled`, `mesh.visible` and the per-device rights when the base has it |
| Assistant | all (`assistant.*`) | the assistant: `ultar-llm` is started by the old surface and has no bus contract |
| Cells and containers | all (`cells.*`) | `ultar-zellen` has no bus contract and no shipped service definition |
| Emergency | `emergency.power_presses` | the power key never reaches a system handler on this branch: the seat's input path (hal-gfx evdev → displayd → compositor) does not report it to a service, and powerd (user 819) may not open the seat's input nodes – that would let it read every keystroke. Wiring needs the compositor to report the key to powerd (a new surface request and a narrow `power.key` call), outside this package |
| Emergency | `emergency.key_escape` | holding both volume keys must close the app, then the session, then restart – the three stages belong to the compositor/shell input path, which does not report the volume keys as a pair yet (outside this package) |
| Mouse and touchpad | `pointer.primary_button`, `.acceleration`, `.size`, `pointer.touchpad.*` | compositor: button swap, acceleration curve, cursor size; touchpad options need the input layer (the kernel driver reports plain relative motion) |
| Keyboard | `keyboard.input_languages`, `.switch_shortcut`, `.per_window` | layouts belong to build/phonekb – not duplicated here |
| Keyboard | `keyboard.compose_key`, `.on_screen`, `.suggestions`, `.autocorrect` | compose table in the compositor; the on-screen keyboard (`ultar-ime`, build/phonekb) |
| Accessibility | `screen_reader`, `on_screen_keyboard`, `zoom`, `zoom_percent`, `reader_rate`, `large_pointer`, `colour_filter`, `visual_alert`, `mono_audio`, `captions`, `sticky_keys`, `slow_keys`, `bounce_keys`, `mouse_keys`, `dwell_click`, `double_click_ms`, `show_in_bar` | no screen reader or on-screen keyboard program yet; zoom, colour filter and cursor size in the compositor; key and pointer helpers in the compositor's input path; mono audio in audiod |
| Search | `search.source.files`, `.characters`, `.store`, `search.file_places`, `search.history` | the overview search has no file, character or Store source and keeps no history |
| Apps | `apps.default.*`, `apps.removable_media`, `apps.autostart` | the launcher's bus contract has no table of default apps and no autostart list |
| Users | `users.guest` | a guest session needs a credential in authd and a desktop of its own with a temporary home the seat wipes at sign-out. Today one desktop runs for the seat and only the session's owner may unlock it (review security S-1), so a guest entry would land in the owner's desktop and reach their files; accountd/sessiond alone cannot keep a guest away from them |
| Storage | `storage.clean_*`, `.low_space_percent`, `.automount`, `.trim` | a clean-up job; volumed mounts every removable drive of the person in front |
| Updates | `updates.auto_*`, `.window.*`, `.channel`, `.metered`, `.offline_usb`, `.notify` | the update service has its own policy (`update.get_policy`, `/etc/ultar/update.conf`) and does not read the settings |

### SSH

`ultar-sshd` is shipped (`services/entwickler.toml`): a root developer shell that starts only when
`/data/ultar/entwickler/an` exists. `sharing.ssh` is not wired on purpose in this change: switching it
on needs a privileged component that writes that file and starts the service on a setting an
administrator confirmed, and the service it opens is a root shell with input injection. That bridge
is small but security-relevant and wants its own review (who may write the key list, how the switch
is shown on the lock screen). Until then the row is disabled with the note.

## 3. Hidden (hardware absent)

`settingsd` leaves a panel out when the device lacks its hardware (`ultar-profile` panel rules, kept
up to date from `devices.capabilities`); the sidebar hides the area whose page is that panel
(`view/shell.rs` `offered`). While the settings service cannot be read nothing is hidden, so the
page can say why.

| Area | Hidden without |
|---|---|
| Mobile network | a modem |
| Mouse and touchpad | a pointer device |
| Keyboard | a keyboard and a touch screen |
| Colour, Desktop and dock, Windows and workspaces | a display |
| Power: battery rows (charge limit, saver) | a battery (rows, not the area) |

Wi-Fi, Network, Bluetooth and Sound are status pages of their services (not part of this change);
they say when their service or adapter is missing.

## 4. Contracts added for this

| Contract | Where | Who may use it |
|---|---|---|
| `Request::ConfigurePointer { speed, natural_scroll }` (opcode 43) | `ultar-surface/src/proto/request.rs`, `ultar-compositor/src/state/{input,output_config}.rs` | right `ui.output-config` (the display service); an app gets `Permission` |
| `Request::ConfigureKeyRepeat { enabled, delay_ms, per_second }` (opcode 44) | the same, `state/key_repeat.rs` | the same |
| `volumes.usage` → `UsageReply { places: [{id, name, total_bytes, free_bytes}] }` (`internal` first) | `ultar-bus/src/messages/volumes.rs`, `ultar-volumed/src/service.rs` | bus rule `app.settings` |
| Bus rule `app.settings` | `rootfs/etc/ultar/bus-policy.toml` | new calls: `update.check`, `launcher.list`, `launcher.permissions`, `launcher.revoke` (never `launcher.grant`), `volumes.usage`; new topic: `update.progress` |
| `call.emergency` → `DialReply` (no number in the request) | `ultar-bus/src/messages/call_requests.rs`, `ultar-call/src/{emergency,service}.rs` | right `call.emergency` (the lock screen); calld reads `emergency.number` itself (`settings.get`, `settings.changed`) |
| Bus rules for the lock screen, accountd, sessiond, calld | `rootfs/etc/ultar/bus-policy.toml` | `system.lock`: `settings.get`, `call.emergency`, topics `settings.changed`, `call.state`; `system.accountd` and `system.sessiond`: `settings.get`, topic `settings.changed` (sessiond also `notify.post` for the warning); `system.calld`: `settings.get`, topic `settings.changed` |

Adding a wired setting: make its service follow it, add the key to `WIRED`, add the change through
its page and the check of its effect to `tools/settings-e2e/examples/settingstest/wired.rs`, and move
its line from section 2 to section 1.
