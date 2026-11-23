# Icon table – proposal for DESKTOP-SPEC section 4 (round 2)

Source: `icons.py` (drawings), `icons.json` (name, meaning, mirrored in RTL, where used) – both written by the generator.
Rules kept: one set, stroke 1.5 at 24, two sizes (16, 24), outline. One icon = one meaning, one meaning = one icon.
"Mirrored in RTL" is a property of the icon; the generator mirrors at draw time from the direction of the board (`ic()` in `gen.py`), never in a call. The check `icon mirroring by direction` fails on any other case.

## Double meanings of round 1 and how they are resolved

| Round-1 finding | Was | Now |
|---|---|---|
| D46 `grid` | all apps + settings area Apps + Store Installed + Android | `grid` = all apps only · `apps` = settings area · `installed` = Store · `android` = component |
| D47 `shield` | security + VPN + containers + tracking protection | `shield` = security only · `vpn` = shield with key · `server` = containers and server · `block` = ad and tracker blocking |
| D48 `update` | reload + updates | `reload` = one circular arrow · `update` = arrow up in a circle |
| D49 `star` | Discover + main display | `compass` = Discover · main display = text badge with `check` (no icon of its own); `star` stays reserved for "favourite" and is not drawn yet |
| D50 `file` | text editor + Documents + .docx + .xlsx + Office | `editor` (app) · `place-documents` · `kind-text` · `kind-sheet` · `office` (component) |
| D51 `box` | Linux programs + Store section | `package` = Linux programs only; the Store section is gone (it is part of Discover) · `package-file` = install from a file |
| D52 `window` | settings area + Windows programs | `workspaces` and `dock` = the two settings areas · `windows` = component |
| D53 `globe` | Ultar Browser + Firefox | `browser` · `firefox` |
| D54 `chev` | back/forward + select arrow (rotated) | `back`, `forward` (mirrored by direction) · `chevron-down` = select, own drawing, no rotation |
| D55 `bell` | notifications + do not disturb | `bell` · `bell-off` (crossed) |
| D56 `lock` | locked + secure connection | `lock` · `secure` (lock with check) |
| D57 `trash` | delete + place Trash | `trash` = delete (action) · `bin` = place Trash (bar, dock, Files) |
| D58 `download`, `music`, `image`, `usb` | places and actions | `download` = action · `place-downloads`, `place-music`, `place-pictures` = places · `media` = component · `kind-image` = file kind · `usb` = removable drive (one meaning: the drive, wherever it is shown) |
| section 4 itself: moon = night light **and** dark | two meanings | `moon` = night light · `contrast` = dark theme |
| section 4 itself: person = account, cloud = Ultar account | overlap | `user` = account/person (sign-in, online accounts) · `cloud` = Ultar account and its services · `users` = users of this device |

## Decisions on mirroring (R10–R15)

- `back`/`forward`: mirrored; in Arabic "back" points right.
- `volume`: mirrored (waves point in the direction the slider grows).
- `kind-text`, `kind-sheet`, `place-documents`, `spark`, `message`, `copy`, `backspace`, `battery`, `cell`, call arrows, `language`, `backup`, `reload`, `sharing`, `users`: mirrored (they have a reading direction).
- Display-mode pictograms: physical picture of two screens, **never** mirrored; the arrow of round 1 is gone (extend = two outlines joined by a line).
- Media transport, clock-like and symmetric icons, brand mark: never mirrored.

## Table

### Navigation

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `back` | back (towards the start side) | yes | Account, CalendarSheet, DesktopBottom, DesktopTop, Main |
| `forward` | forward / open (towards the end side) | yes | CalendarSheet, DesktopBottom, DesktopTop, Main, QuickSettings, SettingsDisplay, SettingsDisplayLight, States |
| `chevron-down` | opens a list of choices (select) | no | InstallerDisk, InstallerDrivers, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, States |
| `home` | home folder | no | DesktopTop, Main |
| `grid` | all apps | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |
| `search` | search | no | Account, CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store, SurfaceStates |
| `more` | more actions for this element | no | PhoneDial |

