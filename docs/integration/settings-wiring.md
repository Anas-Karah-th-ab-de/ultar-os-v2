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
| Mouse and touchpad | `pointer.speed` (−10…10), `pointer.natural_scroll`, `pointer.primary_button`, `pointer.acceleration`, `pointer.size` | compositor | displayd follows `settings.changed` and sends `Request::ConfigurePointer` (again after every reconnect); relative motion × 2^(speed/5), scroll deltas reversed; primary and secondary clicks swapped; a velocity profile (1…3×, measured between the event times) when acceleration is on; the pointer image drawn at 1 / 1.5 / 2× the token `size.icon` | compositor `tests/input_config.rs`, displayd `tests/process.rs` (real settingsd + displayd + compositor), settings-e2e |
| Keyboard | `keyboard.repeat.enabled`, `.delay_ms`, `.per_second` | compositor | displayd sends `Request::ConfigureKeyRepeat`; the compositor thins out the driver's repeats (`state/key_repeat.rs`) | the same three |
| Accessibility | `accessibility.large_text`, `.high_contrast`, `.reduce_motion` | shell (bar, sheets, lock screen) and the Settings window | `Look::with_access`, `Ui::set_reduce_motion`; Settings: `App::follow_access` | desktop `link.rs` tests, settings-e2e |
| Accessibility | `accessibility.large_pointer`, `accessibility.double_click_ms` (200…1000 ms) | compositor; the shell and the Settings window follow the double-click time in their own widgets too | displayd sends both in `Request::ConfigurePointer`: a large pointer forces at least the large cursor size (`pointer.size` never shrinks below it); the interval replaces the 400 ms token in title-bar double clicks and in the toolkit (`Ui::set_double_click_ms`, kept across rebuilds) | the same three, desktop `view/mod.rs` and `link.rs` tests, settings `tests/groups.rs` |
| Accessibility | `accessibility.zoom`, `.zoom_percent` (100…800 %), `.colour_filter` | compositor | displayd sends `Request::ConfigureVisual` (again after every reconnect); the magnifier scales the output around the pointer at `zoom_percent` (100 % = as it is), the colour filter (greyscale, protanopia, deuteranopia, tritanopia, invert) is a final colour matrix over every pixel (`effects.rs`, applied to a copy of the composed frame) | compositor `tests/visual.rs`, displayd `tests/process.rs` (real settingsd + displayd + compositor), settings-e2e |
| Accessibility | `accessibility.visual_alert`, `.show_in_bar` | shell | a banner that appears while `visual_alert` is on owes a short screen flash (`Request::VisualAlert`; the compositor draws it over every window, lock screen included, and it goes by itself); `show_in_bar` keeps an accessibility button in the quick settings footer that opens Settings > Accessibility | desktop `model/notes.rs`, `view/quick.rs`, `link.rs` tests, settings-e2e |
| Search | `search.source.apps`, `.settings`, `.calculator`, `.commands`, `.characters`, `search.history`, `search.arabic_variants` | the shell's overview search | `model/search.rs` `Sources`, read at start and on `settings.changed`; characters from the shell's own table (`model/characters.rs`, ar+en names), choosing one inserts it; history kept beside the shell per person (`model/history.rs`, bounded, off deletes it), matching entries offered and run again | desktop `model/search.rs`, `model/characters.rs`, `model/history.rs`, `link.rs` tests, settings-e2e |
| Storage | "Space used": this device and every mounted drive, free of total with a bar | volume service | new call `volumes.usage` (statvfs of the data partition and of each mount of the person in front) | volumed unit + `tests/process.rs`, settings `tests/groups.rs`, settings-e2e (real volumed; `tools/settings-e2e`) |
| Apps | installed apps with their declared permissions; switching one off takes it back | launcher | `launcher.list`, `launcher.permissions`, `launcher.revoke` (granting stays with the system dialog) | settings `tests/groups.rs` (fake service) |
| Users | add, remove, change password | account service | `account.users_list`, `account.user_create`, `account.user_remove`, `account.user_password`; the account service asks for the administrator in the system dialog | settings `tests/groups.rs` (fake service) |
| Updates | "Check now", "Update from a drive", state line | update service | `update.check`, `update.offline`, `update.progress` | settings `tests/groups.rs` (fake service) |
| Updates | `updates.auto_system`, `.window.from`, `.window.until`, `.channel`, `.metered`, `.offline_usb`, `.notify` | update service | reads them at start and on `settings.changed` and maps them onto its policy under the ceiling of `/etc/ultar/update.conf`; `notify` posts through notifyd, `offline_usb` offers a bundle from a plugged-in drive | ultar-update `tests/settings_follow.rs` (real bus), settings-e2e |

