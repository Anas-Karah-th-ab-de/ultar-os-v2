# ultar-foreign – apps from other systems (stage P2: Linux, stage P4: Windows)

Plan and stages: `docs/FREMD-APPS.md`. This document describes what stage P2 built: Linux programs from a
`.deb` of the person, installed into a Linux area and run as apps of their own, each in its own cell.
Security comes before every function: nothing is installed without an administrator, nothing of a foreign
package runs outside a cell, and there is no unsafe mode.

## Parts

| Part | Where | What it does |
|---|---|---|
| Linux area | `/data/ultar/foreign/linux`, built by `tools/foreign/make-linux-runtime.sh` | Ubuntu 26.04 "resolute" minbase (24.04 "noble" only when resolute cannot be reached): apt/dpkg, GTK 3 and 4, the Qt 6 Wayland plugin, Mesa (llvmpipe), Noto fonts with Arabic, CA certificates, locales ar_SY/en_US/de_DE, a replacement `xdg-utils` (fails with a message: no bus in the cells). Never systemd, udev, an SSH server, snapd, polkit (apt pins at priority -1); no set-id program (removed at build time and by a `DPkg::Post-Invoke` hook after every dpkg run); no device node. Output: the tree, an ext4 image with its SHA-256, and with `ULTAR_PKG_KEY` a signed ultar-pkg component `foreign.linux-runtime-<version>-<arch>.uap` (kind `data`: the image, its checksum and `runtime.txt`). |
| Service | `system/services/ultar-foreign`, user 844, `rootfs/etc/ultar/services/foreign.toml` | Offers `foreign.*` on the bus (contract `system/core/ultar-bus/src/messages/foreign.rs`, texts `locales/foreign/{ar,en,de}.ftl`). Inspects files, installs `.deb` files after the administrator confirmed, registers each visible program with the launcher, changes and removes them. |
| Launcher profile `app.foreign` | `system/core/ultar-launcher` (`foreign.rs`, `profile.rs`, `plan.rs`) | Kind `compat-deb`: the app's cell has the Linux area read-only (nosuid, nodev) as its root, its own user, home and `/tmp`, the Wayland bridge read-only at `/.ultar`, no bus socket. |
| Android apps (stage P5) | component of the Linux area, built by `tools/foreign/make-android-runtime.sh` | Android Translation Layer (ATL) with ART (`art_standalone`), `bionic_translation`, wolfSSL (JNI) and OpenSL ES from pinned sources as `ultar-android-runtime_<version>_amd64.deb`, installed into the area; `/opt/ultar-android/bin/ultar-apk-run` starts an app. See "Android apps" below. |
| Bridge | `ultar-wayland` (stage P1, `/usr/lib/ultar/foreign`) | Started in the cell as `/.ultar/ultar-wayland -- <Exec>`; the only socket in `XDG_RUNTIME_DIR=/tmp/.run` is its `wayland-0`. Without the bridge the program starts without a display. |

## Flow of an installation

1. **Inspect** (`foreign.inspect`, Files): the file is opened below the person's areas or mounted drives only
   (`/data/ultar/areas`, `/run/ultar/media`), one path component at a time without following any link, only
   a plain file (a planted pipe or link is refused). Its head says what it is (`deb`, `exe`, `msi`, `apk`);
   only `deb` is installable in P2. The fields of a `.deb` are read by the area's own `dpkg-deb` in the
   **inspection cell** from a copy in the service's inbox; the copy is what is installed afterwards, whatever
   happens to the original.
2. **Confirm**: the system dialog asks for an administrator's password (`sysdialog.authenticate`), naming the
   file's own (unverified) name, that it comes from another system and was not checked by KM Code Craft,
   and what the app will get (network, folders). Cancelled or no answer: nothing is installed.
3. **Install**: `apt-get -o APT::Sandbox::User=root install` of the copy in the **installation cell**
   (no network; the cell is apt's sandbox, see
   below). A failed step leaves nothing: the package is purged again, registered apps are removed, the copy
   is deleted.
4. **Register**: the package's visible desktop entries (`Type=Application`, not hidden, not terminal-only)
   whose program resolves inside the area become apps `foreign.<source>-<package>` via
   `launcher.install_foreign`; icons are copied out of the area (hicolor, pixmaps, at most 1 MiB, no link
   leaves the area). Progress of every job: `foreign.progress`; list changes: `foreign.changed`.

**Later changes** (`foreign.set_choices`, Settings): granting more (network, the local network, a folder)
is confirmed by an administrator like an installation (`Outcome::Asking(job)`, text
`foreign.confirm.change`); taking back is immediate. **Removal** (`foreign.remove`): the launcher removes
the app (and its data unless kept), the icon goes, the package is purged when no other app uses it.

## The two cells of the service