### Window

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `min` | minimise window | no | Account, DesktopBottom, DesktopTop, Main, PhoneDial, PhoneInCall, PhoneIncoming, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |
| `max` | maximise window | no | Account, DesktopBottom, DesktopTop, Main, PhoneDial, PhoneInCall, PhoneIncoming, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |
| `close` | close window or dialog | no | Account, CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, PhoneDial, PhoneInCall, PhoneIncoming, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |

### Actions

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `plus` | new / add | no | DesktopBottom, DesktopTop, Main, Overview |
| `trash` | delete (action) | no | Account |
| `copy` | copy | yes | Account, PhoneDial, PhoneInCall, PhoneIncoming |
| `download` | download (action, download list) | no | DesktopBottom, InstallerDrivers, SettingsDrivers |
| `reload` | reload | yes | DesktopBottom |
| `eject` | eject a removable drive | no | DesktopTop, Main |
| `backspace` | delete the last digit | yes | PhoneDial |

### State

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `check` | done / selected | no | Account, DesktopBottom, Installer, InstallerDisk, InstallerDrivers, InstallerSummary, Login, Overview, PhoneDial, Projection, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, States, Store, SurfaceStates |
| `info` | hint | no | Installer, InstallerDisk, States, Store, SurfaceStates |
| `warning` | warning | no | InstallerDrivers, SettingsDrivers, SurfaceStates |
| `error` | error | no | Account, InstallerDisk, InstallerDrivers, InstallerSummary, SettingsDrivers, States, SurfaceStates |
| `lock` | locked / lock the screen | no | InstallerDrivers, Lock, QuickSettings, SettingsDrivers |
| `secure` | encrypted connection or call | no | PhoneInCall, PhoneIncoming |
| `shield` | security | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `eye` | show (password visible) | no | Account, InstallerDisk, Login |
| `ring` | in progress (progress ring) | no | States, SurfaceStates |
| `block` | ad and tracker blocking | no | DesktopBottom |

### Connection

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `wifi` | Wi-Fi (strength) | no | Account, CalendarSheet, DesktopBottom, DesktopTop, InstallerDrivers, Lock, Main, Overview, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, States |
| `offline` | no connection (offline) | no | Account, SurfaceStates |
| `cable` | wired network | no | InstallerDrivers, QuickSettings, SettingsDrivers |
| `cell` | mobile network (signal bars) | yes | PhoneDial, PhoneInCall |
| `plane` | airplane mode | no | QuickSettings |
| `bluetooth` | Bluetooth | no | Account, InstallerDrivers, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `vpn` | VPN (shield with key) | no | QuickSettings |
| `mesh` | Mesh (linked devices) | no | Account, Installer, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |
| `cloud` | Ultar account / cloud | no | Account, PhoneDial, PhoneIncoming |
| `network` | network (settings area) | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `sharing` | sharing services (settings area) | yes | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |

### Device

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `volume` | sound / volume | yes | Account, CalendarSheet, DesktopBottom, DesktopTop, InstallerDrivers, Main, Overview, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `speaker` | loudspeaker / output device | no | PhoneInCall, QuickSettings |
| `mic` | microphone | no | PhoneInCall |
| `sun` | brightness | no | QuickSettings |
| `moon` | night light | no | QuickSettings |
| `contrast` | dark theme | no | QuickSettings |
| `battery` | power / battery | yes | Account, CalendarSheet, DesktopBottom, DesktopTop, Lock, Main, Overview, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `monitor` | display | no | Account, InstallerDrivers, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `keyboard` | keyboard | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `mouse` | mouse and touchpad | no | Account, InstallerDrivers, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `printer` | printing | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `chip` | drivers and hardware | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `video` | camera / video on | no | InstallerDrivers, PhoneDial, PhoneInCall, SettingsDrivers |
| `camera-switch` | switch camera | no | PhoneInCall |
| `screen-share` | share the screen | no | PhoneInCall |
| `usb` | removable drive (USB) | no | CalendarSheet, DesktopTop, Main, Overview, QuickSettings, Store, SurfaceStates |
| `disk` | internal disk / storage | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `device` | this device / system information | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |

