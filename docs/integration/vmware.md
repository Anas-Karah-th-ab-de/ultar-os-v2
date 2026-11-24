# Ultar OS v2 in VMware Workstation – integration notes

`tools/vm/make-vmware.sh` builds a VMware Workstation machine from the tree, `tools/vm/install-vmware.ps1`
puts it on the Windows PC and starts it, `tools/kern-test/vmware-like.sh` is the kernel gate on the same kind
of hardware. **QEMU is the test bed for all of it; VMware Workstation itself was not started by these tools.**

## 1. What the kernel has for VMware's virtual hardware

| VMware device | Kernel | Checked in QEMU with |
|---|---|---|
| UEFI firmware, GOP frame buffer | `efi.rs`, `grafik::init_gop`, `/dev/fb0` | OVMF, `-vga std` and `-vga vmware` |
| VMware SVGA II (15ad:0405) | **no driver**: the firmware's GOP picture and resolution stay (no mode change, no acceleration) | `-vga vmware` |
| NVMe (`nvme0`) | `drivers/nvme.rs` (polling) | `-device nvme` (start disk, installer target) |
| SATA/AHCI (`sata0`) | `drivers/gen_ahci.rs` (polling; disks only, no CD/DVD) | q35 AHCI (start disk, installation medium) |
| LSI Logic SAS/parallel, BusLogic, PVSCSI, IDE (PIIX4) | **no driver** | – |
| e1000 / e1000e | `drivers/gen_intel_eth.rs` (8086:100e, 10d3, …) | `e1000`, `e1000e` |
| vmxnet3 | **no driver** | – |
| xHCI (`usb_xhci`) | `drivers/gen_xhci.rs`: keyboards, mice, absolute tablets (HID report descriptor), hot-plug; no hubs, no storage | `qemu-xhci` + `usb-kbd`, `usb-tablet` |
| UHCI/EHCI (`usb`, `ehci`) | **no driver**: what VMware attaches there is not seen | – |
| PS/2 keyboard and mouse (i8042), VMware absolute pointer (backdoor) | `drivers/ps2kbd.rs`, `drivers/ps2maus.rs` | i8042, `vmmouse` of QEMU |
| HD audio (`hdaudio`) | `drivers/gen_hda.rs`; the machine is built without sound | – |
| VMCI, PCI bridges, PCIe root ports | bridges: `drivers/bruecke.rs`; VMCI is turned off in the `.vmx` | PCIe root port in `ci.sh` |

PCI is read through the ports `0xCF8/0xCFC` over all 256 buses, so devices behind VMware's PCIe root ports are
found without the PCIe configuration space. Nothing essential is missing for a start: no driver was added.
In VMware the kernel line "every device has a driver" will not appear (SVGA II, PIIX4 IDE, UHCI/EHCI are
listed as driverless) – that is expected and does not stop anything.

## 2. Kernel gate `tools/kern-test/vmware-like.sh`

The programs and markers of the first boot of `ci.sh`, without any virtio device (a bound virtio driver fails
the run): `q35-ahci-usb` (AHCI start disk, e1000e, xHCI keyboard + tablet), `q35-nvme-ps2` (NVMe, e1000 as
the only network card – here geraetetest runs its bind cycle –, i8042 input only), `pc-nvme-svga`
(i440FX/PIIX, NVMe, e1000e, xHCI, VMware SVGA II: only the IDE controller and the display may be driverless
and the GOP picture must be there). `REPEAT=3`, `RUNS=…`, `VL_LOG=<prefix>`; verdict `VMWARE-LIKE: ALLES OK`.
Two changes made it possible: `qtest.py` takes `MACHINE`, and `fenstertest` no longer assumes that no USB
input device is plugged in at the start (it then opens `event2` instead of expecting it to be absent).

## 3. Session start by the kernel: `/etc/ultar/session`

New, in `shell::init_starten` (called from `main.rs` on an installed system): when the system partition holds
`/etc/ultar/session` with the word `init`, the kernel starts `/sbin/ultar-init` (else `/usr/bin/ultar-init`)
as PID 1 with the system language in `ULTAR_LANG`, instead of `ultar-shell`. Without the file, with another
word, or when the program is missing or does not start, everything is as before (`ultar-shell`, then the
start page). No existing medium has the file.

## 4. The machine

`make-vmware.sh` → `out/vmware/UltarOS-v2/{UltarOS-v2.vmx, UltarOS-v2.vmdk}`, `out/vmware/UltarOS-v2.zip`
(+ `.sha256`), logs and screenshots `out/vmware/UltarOS-v2-*.{log,ppm}` (`.png` with Pillow).