Both: new mount, pid and network namespaces; the area as root (`pivot_root`, the old root detached);
`tmpfs` on `/run` and `/tmp`; own `/proc` with `sys`, `sysrq-trigger`, `irq`, `bus`, `fs` bound read-only;
in `/dev` only `null zero full random urandom`, each bound on its own (nodes the kernel lacks are left
out; never the kernel's device tree – a kernel that cannot bind a node builds no cell); the inbox
read-only at `/run/inbox`; no new privileges; stage 2 is pid 1 and ends with the parent. **Rights:** on
Linux the capability bounding set is cut before the identity switch; on the Ultar kernel it is not
(`PR_CAPBSET_DROP` takes the matching Ultar rights at once – `CAP_SETUID` would take `set_id` before the
switch): there every Ultar right is dropped after the switch, but `cell_chown` in the installation cell,
and the rest is checked. A cell that cannot be built does not start; its exit
code `140 + n` names the failed step (`cellrun::SetupStep`).

| Cell | Area | Identity | Capabilities (Linux) | Runs |
|---|---|---|---|---|
| inspection | read-only | nobody | none | `dpkg-deb --field` |
| installation | writable, nosuid, nodev | root of the cell; Ultar: right `cell_chown` only | bounding set CHOWN, DAC_OVERRIDE, FOWNER, FSETID (no set-id: apt does not switch to `_apt`) | `apt-get install/remove`, `dpkg --purge` |

## Who may do what (bus policy, `rootfs/etc/ultar/bus-policy.toml`)

| Identity | May |
|---|---|
| `system.foreign` | register `foreign`, publish `foreign.progress`/`foreign.changed`, call `launcher.install_foreign`, `launcher.uninstall`, `sysdialog.authenticate`; holds the right `launcher.foreign` (pseudo method: the launcher takes unsigned `app.foreign` manifests and uninstalls foreign apps from this identity only) – never `launcher.install`, `launcher.admin`, `launcher.grant` |
| `app.files` | `foreign.inspect`, `foreign.install`; subscribe `foreign.progress` |
| `app.settings` | `foreign.list`, `set_choices`, `remove`, `runtime_state`, `runtime_update`; subscribe `foreign.changed`, `foreign.progress` |
| everyone else | nothing (checked by `ultar-busd` `the_foreign_service_and_its_callers_get_exactly_their_part`) |

Rights of the service (`foreign.toml`, user 844): `mount` (the cells' views), `set_id` (nobody, root of
the cell) and `cell_chown` (handed to the installation cell alone: the kernel lets it give owners only on
the cell's own writable mounts, `docs/integration/zell-chown.md`), group `userfiles` (836) to read the
chosen file. The launcher checks independently: a foreign manifest gets
an id `foreign.*`, the fixed signer `sha256("ultar.launcher.install_foreign.v1")`, never replaces an app of
another profile, and store keys may not be named "foreign".

## The app's cell (launcher, profile `app.foreign`)

Strictly required (no `accept_missing`): own user ids, the cell, memory groups, the network namespace and
single device binds (`device-binds`: the probe bit that gives every cell its single-bind `/dev`, stage
P0). Without the network permission the cell has its own empty network namespace; **with** it the cell
gets its own namespace behind the kernel's barrier "internet only" (`Feature::NetGuard`,
`ultar_sys::ultar_net_guard`, `docs/integration/zell-netz.md`) – a kernel without the barrier (Linux)
refuses the start, there is no unsafe mode. The barrier has no mode for the local network yet: an app the
person allowed the local network gets the internet only (never more than granted).

**Title bars.** Every `app.foreign` app gets the compositor right `ui.client-header` (declared by
`foreign::parse`, granted with the profile): programs that draw their own title bar (GTK 3 and 4 never
negotiate `xdg-decoration`) appear with one bar instead of two; a program that asks for server-side
decoration through `xdg-decoration` gets the compositor's (ultar-wayland passes the mode on; a decoration
object without a mode gets its first `configure` at the initial commit). This is safe because the system
dialogs carry the compositor's own mark, drawn outside any app surface – an app's title bar cannot
imitate it. Limits: memory
1024/4096 MiB, 256/1024 processes, `/tmp` 256 MiB, no kernel rights. Environment: `PATH`,
`XDG_RUNTIME_DIR=/tmp/.run`, `WAYLAND_DISPLAY=wayland-0`, `GDK_BACKEND`/`QT_QPA_PLATFORM`/
`SDL_VIDEODRIVER=wayland`, `MOZ_ENABLE_WAYLAND=1`, `LIBGL_ALWAYS_SOFTWARE=1`, `NO_AT_BRIDGE=1`, `LANG`/`LC_ALL`
from the person's language (`ar_SY`, `en_US`, `de_DE`).