### Phone

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `phone` | phone / voice call | no | Account, Lock, PhoneDial, PhoneIncoming |
| `hangup` | end or decline a call | no | PhoneInCall, PhoneIncoming |
| `keypad` | dial pad | no | PhoneInCall |
| `pause` | hold the call | no | PhoneInCall |
| `message` | message | yes | PhoneIncoming |
| `call-in` | incoming call (list) | yes | PhoneDial |
| `call-out` | outgoing call (list) | yes | PhoneDial |
| `call-missed` | missed call (list) | yes | PhoneDial |
| `qr` | QR code | no | Account |

### System

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `gear` | Settings (only that) | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, PhoneDial, QuickSettings, States |
| `power` | power menu | no | Lock, Login, QuickSettings |
| `user` | account / person | no | Account, Login, PhoneDial, PhoneInCall, PhoneIncoming, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |
| `users` | users of this device (settings area) | yes | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `bell` | notifications | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `bell-off` | do not disturb | no | QuickSettings |
| `clock` | time | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `bag` | Ultar Store | no | CalendarSheet, DesktopBottom, DesktopTop, Lock, Main, Overview, QuickSettings |
| `folder` | Files (app) | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |
| `access` | accessibility | no | Account, Login, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `spark` | Assistant | yes | Account, Installer, QuickSettings, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store, SurfaceStates |
| `update` | updates | no | Account, CalendarSheet, SettingsDisplay, SettingsDisplayLight, SettingsDrivers, Store |
| `shot` | screenshot | no | QuickSettings |
| `palette` | appearance | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `colour` | colour management | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `dock` | desktop and dock (settings area) | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `workspaces` | windows and workspaces (settings area) | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `apps` | apps (settings area) | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `language` | region and language | yes | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `backup` | backup | yes | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `cells` | cells and containers | no | Account, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |
| `sos` | emergency | no | Account, Lock, SettingsDisplay, SettingsDisplayLight, SettingsDrivers |

### Media transport

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `play` | play | no | Lock |
| `prev` | previous track | no | Lock |
| `next` | next track | no | Lock |

### Store

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `compass` | Discover (Store) | no | Store |
| `installed` | installed (Store) | no | Store |
| `package-file` | install from a file (Store) | no | Store |

### App family

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `browser` | Ultar Browser (app) | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |
| `firefox` | Firefox (fallback browser component) | no | Installer, Store |
| `terminal` | Terminal (app) | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |
| `editor` | Text editor (app) | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |

### Components

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `office` | component Office tools | no | Installer, Store, SurfaceStates |
| `code` | component Coder | no | Installer, Store, SurfaceStates |
| `media` | component Media | no | Installer, Store |
| `mail` | component Communication | no | Installer, Store |
| `package` | component Linux programs | no | Installer, Store |
| `windows` | component Windows programs | no | Installer, Store |
| `android` | component Android apps | no | Installer, Store |
| `server` | component Containers and server | no | Installer, Store |
| `remote` | component Remote access | no | Installer, Store |

### Places in Files

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `place-documents` | place: Documents | yes | DesktopTop, Main |
| `place-downloads` | place: Downloads | no | DesktopTop, Main |
| `place-pictures` | place: Pictures | no | DesktopTop, Main |
| `place-music` | place: Music | no | DesktopTop, Main |
| `bin` | place: Trash | no | CalendarSheet, DesktopBottom, DesktopTop, Main, Overview, QuickSettings |

### File kinds

| Icon | Meaning | Mirrored in RTL | Used on |
|---|---|---|---|
| `kind-folder` | file kind: folder | no | DesktopTop, Main |
| `kind-text` | file kind: text document | yes | DesktopTop, Main |
| `kind-sheet` | file kind: spreadsheet | yes | DesktopTop, Main |
| `kind-image` | file kind: image | no | DesktopTop, Main |

## Named in section 4, not needed on these boards (keep in the table, not drawn yet)

main menu (three lines), restore (two squares), pen, clipboard, scissors, share/export, import, undo, unlock, eye crossed, microphone crossed, speaker muted, video crossed, calendar sheet, star, pin, cloud variants.

## Open

- App icons are monochrome here: the coloured app family of section 4 needs colour tokens per theme (round-1 finding D40/A1). No colour outside `tokens.toml` is used.
- File and app icons larger than 24 need a size token (`icon_large`); the boards put a 24 icon in a 48 tile instead.