Limits that are part of the truth:

- Key repeat is thinned out, not generated: the delay cannot be shorter and the rate not higher than
  what the keyboard driver reports (usually 250 ms and about 30 per second). The page offers 150–1000 ms
  and 5–60 per second; values beyond the driver's are capped by it until the input layer can set the
  driver's own repeat. A driver that reports no event times is passed through unchanged.
- Pointer acceleration measures the velocity between two motion events (device units per millisecond):
  up to 3/ms the motion passes as reported, at 30/ms and above it travels three times as far, linearly
  in between (`state/accel.rs`). A driver that reports no usable event times is passed through unchanged,
  as with the key repeat. The kernel's PS/2 driver applies its own acceleration on top until it reports
  raw deltas.
- The double-click time reaches the compositor (every title bar) and, through `Look`/`follow_access`,
  the shell's windows and the Settings window. Other apps follow once they read the settings (app bus
  access, branch build/appbus); until then they keep the toolkit's 400 ms.
- Large text, high contrast and reduced motion reach the shell, the lock screen and the Settings
  window. Other apps follow once they read the settings (app bus access, branch build/appbus).
- The Apps and Users sections are tested against fake services only: the kernel run
  (settings-e2e) starts no launcher or account service. The Updates page's actions (check,
  offline) use a fake update service; its wired settings are checked with the update service's
  own code over the real bus (as the display service's are) and in ultar-update
  `tests/settings_follow.rs`.
- The update settings are the one source the person sees: at start and on every change they
  overwrite the mapped policy fields (`automatic`, `window`, `channel`, `allow_metered`).
  `/etc/ultar/update.conf` is the administrator's ceiling (`allow_automatic`, `allow_metered`,
  `channels`): a setting cannot allow what the file forbids; a forbidden channel falls back to
  the first allowed one. The rate limit and `uupdate policy set` remain for what the page does
  not offer. The window is UTC, as the policy and the system clock are (the time zone setting
  is not wired yet). `updates.notify` also gates the drive offer (it is a notification).

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
| Emergency | all (`emergency.*`) | lock screen and phone do not read them yet |
| Mouse and touchpad | `pointer.touchpad.tap_to_click`, `.two_finger_scroll`, `.natural_scroll`, `.disable_while_typing`, `.gestures` | an input layer that tells touchpads apart: the kernel's PS/2 driver (`kernel/ultar-kern/src/drivers/ps2maus.rs`) reports plain relative motion, and the evdev classifier (`ultar-hal-gfx/src/evdev_devices.rs`, `classify`) reports every relative device and every absolute device with buttons as `Mouse` – there are no tap, finger or gesture events to follow, so no touchpad scroll is distinguishable from a wheel either |
| Keyboard | `keyboard.input_languages`, `.switch_shortcut`, `.per_window` | layouts belong to build/phonekb – not duplicated here |
| Keyboard | `keyboard.compose_key`, `.on_screen`, `.suggestions`, `.autocorrect` | compose table in the compositor; the on-screen keyboard (`ultar-ime`, build/phonekb) |
| Accessibility | `screen_reader`, `on_screen_keyboard`, `reader_rate`, `captions`, `sticky_keys`, `slow_keys`, `bounce_keys`, `mouse_keys`, `dwell_click` | no screen reader or on-screen keyboard program yet; key and pointer helpers in the compositor's input path |
| Accessibility | `mono_audio` | audiod only sets the sound cards' volume, mute and output (`CardControls`); no shipped component mixes PCM samples before output – the HAL `Audio`/`AudioStream` traits have no service implementation, and `ultar-audio` is the A30 device CLI with no bus – so there is no sample path in which to sum the channels to mono. Wiring needs a mixer in the output path that reads the setting |
| Search | `search.source.files`, `search.file_places` | no bus service may read the person's home: it is `0700`, owned by the person, while the shell (uid 816) and the volume service (828) run as their own users, and no file-search service exists. Wiring needs a service that searches the person's own places (the home folders named by `search.file_places`) and answers the shell – bounded in count and time, no root access for the caller, nothing outside the person's places |
| Search | `search.source.store` | the catalogue parser lives in `apps/ultar-store`; the platform shell may not depend on the apps area (`tools/check-layers.toml`), and the parser must be reused, not duplicated. Wiring needs the catalogue data and parser extracted into a shared `system/core` crate that both the Store and the shell use |
| Apps | `apps.default.*`, `apps.removable_media`, `apps.autostart` | the launcher's bus contract has no table of default apps and no autostart list |
| Users | `users.list_on_login`, `.guest`, `.auto_login`, `.new_kind`, `.parental*` | the lock screen and the account service do not read them (automatic sign-in is `account.user_auto_login` per user; not on the page yet) |
| Storage | `storage.clean_*`, `.low_space_percent`, `.automount`, `.trim` | a clean-up job; volumed mounts every removable drive of the person in front |
| Updates | `updates.auto_apps` | app updates belong to the launcher and the Store; the system updater installs only system images (kernel/system/boot roles) and the launcher has no automatic app-update path |

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
| `Request::ConfigurePointer { speed, natural_scroll, swap_buttons, acceleration, size, double_click_ms }` (opcode 44; the old two-field body still decodes, the added fields default to the schema) | `ultar-surface/src/proto/{request,request_decode,input_config}.rs`, `ultar-compositor/src/state/{input,output_config,accel,grabs}.rs` | right `ui.output-config` (the display service); an app gets `Permission` |
| `Request::ConfigureKeyRepeat { enabled, delay_ms, per_second }` (opcode 45) | the same, `state/key_repeat.rs` | the same |
| `Request::ConfigureVisual { zoom, zoom_percent, filter }` (opcode 56; a gap after 45 is left for the key-helper package) | `ultar-surface/src/proto/{request,request_decode,visual}.rs`, `ultar-compositor/src/{effects.rs,state/visual.rs}` | right `ui.output-config` (the display service) |
| `Request::VisualAlert { ms }` (opcode 57) | the same, `ultar-compositor/src/state/visual.rs` | right `ui.shell` (the shell) |
| `volumes.usage` → `UsageReply { places: [{id, name, total_bytes, free_bytes}] }` (`internal` first) | `ultar-bus/src/messages/volumes.rs`, `ultar-volumed/src/service.rs` | bus rule `app.settings` |
| Bus rule `app.settings` | `rootfs/etc/ultar/bus-policy.toml` | new calls: `update.check`, `launcher.list`, `launcher.permissions`, `launcher.revoke` (never `launcher.grant`), `volumes.usage`; new topic: `update.progress` |
| Bus rule `system.updated` | `rootfs/etc/ultar/bus-policy.toml`, `services/bus.toml` | `settings.get` + `settings.changed` (the `updates.*` policy), `notify.post` (the notifications), `volumes.state` (the drive offer); own root-only socket `updated.sock` |

Adding a wired setting: make its service follow it, add the key to `WIRED`, add the change through
its page and the check of its effect to `tools/settings-e2e/examples/settingstest/wired.rs`, and move
its line from section 2 to section 1.
