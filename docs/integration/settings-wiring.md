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
| Accessibility | `accessibility.mono_audio` | call service + Ultar kernel | calld (it holds the right `sound` for the call streams already, no new right) reads it at start and on `settings.changed` and writes `1`/`0` to the kernel's switch `/dev/ultar/pcm-mono` (right `sound`, not exclusive; written again when the card appears later); while it is on, the kernel sums left and right of every frame into both before they reach the card (`svcdev/pcm.rs` `downmix`) – for every writer of `/dev/ultar/pcm` (browser, calls). On a Linux kernel there is no such node and nothing changes | calld `mono.rs`, `tests/service.rs` (real bus, runtime change), kernel `tools/kern-test/pcm-e2e.sh` (QEMU: tone on the left only, the card plays it on the right too) |
| Accessibility | `accessibility.visual_alert`, `.show_in_bar` | shell | a banner that appears while `visual_alert` is on owes a short screen flash (`Request::VisualAlert`; the compositor draws it over every window, lock screen included, and it goes by itself); `show_in_bar` keeps an accessibility button in the quick settings footer that opens Settings > Accessibility | desktop `model/notes.rs`, `view/quick.rs`, `link.rs` tests, settings-e2e |
| Accessibility | `accessibility.sticky_keys`, `.slow_keys`, `.bounce_keys`, `.mouse_keys` | compositor | displayd sends `Request::ConfigureKeyHelpers` (again after every reconnect); the key input path (`state/key_helpers.rs`) latches a modifier pressed and released alone for the next key (twice locks it), holds a press back until it has been held 300 ms, drops a re-press of the same key within 300 ms, and turns the numeric keypad into pointer motion (20 points a step) and clicks | compositor `tests/key_helpers.rs`, displayd `tests/process.rs` (real settingsd + displayd + compositor), settings-e2e |
| Keyboard | `keyboard.compose_key` (`none`, `alt_gr`, `menu`, `caps_lock`) | compositor | displayd sends it in `Request::ConfigureKeyHelpers`; the chosen key starts a two-key sequence that the compose table (`state/compose_table.rs`, data: common Latin accents and symbols, Arabic hamza carriers, madda and the rial sign) turns into text delivered to the keyboard's target; as Caps Lock it no longer toggles caps | the same three |
| Search | `search.source.apps`, `.settings`, `.calculator`, `.commands`, `.characters`, `search.history`, `search.arabic_variants` | the shell's overview search | `model/search.rs` `Sources`, read at start and on `settings.changed`; characters from the shell's own table (`model/characters.rs`, ar+en names), choosing one inserts it; history kept beside the shell per person (`model/history.rs`, bounded, off deletes it), matching entries offered and run again | desktop `model/search.rs`, `model/characters.rs`, `model/history.rs`, `link.rs` tests, settings-e2e |
| Search | `search.source.store` | the shell's overview search, the Store, the launcher | the Store publishes its verified catalogue to its publish directory (launcher `publishers`, `/publish` in its cell); the shell reads it at start and when the switch changes (`model/store.rs`), checks the signature (fail closed) and offers matching apps with the deep link `store://app/<id>` | desktop `model/store.rs`/`link.rs`, ultar-catalog `source.rs` (plain files only), launcher `datadir.rs`/`config.rs`/`cell/tests.rs`, store `online_tests.rs`, settings-e2e |
| Storage | "Space used": this device and every mounted drive, free of total with a bar | volume service | new call `volumes.usage` (statvfs of the data partition and of each mount of the person in front) | volumed unit + `tests/process.rs`, settings `tests/groups.rs`, settings-e2e (real volumed; `tools/settings-e2e`) |
| Storage | `storage.low_space_percent`, `.automount` | volume service | volumed reads them at start and follows `settings.changed`: it warns once (through notifyd) when the free space of the data partition drops below the percentage and re-arms only after it rose above it; with automount off a drive is offered with a "Mount" button instead of mounted | volumed unit + `tests/process.rs` (real daemon, `settings.changed`), settings-e2e |
| Storage | `storage.clean_downloads`, `.clean_downloads_days` | launcher | the launcher owns the person's file areas (userfiles group), so it – not the volume service, which handles untrusted USB file systems – reads them at start and follows `settings.changed`: a daily clean-up removes the person's regular files older than N days in the downloads folder (only that folder, only their files, never through a link) | launcher unit + `tests/clean.rs` (follow loop on the bus), settings-e2e |
| Apps | installed apps with their declared permissions; switching one off takes it back | launcher | `launcher.list`, `launcher.permissions`, `launcher.revoke` (granting stays with the system dialog) | settings `tests/groups.rs` (fake service) |
| Apps | `apps.default.web`, `.mail`, `.calendar`, `.music`, `.video`, `.photos`, `.documents` | launcher; Files opens files through it | the launcher reads them at start and on `settings.changed`; `launcher.resolve` turns a URI into the installed app of its kind that declares the URI's scheme (an invalid value falls back to the system default); `launcher.candidates` lists the installed apps that declare a kind, which the page offers as the choices; Files asks `resolve` and starts that app | launcher `tests/defaults.rs`, files `tests/clicks.rs`, settings `tests/apps_page.rs` (fake service), settings-e2e |
| Apps | `apps.removable_media` (ask / open_files / nothing) | shell | the shell reads it at start and on `settings.changed`; a drive that appears in `volumes.state` asks (a notice with an Open button), opens in Files, or does nothing (a first report after start is no insertion) | desktop `shell/media.rs` and `link.rs` tests, settings-e2e |
| Apps | `apps.autostart` | launcher | the launcher reads it at start and on `settings.changed`, follows `session.state` and starts the listed installed apps (their window) once after a sign-in | launcher `tests/defaults.rs`, settings-e2e |
| Users | add, remove, change password | account service | `account.users_list`, `account.user_create`, `account.user_remove`, `account.user_password`; the account service asks for the administrator in the system dialog | settings `tests/groups.rs` (fake service) |
| Users | `users.list_on_login` | lock screen | read at start and on `settings.changed`; off: the sign-in shows only the session user's name field, no account cards (`lock/prefs.rs`) | desktop `lock/prefs.rs`, `lock/mod.rs` tests, settings-e2e |
| Users | `users.auto_login` | account service + lock screen | accountd keeps the per-user flag (`account.user_auto_login`) in sync with the device setting; at a fresh start (nobody signed in) the lock screen signs the named user in without a password – never into a locked session of somebody else, never outside the parental window | accountd `settings.rs`, `core_tests.rs`, `tests/process.rs`; desktop `app/lock_follow.rs`; settings-e2e |
| Users | `users.new_kind` | the Settings app's Users page | the add-user dialog says the configured kind and creates that kind; an invalid value means standard | settings `tests/users_kind.rs`, settings-e2e |
| Users | `users.parental`, `.parental.from`, `.parental.until` | lock screen + session service | standard users sign in only in the window (the lock screen refuses with the hours, sessiond refuses too); sessiond warns five minutes before the end (`notify.post`) and signs them out at it, locking the screen first when one of them is in front; administrators are never restricted; a window over midnight works | desktop `lock/prefs.rs`, `lock/mod.rs`; sessiond `parental.rs`, `tests/process.rs`; settings-e2e |
| Emergency | `emergency.on_lock_screen`, `.contact_name`, `.contact_phone`, `.medical` | lock screen | the cover's Emergency button opens a panel showing exactly the stored contact and medical details – nothing else – without unlocking; off: the button is disabled with the reason | desktop `lock/emergency.rs`, `lock/view.rs`; settings-e2e |
| Emergency | `emergency.number` | call service + lock screen | `call.emergency` carries no number: calld reads and validates it itself (digits, optional `+`, 3–16; invalid → 112) and dials it on the mobile network (slot 1, else 2); the lock screen shows the outcome | calld `emergency.rs`, `tests/service.rs`; settings-e2e |
| Emergency | `emergency.sos_light` | lock screen | while the emergency call is placed or active, the screen flashes SOS (··· −−− ···, 200 ms unit) – the screen is the light the lock screen owns; the flash ends when `call.state` no longer lists the call | desktop `lock/emergency.rs` tests, settings-e2e |
| Updates | "Check now", "Update from a drive", state line | update service | `update.check`, `update.offline`, `update.progress` | settings `tests/groups.rs` (fake service) |
| Updates | `updates.auto_system`, `.window.from`, `.window.until`, `.channel`, `.metered`, `.offline_usb`, `.notify` | update service | reads them at start and on `settings.changed` and maps them onto its policy under the ceiling of `/etc/ultar/update.conf`; `notify` posts through notifyd, `offline_usb` offers a bundle from a plugged-in drive | ultar-update `tests/settings_follow.rs` (real bus), settings-e2e |
| Updates | `updates.auto_apps` | the Store | `apps/ultar-store` follows it (and the shared `updates.metered`, `updates.window.from`/`.until`) at start and on `settings.changed` (`src/settings.rs`, `src/follow.rs`); installs available app updates by itself under the same window and metered rules as the system updater — off = only offer updates; `net.state` gives the metered flag | store `settings.rs` + `tests/transitions2.rs`, settings-e2e |
| Assistant | `assistant.model` (small/medium/large) | llm-bus | reads it at start and on `settings.changed`; the largest installed model of the chosen class answers (the `model` field of the completion) and `llm.state` reports it; a class with no installed model leaves the model service's own pick | llm-bus `choice.rs`/`backend.rs` tests, `tests/process.rs` (real bus + stand-in model socket), settings-e2e |
| Assistant | `assistant.model_place` | llm-bus, model service `ultar-llm` | llm-bus reads it at start and on `settings.changed`, accepts only a plain absolute path below `/data/ultar/models`, `/data/ultar/agent` or `/run/ultar/media` (no `.`/`..`, no control characters, ≤ 256 bytes) and writes it whole (write + rename) to `/run/ultar/llmbus/model-place` (its own directory, mode 0755); any other value empties the file. The model service reads the file with every model search – so a new place counts without a restart – checks it again and also after resolving links, and adds the `.gguf` files there to its own model folders | llm-bus `place.rs` tests, ultar-llm `place.rs` tests (links out of the roots refused), settings-e2e |
| Assistant | `assistant.check_links`, `assistant.see_screen` | agentd | `check_links` appends the catalogue warning (`agent.link_warning`) to the model's answer when a link in it looks suspicious – a local check, no request goes out (scheme other than http/https, user-info part, a host that is an address as numbers, punycode `xn--`); `see_screen` opens the tool `see_screen`: the person consents in the system dialog first, then the capture service saves a screenshot and the helper says where | agentd `links.rs`/`session.rs`/`tools.rs` tests, `tests/reach.rs` (real bus, runtime change), settings-e2e |
| Keyboard | `keyboard.switch_shortcut`, `keyboard.per_window` | compositor | displayd sends `Request::ConfigureKeyboard` (at start, on `settings.changed` and after every reconnect); only the chosen combination switches the layout; with `per_window` every window remembers its own layout and gets it back on focus (off forgets; a new layout list keeps the languages that are still in it) | compositor `tests/input_config.rs`, displayd `tests/process.rs` (real settingsd + displayd + compositor), settings-e2e |
| Keyboard, Accessibility | `keyboard.on_screen` (automatic / always / never), `keyboard.suggestions`, `keyboard.autocorrect`, `accessibility.on_screen_keyboard` | the on-screen keyboard `ultar-osk` (own user 839, compositor right `ui.input-method`) | reads them at start and on `settings.changed`, and `devices.capabilities` (a hardware keyboard); every app tells the compositor when its focus enters or leaves a text field (`Request::TextInput`, sent by `ultar-surface`'s `UiWindow` and the desktop's host), the compositor tells the keyboard (`ImeState`); the keyboard shows its *input panel* (automatic: only without a hardware keyboard; the accessibility switch: always) with the layouts of `ultar-ime` (Arabic, English, German, symbol pages, PIN/phone pads), types with `ImeCommit` and `ImeKey` (Backspace, Enter), offers up to three words from small per-language lists (`assets/osk-words-*.txt`; Arabic without hamza/taa marbuta differences) and, with autocorrect, replaces a word one typo away from a known one on space; never in password or PIN fields, no learning, nothing stored | desktop `osk/{mod,words,view}.rs` tests, compositor `tests/ime_keyboard.rs`, settings-e2e |
| Keyboard | `keyboard.input_languages` | the shell (desktop); the sign-in reads the same list | the shell reads it at start and on `settings.changed` and gives the list to the compositor (`SetKeyboardLayouts`); when it names any, it wins over `region.keyboard.layouts` (empty: the region's list, as before); the active one stays `region.keyboard.active`; the sign-in reads the list through `login.access` at start | desktop `app/keyboard.rs` tests, settingsd `tests/accessibility.rs`, settings-e2e |

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
- Slow and bounce keys are switches in the schema, so their interval is fixed at 300 ms; mouse keys
  move the pointer 20 logical points per keypad step. Slow keys hold a press back on the compositor's
  own clock (a release before 300 ms cancels it), so they work whatever the driver's repeat does; a
  driver that reports no event times passes bounce keys through unchanged, as with the key repeat.
  The compose table is two-key sequences matched in order; a sequence it does not know produces
  nothing. Sticky keys apply to shortcuts and to the character delivered, not to the shell's own
  Super shortcuts or the layout-switch chord (those read the physically held modifiers).
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
- The Store source reads the Store's **published** catalogue cache, not its private one: the
  Store's own cache lives in its data area (`ULTAR_STORE_DATA/catalog-cache`, `0700`, the app's
  user), which the shell (its own user) cannot read. The launcher gives the apps named in
  `launcher.toml` `publishers` (only the Store, id `store`, a system app, only in a cell) a publish
  directory `/data/ultar/publish/<id>` (`0755`, owned by the app; the root is the launcher's
  `0755` directory), bound writable but never executable at `/publish` in the cell
  (`ULTAR_PUBLISH`); uninstalling removes it. The Store writes the verified bytes there (write +
  rename), the shell reads `/data/ultar/publish/store` and verifies the signature again
  before a byte is parsed; it reads only plain files (no link followed, no pipe or device: it
  could block the reader), each size-capped. The switch always gates what is offered; no network
  is involved on either side, and an unsigned or tampered cache offers nothing.