* Disk: written by the kernel's own installer (medium on AHCI → empty 4 GiB disk on NVMe, in QEMU):
  `ULTAR-EFI` (loader), `ULTAR-BOOTCTL`, `ultar-kernel-a/b`, `ultar-system-a/b`, `ultar-data`; then
  `qemu-img convert -O vmdk` (monolithicSparse).
* System: `/sbin/ultar-init`, the shipped `init.toml`, `ids.toml`, `bus-policy.toml`, rights and seat files,
  the shipped service files `logd`, `bus`, `settingsd`, `devd`, `seat`, `authd`, the app launcher and the
  services the shell talks to (`sessiond`, `powerd`, `notifyd`, `calendard`, `audiod`, `mediad`, `captured`,
  `netd`, `btd`, `volumed`, `displayd`, `accountd`), and `compositor` without its line `manual = true` (this
  image has no `ultar-shell` that would drive the same display); plus the desktop session (lock, sysdialog,
  desktop: the shipped definitions without `manual`; one account, password from `VM_PASSWORD_FILE` or random
  in `out/vmware/UltarOS-v2.password`). `/bin/busybox` when the build computer has a static one. Language:
  `LANG_CODE` (default `ar`) on `ULTAR-EFI` and in the system.
* System apps (§6): Settings, Files, Store, Phone and the browser, each a package (`/usr/lib/ultar/apps/<id>.uap`)
  signed with the system key and installed by a one-shot service `app-<id>` (`ulaunch install` on the
  administrator's socket) at every start; the launcher runs each in its cell as its own user.
* Start: firmware → `ultar-loader` (slot A) → kernel (start picture in Arabic) → `ultar-init` (PID 1) → logd,
  seat, bus, compositor, devd, settingsd, authd, launcher and the shell's services → compositor on `fb0`,
  **locked**: the lock screen first, the account's password unlocks, then the shell's bar (Arabic: mirrored),
  sheets, overview, app grid; Super+L locks again. `ultar-shell` (the old surface) is not in this image. The
  check (`vmware-vm.py check`) wants the lock screen (and no bar) on the screen and every app installed.
* `.vmx`: hardware version 16, `firmware = "efi"`, 4 GiB, 4 processors, NVMe disk, e1000e (NAT), UHCI/EHCI +
  xHCI with VMware's USB pointer on the xHCI, PS/2 keyboard, no sound, no VMCI, `serial0` → `serial.log`
  beside the `.vmx` (the kernel's whole log – the first thing to read when the screen stays black).
* Checks (`tools/vm/vmware-vm.py check`, each on a copy-on-write layer, the `.vmdk` is delivered unstarted):
  the raw disk on NVMe, the `.vmdk` on NVMe with the VMware SVGA II adapter, the `.vmdk` on AHCI. Verdict
  `VMWARE-CHECK: OK` needs the `[boot] slot` line of the loader, `ready` of the six services, `start` of
  both windows, `settled`, no refused or ended service, no panic, no virtio device, and at least 1 000 pixels
  of each window colour on the screenshot.

## 5. Not checked, and what to try

* VMware Workstation itself: firmware, processor features, timer calibration, start of the further
  processors, VMware's NVMe/AHCI/xHCI models and its USB pointer were never run with this kernel.
* Screen stays black or the machine restarts: read `serial.log`. Disk not found: `install-vmware.ps1 -Disk
  sata`. Pointer does not move: `install-vmware.ps1 -Ps2Mouse` (then click into the window; Ctrl+Alt frees
  the pointer).
* Network only as far as the kernel's DHCP line.

## 6. Input, lock screen and apps in VMware (fix/vmware-input)

Found in VMware Workstation 26 on the image of `b18f58b` (desktop drawn, no input, no lock screen, nothing
opens); reproduced in QEMU with the same disk and only emulated devices (`tools/vm/vmware-input.py`).

| Symptom | Root cause | Fix |
|---|---|---|
| Keys do nothing (USB and PS/2) | the kernel's input nodes had **one** event queue each: whoever read first took the event. In this image `ultar-init` is PID 1 and watches every node with volume keys for its emergency exit (`rescue.rs`, Vol+ and Vol− held) – every keyboard has them – so it read away the keys before the compositor (in the desktop gate `ultar-init` is not PID 1: no watch, no loss). Pointer nodes have no volume keys and were not affected | kernel `input.rs`, `eingabe.rs`: one queue per open descriptor (as Linux' evdev clients); every reader gets every event; a descriptor of a number that now belongs to another device reads `ENODEV` |
| Desktop visible at start, before any password | the compositor locked only on the first lock surface, Super+L or Ctrl+Alt+Del; nothing locked at boot (the lock screen stays hidden until told) | compositor `--start-locked-in system` (`Config::start_locked`): the session starts locked in the boot profile `system` (`ULTAR_PROFILE`, now passed to every service by `ultar-init`), not on the installation medium. Nothing but the lock screen is drawn or gets input until it unlocks with the password; a lock screen that attaches is told `SessionLocked{true}` at once and shows itself. Test `tests/start_locked.rs` |
| Clicking Settings, Files, Store, browser, phone opens nothing | (1) no app launcher and no app in the image; (2) the shell asked the launcher for `org.ultar.settings` etc., which can never be a launcher id (system apps have one-part ids, `install.rs`); (3) apps could not reach the bus at all (every bus socket is a service's own, `0660` + its group), Settings and Files end without it; (4) the Store looked for `/run/ultar/ui/compositor`; (5) no `ultar-phone` program; (6) the browser ends when its engine bundle (WPE WebKit, aarch64 only) is missing | (1) image: launcher, the shell's services, the apps as signed packages, one-shot installs, `accept_missing = ["net"]` (the kernel has no network namespaces); (2) shell `ports/bus.rs` maps `org.ultar.<id>` ↔ `<id>`; (3) bus socket `/run/ultar/appbus/bus.sock` = identity `app.shared` (`0666`, ≤ 32 connections, only rule `app.*`), bound read-only into every cell (`launcher.toml` `shares`); (4) the shipped socket; (5) `apps/ultar-phone/src/bin/ultar-phone.rs` (without a call service every request says it is not reachable); the phone draws its own header, which needs the compositor right `ui.client-header` – the launcher did not know it, so the compositor ended the phone's connection at once: new launcher permission `ui.client-header` (system apps get it at installation), the phone's manifest asks for it; (6) `engine::offline`: the window opens, every page load fails at once |
| The right password often ended on the cover again (found by the new gate) | after `Unlock` the lock screen drew the sign-in once more; every frame of a lock surface locks (§8a), so that late frame locked the session again | `app/lock.rs`: hidden at once when it sends `Unlock` (the surface goes, no frame); e2e regression check |
| Lock screen: the accessibility menu shows four switches without a word (owner's VMware screenshot) | the menu added only switches; a switch carries just its accessible name, it draws no text | `lock/access.rs`: the name as text beside each switch |
| Sign-in: "no user on this device" under the user's own name | the sign-in sets that error before the user list arrives and never cleared it | `app/lock.rs` `refresh_user` clears it once a user is known |
| The lock screen's accessibility switches changed nothing (owner) | the settings service stored `accessibility.*`, nobody read it; no high-contrast palette, no text scale in the toolkit; no screen reader and no on-screen keyboard program exist | toolkit: palette `[theme.contrast]` (tokens.toml, ≥ 7:1) and a text factor (`Ui::set_high_contrast`, `Ui::set_text_scale`, `PaintEnv`); lock screen and shell follow `accessibility.high_contrast` / `large_text` (1.25) live (`Look::with_access`; the shell through `settings.changed`); the screen reader and on-screen keyboard switches are disabled with "not available on this system yet". Apps do not follow yet (no settings right on `app.shared`) |
| Ctrl+A, Ctrl+C, Ctrl+V did nothing (owner) | text fields had no selection and no clipboard commands (Ctrl+letter went to the app); a paste with an empty clipboard was a protocol error that ended the app | toolkit text field: selection (drawn), Ctrl+A/C/X/V/Z (also on the Arabic layout's keys), undo (32 steps, none for passwords), nothing copied or cut out of a password field; `UiWindow` offers copies and reads pastes (compositor clipboard `SetSelection`/`ReadSelection`); compositor: an empty clipboard answers `TransferCancelled`. The shell's and the lock screen's own fields (desktop `Host`) and the browser's window loop do not take part yet |
| A second, nameless bar button next to Settings / the browser | their windows announced `app.settings` / `ultar-web` | window app ids `org.ultar.settings`, `org.ultar.browser` |
| Files: "show as list" hangs the window (owner, 1024x768) | in list view the tree never showed the state's selection: the table was built without its marked rows and the app read the selection only from grid tiles, so after any selection every turn of the window loop found a difference and built the whole tree again (100 % CPU, the press of every click lost between its down and up); the table was not in a scroll container (1 000+ rows unreachable) and drew only the keyboard row as selected | `apps/ultar-files/src/app.rs`: the selection is read from tiles or rows, compared as sets, nothing happens on an idle turn; `view/items.rs`: the table sits in a `Scroll` (rows painted and given accessibility nodes only in the viewport) and shows the state's selection, the scroll offset survives a rebuild; toolkit `Table` draws every marked row. Tests `apps/ultar-files/tests/listview.rs` (REAL-INPUT: down, idle turn, up; 1 500 entries; empty folder) |
| Quick settings: the Wi-Fi symbol and tile, the tiles with an arrow (wired, VPN, power mode), gear, lock and power open nothing; sliders say "0%" with the knob at the end (owner) | a tile without its service (Wi-Fi, VPN, Bluetooth, Airplane in the image) and its arrow were disabled: clicks were swallowed; the bar's status button had one meaning only; Settings' manifest in the image declared no `[open]` scheme, so the launcher refused `settings://network` (wired tile, "Wi-Fi settings", the settings tiles) and Settings ignored the URI anyway; a refused lock was silent; sliders without a device showed the value 0 (in Arabic the knob of 0 sits at the right end) | shell: a tile that cannot switch opens its page (Wi-Fi, VPN, Bluetooth – the page says why, its switch is disabled, "… settings" still leads on) or its area of Settings; arrows always open their page; the network symbol on the bar opens the Wi-Fi page (toolkit: a status icon click reports `Selected(icon)`); a failed lock posts a notice; sliders without a value write "—" and draw no knob (`Slider::unknown`). Launcher: `[open] page_arg` (manifest) hands the page of `<scheme>://<page>` to the app as its own argument; the image's Settings manifest: `schemes = ["settings"]`, `page_arg = "--area"`. Tests `platform/ultar-desktop/tests/e2e_quick.rs` (REAL-INPUT: real compositor at 1024x768, absolute pointer like VMware's), `clicks.rs`, launcher `tests/opening.rs`; gate `tools/kern-test/ui-regress.sh` in `ci.sh`, VMware gate steps "list view" and "quick settings" |

**Gate** `tools/vm/vmware-input-e2e.sh` (in `vmware-like.sh`, so in `ci.sh`; `VMWARE_INPUT=0` skips it):
builds the disk with `make-vmware.sh` (test password, digits: the lock screen starts with the Arabic layout)
and drives it **only** through QEMU's emulated devices with QMP `input-send-event` – no `--input-ack`, no
developer remote control, no write into an input node: run `usb` (q35, xHCI keyboard + tablet, as the
`.vmx`) and run `ps2` (i440FX, i8042 keyboard + VMware absolute pointer through the backdoor, VMware SVGA II,
no USB) and run `usb-hd` (as `usb` with a 1920x1080 firmware screen). Steps, each judged on a QEMU screenshot: lock screen from the first frame (the bar must never be
seen before the unlock) → a key opens the sign-in, which is checked against board Lock: the whole frame
buffer covered (no black band), brand, avatar, name and password field drawn (Arabic and Latin glyphs), the
accessibility menu closed until clicked and then with the names of its switches → a wrong password stays locked → the password shows the
desktop at the first try and it stays → the start button (found by the brand mark on the bar) opens the overview → Settings, Files,
Store, browser (bar) and the phone (app grid) each open a window → the clock in the bar turns → Ctrl+A,
Ctrl+C in Settings' search field and Ctrl+V in Files' search field (text arrives) → Super+L, the
accessibility menu: high contrast (black ground) and large text (more text pixels) take effect at once and
high contrast is still on after `system_reset`.
`VMWARE-INPUT-E2E: ALLES OK`. After a failed run (or `LOGS=1`) the log service's directory is read from the
data partition of the run's disk layer (`PREFIX-log/`; with `ULOG=` as text) – records of the last seconds
before QEMU is stopped may be missing (`--fsync batch`).

Open points:
* VMware itself is still not run by any tool. The USB pointer VMware attaches (`0e0f:0003`) was parsed as a
  tablet; its reports were never seen in a test (QEMU's `usb-tablet` has another report descriptor). When
  the guest turns the VMware absolute mouse on (it does: `[maus] absolute Maus (VMware-Schnittstelle)
  aktiv`), VMware may send the pointer there instead – both paths reach the compositor (`event0` / `eventN`).
* Apps share one bus identity (`app.shared`): what a manifest declares for the bus (`[permissions] bus`) is
  not granted yet, so Settings shows its services as unreachable, the Store cannot list or install, the
  phone has no call service. Per-app identities need the launcher to hand each cell its own socket.
* Files: the user's areas are not bound into the cell on the Ultar kernel (`file_binds=false`), so Files
  shows "this place is not available".
* The browser has no engine on x86 (pages fail at once); the screen keeps the firmware's size (no SVGA
  driver). In QEMU the session fills the firmware's screen at 1024x768, 1280x800 and 1920x1080; the black
  bands of the owner's VMware screenshot were not reproduced (that picture shows the bottom start corner of
  the sign-in with the accessibility menu opened by a click). Only the Arabic image was run; the English
  catalogue is the same program with other strings (the fonts are inside the programs).