## Tests

| Test | Command | Last line |
|---|---|---|
| unit and contract | `cargo test -p ultar-foreign -p ultar-launcher -p ultar-bus -p ultar-busd` | `test result: ok` |
| Linux end to end (root) | `tools/foreign/linux-e2e.sh [OUT]` | `FOREIGN-LINUX-E2E: ALLES OK` |
| area | `TEST_PACKAGE=mousepad tools/foreign/make-linux-runtime.sh OUT` | `LINUX-RUNTIME: OK` |
| Android runtime | `tools/foreign/make-android-runtime.sh AREA OUT` | `ANDROID-RUNTIME: OK` |
| Android end to end (root) | `tools/foreign/android-e2e.sh [OUT]` | `FOREIGN-ANDROID-E2E: ALLES OK` |
| Ultar kernel (QEMU) | `AREA=OUT tools/kern-test/foreign-e2e.sh` | `FOREIGN: ALLES OK` or `FOREIGN: FEHLENDE KERN-FUNKTIONEN: …` |
| kernel features alone (QEMU) | `tools/kern-test/fremd-kern.sh` | `FREMD-KERN: ALLES OK` |
| GTK window (QEMU, picture) | `AREA=OUT tools/kern-test/foreign-gtk-kern.sh` | `FOREIGN-GTK: SHOT …` |

The Linux end to end uses the shipped bus policy, the real launcher (in-process, real cells) and the
service binary: callers without a rule and files outside the areas are refused, "cancelled" installs
nothing, mousepad is installed and registered, its cell is checked from inside (own uid, area read-only,
host hidden, no bus, only harmless devices, only `lo`, no capabilities, `NoNewPrivs`, a set-id copy runs as
the app, private `/tmp`), more permissions ask again and network without the barrier refuses the start, a
hostile package (set-id file, `/proc/sys` write, write outside, mount, icon link out, entry pointing out)
stays in the installation cell, and removal leaves the area's package set as before.

## The Ultar kernel (QEMU, 28.11.2026, branch `integ/fa-linux`)

With stage P0 every cell feature is there: mount, pid and network namespaces, the barrier, single-file
binds, read-only binds, remount nosuid/nodev, tmpfs, proc, `pivot_root`, `PR_SET_PDEATHSIG`,
`PR_SET_NO_NEW_PRIVS`, nobody, `cell_chown` in the installation cell (EPERM outside), and Ubuntu 26.04's
own programs (`dpkg-deb`, `apt-get`, glibc, ld.so). Both service cells run; the package fields are read.
For dpkg the integration added to the kernel: rename of an empty directory, `link`/`linkat` as a copy of a
regular file, symbolic links and their rename on a disk by a cell holding `CELL_CHOWN` (only on its own
writable mounts), ext4 `unlink` that removes a link as a link, fast links never freed as block pointers,
`ENOTEMPTY` for `rmdir` of a directory with entries.

The kernel gaps that stopped `apt-get install` of a `.deb` are closed (`foreign-e2e.sh` → `FOREIGN: ALLES OK`
up to `package-install`; `fremd-kern.sh` → `FREMD-KERN: ALLES OK` checks each one alone):