- Automatic app updates (`updates.auto_apps`) run while the Store window is open (the Store is an
  app, not a background service): the follower reads the settings and `net.state`, and the window
  loop installs available updates when the window and the connection allow it. A metered connection
  blocks them unless `updates.metered` is on; an unknown connection is treated as metered.
- `assistant.model` chooses among the installed models by file size (the same classes as
  `llm.state`); it cannot install a model. A class with none installed leaves the model
  service's own pick, and the state still tells what is really there.
- `assistant.see_screen`: the consent is asked for every capture (`sysdialog.authenticate`,
  the trusted system dialog). The picture is saved in the person's `Pictures` folder by the
  capture service; the image itself does not reach the model – the bus completion contract
  carries text only – so the helper tells where the picture is. The tool is not even named to
  the model while the switch is off.
- `assistant.check_links` warns, it does not block: the model's words stay verbatim, the
  warning is a translated catalogue text appended to them. The check is heuristic and local
  (no link is fetched).
- `keyboard.switch_shortcut`: an unconfigured compositor keeps all three combinations
  (Super+Space, Ctrl+Shift, Alt+Shift); the display service imposes the schema's default
  (Super+Space) when it connects, as with the key repeat. The sign-in reads the input
  languages at start (`login.access`); the session shell follows them at runtime.
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
| Assistant | `assistant.voice`, `.wake_word`, `.wake_phrase` | no speech component exists – `ultar-sprache` translates command-line text, there is no speech synthesis or recognition service |
| Cells and containers | all (`cells.*`) | `ultar-zellen` has no bus contract and no shipped service definition |
| Emergency | `emergency.power_presses` | the power key never reaches a system handler on this branch: the seat's input path (hal-gfx evdev → displayd → compositor) does not report it to a service, and powerd (user 819) may not open the seat's input nodes – that would let it read every keystroke. Wiring needs the compositor to report the key to powerd (a new surface request and a narrow `power.key` call), outside this package |
| Emergency | `emergency.key_escape` | holding both volume keys must close the app, then the session, then restart – the three stages belong to the compositor/shell input path, which does not report the volume keys as a pair yet (outside this package) |
| Mouse and touchpad | `pointer.touchpad.tap_to_click`, `.two_finger_scroll`, `.natural_scroll`, `.disable_while_typing`, `.gestures` | an input layer that tells touchpads apart: the kernel's PS/2 driver (`kernel/ultar-kern/src/drivers/ps2maus.rs`) reports plain relative motion, and the evdev classifier (`ultar-hal-gfx/src/evdev_devices.rs`, `classify`) reports every relative device and every absolute device with buttons as `Mouse` – there are no tap, finger or gesture events to follow, so no touchpad scroll is distinguishable from a wheel either |
| Accessibility | `screen_reader`, `reader_rate`, `captions`, `dwell_click` | no screen reader program yet; dwell click in the compositor's pointer path |
| Search | `search.source.files`, `search.file_places` | a file-search service that runs **as the person in front** and searches that person's own places (the home folders named by `search.file_places`, empty = the home), answering the shell – bounded in count and time, no root for the caller, nothing outside the person's places. The home is `0700`, owned by the person, so only a process with that person's id can read it, and **the session layer cannot start a service as that person**: `ultar-sessiond` only records who is signed in (it starts no programs and holds no kernel rights), `ultar-init` starts each service only as the fixed `user` of its service file (`launch.rs`, `spec.rs` – a person's id 1000–19999 is given out at run time and cannot be named there), and the launcher runs apps only as their own app users (20000–49999), never as a person. Starting a per-person search service would need a new privileged spawn path (root or `set_id`) that this change must not add; the shared file areas (`/data/ultar/areas/*`, `0770 launcher:userfiles`) are not the person's home and reach every person's files, so they are not a substitute. Until the session layer starts per-person services, both keys stay unwired |
| Users | `users.guest` | a guest session needs a credential in authd and a desktop of its own with a temporary home the seat wipes at sign-out. Today one desktop runs for the seat and only the session's owner may unlock it (review security S-1), so a guest entry would land in the owner's desktop and reach their files; accountd/sessiond alone cannot keep a guest away from them |
| Storage | `storage.trim`, `storage.clean_old_updates` | trim: the Ultar kernel has no FITRIM (its ioctl path answers ENOTTY and its ext4/FAT drivers have no discard), and on Linux FITRIM needs CAP_SYS_ADMIN, which the unprivileged volume service must not be given; clean_old_updates: `ultar-update` keeps no superseded images – the A/B slots are the running and the fallback system (never to delete) and the one staged bundle is purged by the updater itself – and it offers no clean-up call |

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
| `Request::ConfigureKeyHelpers { sticky_keys, slow_keys, bounce_keys, mouse_keys, compose_key }` (opcode 46) | `ultar-surface/src/proto/{request,request_decode,key_helpers}.rs`, `ultar-compositor/src/state/{key_helpers,sticky_keys,slow_keys,bounce_keys,mouse_keys,compose,compose_table}.rs` | right `ui.output-config` (the display service); an app gets `Permission` |
| `Request::ConfigureKeyboard { switch_shortcut, per_window }` (opcode 47; `SwitchShortcut` = super_space / alt_shift / ctrl_shift) | `ultar-surface/src/proto/{request,request_decode,input_config}.rs`, `ultar-compositor/src/state/{keys,windows,output_config,shell}.rs` | right `ui.output-config` (the display service); an app gets `Permission` |
| `Request::ImeKey { key }` (opcode 48; `ImeKey` = Backspace / Enter) | `ultar-surface/src/proto/{request,request_decode,ime}.rs`, `ultar-compositor/src/state/text.rs` | right `ui.input-method` while its input panel is shown; the focused field gets a press and a release (serial 0); `ImeState` now goes to every client with that right, so the keyboard hears about a field before it shows its panel |
| Bus rule `system.osk` | `rootfs/etc/ultar/bus-policy.toml`, `services/osk.toml`, `ids.toml` (839), `compositor-rights.toml` | `settings.get`; topics `settings.changed`, `devices.capabilities`; nothing else |
| `volumes.usage` → `UsageReply { places: [{id, name, total_bytes, free_bytes}] }` (`internal` first) | `ultar-bus/src/messages/volumes.rs`, `ultar-volumed/src/service.rs` | bus rule `app.settings` |
| `launcher.resolve {uri}` → `{app: Option<id>}`; `launcher.candidates {kind}` → `{apps: [id]}`; `Kind` and `kind_of_uri` | `ultar-bus/src/messages/launcher.rs`, `ultar-launcher/src/{defaults,follow}.rs` | bus rules `app.files` (resolve), `app.settings` (candidates); the launcher reads `settings.get` and follows `settings.changed`, `session.state` |
| Bus rule `app.settings` | `rootfs/etc/ultar/bus-policy.toml` | new calls: `update.check`, `launcher.list`, `launcher.permissions`, `launcher.revoke` (never `launcher.grant`), `launcher.candidates`, `volumes.usage`; new topic: `update.progress` |
| Bus rule `system.updated` | `rootfs/etc/ultar/bus-policy.toml`, `services/bus.toml` | `settings.get` + `settings.changed` (the `updates.*` policy), `notify.post` (the notifications), `volumes.state` (the drive offer); own root-only socket `updated.sock` |
| Bus rule `app.store` (extended) | `rootfs/etc/ultar/bus-policy.toml` | new call `settings.get`; new topics `settings.changed` (the `updates.*` app-update rules) and `net.state` (the metered flag) |
| `store://app/<id>[?action=remove]` deep link | `apps/ultar-store/ultar-app.toml` (`[open] schemes = ["store"]`), `src/deeplink.rs` | the shell (`launcher.start` v2 with the URI); the launcher hands it to a fresh Store as `ULTAR_OPEN_URI` |
| Shared catalogue crate `ultar-catalog` | `system/core/ultar-catalog` (model, parser, signature check, `FileCatalogSource`) | the Store (`apps/ultar-store`) and the shell (`platform/ultar-desktop`); layer rule `apps -> system/core/ultar-catalog` in `tools/check-layers.toml` |
| `call.emergency` → `DialReply` (no number in the request) | `ultar-bus/src/messages/call_requests.rs`, `ultar-call/src/{emergency,service}.rs` | right `call.emergency` (the lock screen); calld reads `emergency.number` itself (`settings.get`, `settings.changed`) |
| Bus rules for the lock screen, accountd, sessiond, calld | `rootfs/etc/ultar/bus-policy.toml` | `system.lock`: `settings.get`, `call.emergency`, topics `settings.changed`, `call.state`; `system.accountd` and `system.sessiond`: `settings.get`, topic `settings.changed` (sessiond also `notify.post` for the warning); `system.calld`: `settings.get`, topic `settings.changed` |

Adding a wired setting: make its service follow it, add the key to `WIRED`, add the change through
its page and the check of its effect to `tools/settings-e2e/examples/settingstest/wired.rs`, and move
its line from section 2 to section 1.