| Gap | Cause | Fix (kernel) |
|---|---|---|
| dpkg: "package metadata contained directory '…/control'" (seen as an empty listing of `dir/`) | `rmdir` of a regular file succeeded, so dpkg took every file for a directory | `rmdir` of a non-directory `ENOTDIR`, `unlink` of a directory `EISDIR`, `unlinkat(AT_REMOVEDIR)`, `rmdir "."` `EINVAL` |
| `dpkg-deb --control`: `Illegal instruction`; perl (debconf): page fault at `0x5555555687c0` | `execve` kept the caller's signal handlers: the new image jumped to the old program's handler address | `execve` resets caught handlers to the default (ignored stay ignored, flags/masks cleared) and drops `clear_child_tid` |
| syscalls 436, 277, 26, 81 | not implemented | `close_range` (with `CLOSE_RANGE_CLOEXEC`), `sync_file_range`, `msync` (file mappings are copies: nothing to write back), `fchdir` (`fdcalls.rs`) |
| apt: "Can not write log (Is /dev/pts mounted?)" | no pseudo-terminals | `/dev/ptmx` and `/dev/pts/N` (`pty.rs`): line discipline (canonical lines, echo, `ICRNL`, `ONLCR`, `^C`/`^\`/`^Z` to the foreground group), termios and window size (`SIGWINCH`), `TIOCGPTN`/`TIOCSPTLCK`/`TIOCGPTPEER`, controlling terminal; at most 256 |

Security of the terminals: the table is global, but a slave opens only for the user that opened its master
(or a holder of `FILE_ADMIN`); `/dev/pts/N` reports that owner and mode 0620. A cell gets terminals through
`ultar_sys::cell::mount_ptys`: on Linux a devpts instance of its own (`newinstance`, `nosuid,noexec`), on the
Ultar kernel binds of `/dev/ptmx` and `/dev/pts` – an app sees the numbers of other terminals but cannot open
them (own uid). The foreign service's cells and the launcher's app cells mount them; the end to end checks that
an app cell sees no device beyond `null zero full random urandom tty ptmx`.

GTK on the Ultar kernel (`tools/kern-test/foreign-gtk-kern.sh`, mousepad with the area as its root behind
ultar-wayland): the window shows (`FOREIGN-GTK: SHOT … colours=453`). GTK's image loader glycin starts its
loaders through `bwrap`, which needs user namespaces (the Ultar kernel has none, on purpose). The area ships
`bubblewrap 99:0.11-ultar1` from `tools/foreign/bwrap-shim.c` instead (pinned like the xdg-utils
replacement): it reads bwrap's options, keeps `--chdir`, `--setenv`/`--unsetenv`/`--clearenv`, `--argv0`,
`--die-with-parent`, `--block-fd`, `--args` and runs the program in place. This weakens nothing: the program
already runs in the app's cell (own uid, read-only area, no bus, no capabilities, `NoNewPrivs`), which is the
sandbox bwrap would have built, and the shim cannot grant more than the cell has.

Known approximations: file mappings with `MAP_SHARED` are copies (apt's binary cache is rebuilt each run);
the byte count of a partial terminal write after `ONLCR` is approximate.

## Stage P3: the surfaces (branch `feat/fa-p3-ui`, not in main)

Every surface says plainly that a file from another system is **not checked**, and every permission
starts **off**, whatever the file suggests.

* **Files** (`apps/ultar-files/src/foreign.rs`, `view/foreign.rs`): opening, double-clicking or
  "Install" in the context menu of one `.deb`/`.exe`/`.msi`/`.apk` (also on the desktop icon) asks
  `foreign.inspect` and shows a sheet: warning bar "Not checked", kind, name, version, size and the
  publisher "as the file says, not checked"; what the app may use – internet (with a LAN switch only
  when internet is on), each folder on its own (documents, downloads, pictures, music, videos, desktop,
  home) – all off. "Install" sends `foreign.install` (`InstallFrom::File`, host path below
  `/data/ultar/areas` or `/run/ultar/media`); the service still asks for an administrator in the system
  dialog. The sheet follows `foreign.progress` (step, percent), ends with "Open"; closing it while
  running lets the job finish and says its end in a notice. Kinds the service cannot install yet
  (`NotYet`), a missing Linux area, a refused package and a missing service answer with their own text.
* **Settings › Apps from other systems** (area `foreign`, group apps; `update/foreign.rs`,
  `view/foreign.rs`): the Linux area (installed, version, size, "Update" → `foreign.runtime_update`),
  the list of apps with their source; the page of one app: warning "not checked", internet / LAN /
  each folder as switches (more asks the administrator – the page waits for the job, less applies at
  once), "Keep data" and "Remove" (confirmation dialog, `foreign.remove` with `keep_data`). Errors of
  the service are shown on the page.
* **Desktop grid** (`platform/ultar-desktop/src/view/apps.rs`): a foreign app's name says its system
  (`Mousepad · Linux`); "Details" and "Uninstall" open the Settings page (removal only through the
  service, never the launcher directly). **Mobile**: the grid carries the same source mark; the system
  dialog loads the `foreign` confirmation texts.
* **Store**: the publisher line of programs from other systems names the system
  (`KM Code Craft · Linux · verified`); the app page already showed "Source".
* **Bus policy**: unchanged from P2 – `app.files` calls `foreign.inspect`/`foreign.install` and
  subscribes `foreign.progress`; `app.settings` calls `foreign.list`/`set_choices`/`remove`/
  `runtime_state`/`runtime_update` and subscribes `foreign.changed`/`foreign.progress`. Nothing else.
* Texts: ar (first), en, de for every new key; digits 0–9; start/end layout; light and dark.

Tests: `apps/ultar-files/tests/foreign.rs` (7: nothing granted until clicked, LAN only with internet,
running job closed, `NotYet`, no service, context menu, desktop icon, pictures),
`apps/ultar-settings/tests/foreign.rs` (5: no service/area, more asks and less applies, removal keeps
data, area update, pictures), unit tests in Files, desktop, mobile and Store. Pictures:
`ULTAR_FILES_SHOTS=<dir>` / `ULTAR_SETTINGS_SHOTS=<dir>` (`files-foreign-{ready,choices,running,done,not-yet}-*`,
`settings-foreign-{,app-,remove-,empty-}{ar,en}-{dark,light}`).

## Windows programs (stage P4)

A `.exe` or `.msi` of the person becomes an app of its own that **installs itself in its own cell** on its
first start. Nothing of the file runs anywhere else, and the cell is the same `app.foreign` cell as a Linux
program's (the area read-only, own user and data, no network unless granted, no bus).

| Part | Where | What it does |
|---|---|---|
| Component „Windows programs“ | `make-linux-runtime.sh` with `COMPONENTS=windows` (amd64 only) | Ubuntu's own `wine`/`wine64` 10 and `fonts-wine` (checked by apt's signatures; no WineHQ repository – one key fewer to trust). Wine 10 of Ubuntu 26.04 runs 32-bit programs in WoW64 mode (`i386-windows` libraries, no 32-bit Linux libraries) and has the Wayland driver (`winewayland`). Plus the wrapper `/usr/lib/ultar/wine/ultar-wine-app`. `/etc/ultar-runtime` says `components=windows`. |
| Readers | `src/pe.rs`, `src/msi.rs`, `src/winread.rs` | What the file says about itself, read and never run: product, version, company (PE version resource; MSI `Property` table through the compound file and its string pool), the installer kind (NSIS mark after the image, Inno Setup in the version resource), a library is refused, the largest PNG icon. Every offset and count is checked, streams and resources are capped (32/16 MiB), chains follow at most as many steps as the file has sectors; fixed-seed fuzz tests (4000 damaged files each). |
| Service | `src/windows.rs` | `inspect`: the file's own name, version, publisher (`verified: false`); `installable` only with the component. `install`: the administrator confirms (`foreign.source.exe`/`.msi` in the question), the file is kept as the app's **own installer** `<root>/setup/<app id>/<safe name>` (`0755`/`0644`), the PNG icon in `icons/`, the app is registered as kind `compat-exe` whose program is the wrapper with `<kind> <file> <name>`. `remove`: the launcher removes the app and its data (the Wine prefix), the service its installer and icon; nothing is purged from the Linux area. |
| Launcher | `foreign.rs` `setup_dir`, `[paths] foreign_setup` | The directory named exactly like the foreign app (never a link) is bound **read-only** at `/.setup` in that app's cell; no manifest may start anything below `/.setup` or `/.ultar` directly. `compat-exe` gets the same cell as `compat-deb`. |
| Wrapper | `tools/foreign/wine/ultar-wine-app` (in the area) | In the cell, as the app: the Wine prefix in `$HOME/.wine` (Wayland as display; no download of Wine's .NET/HTML engines), **only drive `C:`** (Wine's `Z:` for `/` and every other drive link removed at every start), the installer (`msiexec /i … /qb`, NSIS `/S`, Inno Setup `/SILENT`; another installer with its own window; a program without installer is copied to `C:\Program Files`), the program it brought (first start-menu shortcut that is not an uninstaller or document, one matching the product first), remembered in `$HOME/.ultar-wine/program`, then started (`wine start /wait /unix <shortcut>`; the cell ends with it). |

Why the installer runs on the first start and not in the service: the service cannot enter an app's data
(`data_root` is the launcher's, `0700`) and must not – the only place where a Windows program may write is
its own cell. The person sees the installer's progress window through the bridge (MSI, Inno Setup) or the
program after a silent install (NSIS).

Wine as Ubuntu ships it, learned in the cells: its server is not in `PATH`
(`/usr/lib/x86_64-linux-gnu/wine/wineserver`, the wrapper finds it), it **breaks with `TMPDIR` set**
(`free(): invalid pointer` at its start – the cells set `TMPDIR=/tmp`, the wrapper unsets it), it finds
its libraries through `/proc/self/exe` (the cells bring their own `/proc`), NSIS installers give up
(code 2) when started through Wine's `\\?\unix\` path – they read themselves through their own path –
so the wrapper runs every installer from a drive of its own, `S:` = the read-only `/.setup`, removed again
after the installation; and a new prefix is about
**1.2 GB** (Wine copies its Windows libraries, 64 and 32 bit, into every prefix) – per Windows app, in
its own data.

Security notes: Wine's `\\?\unix\` path reaches what the cell sees (the read-only area, its own data, its
`/tmp`), never the host – the cell is the boundary, the missing `Z:` is tidiness. The installer copy is
read-only in the cell (`/.setup`), and the data mount is `noexec` (Wine maps Windows programs by reading
them). No network without the person's permission (same rules as Linux programs).

### Test

| Test | Command | Last line |
|---|---|---|
| readers, service, launcher | `cargo test -p ultar-foreign -p ultar-launcher -p ultar-bus` | `test result: ok` |
| Windows end to end (root, real cells) | `tools/foreign/windows-e2e.sh [OUT]` | `FOREIGN-WINDOWS-E2E: ALLES OK` |
| Wine on the Ultar kernel (QEMU) | `AREA=OUT DEB= tools/kern-test/foreign-e2e.sh` | `FOREIGN: windows-programs ok` (today: `FEHLT`, see below) |

The end to end builds the area with `COMPONENTS=windows` and, for the test only, `TEST_EXTRA="xvfb
x11-apps"` (an X server inside the cell for a screenshot – the real display is the bridge), downloads
Notepad++ 8.7.1 (NSIS) and 7-Zip 24.09 (MSI) from their official pages (SHA-256 printed and pinnable with
`NPP_SHA256`/`SEVENZIP_SHA256`), and checks for each: described from its own data, "cancelled" installs
nothing, confirmed: registered with its own installer, first start installs it in the cell (own uid, only
`lo`, installer read-only, drives only `c:`, `Z:` refused, a host file unreadable through `\\?\unix\`), its
window shows (screenshot with content, `OUT/shots/*.png`), removal through Settings leaves no data, installer
or icon and the area's packages unchanged.

### The Ultar kernel and Wine (QEMU, 28.11.2026)

`AREA=<area with COMPONENTS=windows> DEB= tools/kern-test/foreign-e2e.sh` also runs `windows-programs`
(foreigntest: the area as root in a private mount namespace with its own `/proc`, `/tmp` and device nodes –
no cell, since the kernel has no network namespace). Result: `wine --version` runs (`wine-10.0`), the new
prefix does not: `wineboot -i` ends with `err:virtual:virtual_alloc_first_teb wine: failed to map the shared
user data: c0000018` (`STATUS_CONFLICTING_ADDRESSES`), nothing of the prefix is made.

| Missing for Wine | Seen | Why Wine needs it |
|---|---|---|
| `mmap` at a chosen address: hints are ignored (`MAP_FIXED_NOREPLACE` included, the kernel places at its own next address), `MAP_FIXED` only in 4–256 MiB and from 16 TiB | run | Windows' fixed addresses: `KUSER_SHARED_DATA` at `0x7ffe0000`, the reserved low 1 MiB, the lower 4 GiB of 32-bit programs, image bases (`0x140000000`, `0x180000000`) |
| `waitid` (247) | run (`noch nicht umgesetzt: 247`) | waiting for the started `wineserver`/children |
| `arch_prctl(ARCH_SET_GS)` (only `SET_FS`/`GET_FS`) | code | Windows x64 code finds its TEB through `%gs` |
| `sigaltstack` (131) answers 0 and does nothing | code | Wine handles signals (exceptions of Windows code) on its own signal stack |
| 32-bit programs: a user code selector `0x23` (the kernel has 32-bit code at `0x1b`) and `modify_ldt` (154) | code | WoW64 switches to 32-bit code and gives it its `%fs` TEB through an LDT entry |

"code" means read in the kernel's sources and not yet reached by the run (it stops at the first
mapping). Until these exist, Windows programs run on the Linux kernel only (like apps with network without
the barrier of stage P0).

## Android apps (stage P5, 28.11.2026, branch `feat/fa-p5-android`)

**Runtime.** `tools/foreign/make-android-runtime.sh AREA [OUT]` builds in a build tree of its own (Ubuntu of
the area's release with the build dependencies, never in the area, never on the VPS) from these sources
(their licences; the commits are written to `OUT/SOURCES` and `/opt/ultar-android/SOURCES`):

| Source | Commit / tag | Licence |
|---|---|---|
| `android_translation_layer` (gitlab.com/android_translation_layer) | `b87f0455fda749b8138eba52dee45e0105861656` | GPL-3.0-or-later |
| `art_standalone` (ART, libcore, wolfSSL-JNI) | `381138f574985b593eaaa64ad5660cfc4040d4b4` | Apache-2.0, GPL-2.0 with classpath exception (libcore) |
| `bionic_translation` | `6253dec7f24a57a94039a088cb436d880e92e3be` | Apache-2.0 / BSD |
| `libopensles-standalone` | `c6982620c7dadeb90220b23c575b25eeffda765f` | Apache-2.0 |
| `wolfssl` (github.com/wolfSSL) | `v5.8.2-stable` (`decea12e223869c8f8f3ab5a53dc90b69f436eb2`) | GPL-3.0-or-later |

Everything installed by the five builds goes into one package `ultar-android-runtime` whose `Depends` are the
Ubuntu packages of the libraries it needs (from the programs' `NEEDED` entries); it is installed into a
copy of the area with the area's apt (set-id bits removed, no device node left, every program's libraries
resolve). `OUT/linux` is the area with the component (`/etc/ultar-runtime` gets `android=<version>`),
`OUT/linux-android-<version>-amd64.ext4` its image, with `ULTAR_PKG_KEY` a signed component
`foreign.android-runtime-<version>-amd64.uap`. A self-test lets ART run a program of its own in the area
(`OUT/hello.dex`, also used on the Ultar kernel).

What the component never has: Google services or any Play store; real device identities (ATL answers fixed
values: the Android id `_totally_an_androidID`, manufacturer and model fixed, system properties
`unknown`; the area's `/etc/machine-id` is empty); location, microphone and WebView (`ultar-apk-run`
unsets ATL's switches `ATL_UGLY_ENABLE_LOCATION/MICROPHONE/WEBVIEW` and the debugger port
`JDWP_LISTEN`).

**Inspection** (pure Rust, `src/apk`, in the inspection cell as the internal program
`/.ultar-internal/apk-inspect`; the service re-checks the bounded line answer): ZIP (stored/deflate, no
ZIP64, no encryption; at most 100 000 entries, 16 MiB directory), the binary `AndroidManifest.xml`
(package, version name/code, min SDK, permissions, label, icon, the enabled launcher activity or alias),
`resources.arsc` (label per language, icon per density, references followed four deep), APK Signature
Scheme v3 then v2 (signing block, signature over the signed data with the signer's key: RSA PKCS#1 and
PSS, ECDSA P-256/P-384; the certificate's key must be the signing key; content digest over the entries,
the directory and the end record in 1 MiB chunks). The ZIP, manifest and resource readers are fuzz-tested
(no panic, bounded memory); the signature check is tested with files signed by a test signer
(`src/apk/testsign.rs`, feature `test-sign`, never in the service) and with changed bytes.
The dialog shows the signer as the SHA-256 of its certificate with its common name, **unverified** (an
Android certificate names nobody checked). Suggested: "internet only" when the app asks for `INTERNET`,
nothing else. Camera, microphone, location, contacts, phone, SMS and sensors are never granted; the
answer lists those the app asks for (`Inspected.not_granted`, texts `foreign.android.*`).

**Refused** (`Inspected.refusal`, texts `foreign.refusal.*`; `foreign.install` ends `Failure::Damaged`
before anyone is asked): only ARM code ("needs an ARM processor"), other processors, unsigned, JAR signing
(v1) only, a signature that does not match, no start screen, the runtime not installed. An **update** must
come from one of the installed app's signers (else `Failure::SignerChanged`, before anyone is asked) and
must not be older (`Failure::Older`).

**Installation** (installation cell, internal program `/.ultar-internal/apk-place`, which reads the copy
again and must find the same package without refusal): the APK as
`/opt/ultar-android/apps/<package>/base.apk` (0644 root) and its x86-64 libraries (`lib/x86_64/*.so`, plain
names only, at most 512 files, 256 MiB each, 1 GiB together) in `…/<package>/lib`; the icon (largest
density, at most 256 KiB) next to the others; the app `foreign.apk-<name>` (kind `compat-apk`, same profile
and cell as a Linux program: area read-only as its root, own user, no bus, no network without the
permission) starting `/opt/ultar-android/bin/ultar-apk-run <package> <activity>` behind the Wayland bridge.
**Removal** deletes the package's directory in the area (internal program `apk-remove`) and the app with its
data.

**Start** (`ultar-apk-run`): the app's data (`ANDROID_APP_DATA_DIR`, ART's `ANDROID_DATA`) is
`$HOME/android` – writable, never executable; native code is never extracted there
(`ATL_SKIP_NATIVES_EXTRACTION=1`): its `lib` is a link to the read-only copy in the area.

**Pictures (`bwrap-cell`).** ATL decodes every Android `Bitmap` with GTK; gdk-pixbuf 2.44 of the area loads
images through glycin, which starts its loaders with `bwrap` – that needs user namespaces the app's cell does
not give, so without help every picture stayed empty and the test app stopped with a `NullPointerException`.
`tools/foreign/bwrap-cell.c` (installed by `tools/foreign/install-bwrap-cell.sh`, called by
`make-android-runtime.sh`; Ubuntu's bwrap is diverted to `/usr/bin/bwrap.ubuntu`) accepts bubblewrap's options
and runs the helper directly **in the same cell**: it keeps what needs no rights (`--clearenv/--setenv/
--unsetenv`, `--chdir`, `--die-with-parent`, `--new-session`, `--argv0`, the seccomp filters it is handed –
applied where the kernel has seccomp, so glycin's own syscall filter still holds on Linux), ignores namespace,
mount, capability and id options and never gives the helper more than the app has; it is static and never
set-id. Stage P2 (`main` since `a885a16d`) gives the Linux area the same stand-in as its own package
`bubblewrap` (`tools/foreign/bwrap-shim.c`); `install-bwrap-cell.sh` then leaves that one in place. Once both
are merged bwrap-cell can go – after its handling of the seccomp filters moved into bwrap-shim (which skips
them).

**Checked** (`tools/foreign/android-e2e.sh`, 28.11.2026, Linux server, real cells, headless Weston as the
display): Gravity Defied (`org.happysanta.gd_29.apk` of ATL's test set, SHA-256 `0786257c…d675`, Java, APK
signature v2) – inspected (label, version, signer, "internet only" suggested), refused variants (ARM only,
one changed byte, JAR signing only, unsigned), cancel installs nothing, installed as `foreign.apk-gd` (uid
20000), its window shows the game (menu, track, pictures), an update by another signer and an older one are
refused, removal leaves nothing: `FOREIGN-ANDROID-E2E: ALLES OK`.

**What ATL cannot do (yet).** ATL implements a part of the Android API on GTK: many apps stop at an
unimplemented class or method; no Google services (by design); no WebView, location or microphone (off
here); no GPU – OpenGL ES runs on Mesa's software renderer only, 3D games are slow; no other app's activities
or content providers, no background services that outlive the window, no notifications into the system yet;
ARM-only apps are refused (no translation of ARM code). ART runs without a compiled boot image: the cache in
the app's data is never executable (`noexec`), so ART falls back to its interpreter and JIT – the first start
takes some seconds longer. Shipping a precompiled boot image in the read-only area would remove that.

**On the Ultar kernel** (QEMU, `tools/kern-test/foreign-e2e.sh` with `AREA=OUT` on `integ/fa-linux` `a885a16d`,
28.11.2026): every check of the service and its cells passes, including `package-install`; ART does not run
yet (`FOREIGN: FEHLENDE KERN-FUNKTIONEN: android-art`). The check now stops each program after a time limit
and reports what it saw:

| Missing | Seen | Consequence |
|---|---|---|
| `readlink("/proc/self/exe")` through a proc mounted in the area (`sys_readlink` answers only paths that resolve to the kernel's own `/proc/`) | `readlink /proc/self/exe` prints nothing in the area | glibc cannot expand `$ORIGIN`, the run path of `dalvikvm` – `libnativehelper.so` not found, exit 127 |
| `/proc/<pid>/maps` | `pthread_getattr_np failed for GetThreadStack: No such file or directory` | glibc cannot tell the main thread's stack: ART aborts at its start (seen when started through ld.so, which needs no `$ORIGIN`) |
| mappings below 4 GB (`MAP_32BIT`, address hints) | `The requested address space (0x7f00251df000, …) cannot fit in low_4gb` | ART's heap holds 32-bit references and must lie below 4 GB |
| a program that aborts ends | after its fatal error ART was still running after 300 s (killed by the check) | a crashing app would hang instead of ending |
| writing larger files into the area's image | `ENOSPC` copying the check's files into the 2.2 GB test image (the plain area's 0.6 GB image works) – cause open: the kernel's ext4 or too few free inodes in the image | the check keeps ART's data on a tmpfs |

## Open points

* The installation cell has no network: archive installs (`InstallFrom::Archive`) and `runtime_update`
  answer `Failure::Network` – with stage P0 the cell can get its own namespace behind the barrier.
* GTK under `foreign-gtk-kern.sh` runs as root of the area (foreignshow); the launcher's own start of a foreign
  app on the Ultar kernel (own uid, `app.foreign`) is covered by the Linux end to end only.
* A mode of the barrier for the local network (printers, NAS); until then LAN means internet only.
* Installing the signed component on a device (verify against `/etc/ultar/trusted-keys`, unpack into
  `/data/ultar/foreign/linux`) is not implemented; the images do not ship the service yet
  (`tools/vm/make-vmware.sh` `SERVICES_UP`).
* Linux hosts: without a seccomp filter a cell can create a user namespace (all app cells, not only these;
  reported by the end to end). The Ultar kernel has no user namespaces.
* Windows: each app's prefix is about 1.2 GB (a shared read-only prefix template would save it); the
  Wayland driver is the default but the end to end shows windows through X11 in the cell (Xvfb) – the
  run with the real bridge `ultar-wayland` (P1) is still to be done; icons of MSI packages (their `Icon`
  table) are not read; a program with several start-menu entries becomes one app (the main one); installers that need the network (web installers) fail without
  the network permission; the installer copy stays as the app's own (repair/reinstall) until removal.
* Folders (`files` areas) for foreign apps are wired through the launcher but not covered by the end to end.
* P3: the surfaces ran in the click and picture tests only, not on the Ultar kernel (settings-e2e and
  launcher-e2e do not cover `foreign.*`); on the phone a foreign app is removed in Settings (the grid has
  no menu); "Install" of a file the person picks is offered by Files, not by the Store (the Store shows
  the source of its own catalogue entries).
