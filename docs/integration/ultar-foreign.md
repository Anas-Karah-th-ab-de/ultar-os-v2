# ultar-foreign – apps from other systems (stage P2: Linux, stage P4: Windows)

Plan and stages: `docs/FREMD-APPS.md`. This document describes what stage P2 built: Linux programs from a
`.deb` of the person, installed into a Linux area and run as apps of their own, each in its own cell.
Security comes before every function: nothing is installed without an administrator, nothing of a foreign
package runs outside a cell, and there is no unsafe mode.

## Parts

| Part | Where | What it does |
|---|---|---|
| Linux area | `/data/ultar/foreign/linux`, built by `tools/foreign/make-linux-runtime.sh` | Ubuntu 26.04 "resolute" minbase (24.04 "noble" only when resolute cannot be reached): apt/dpkg, GTK 3 and 4, the Qt 6 Wayland plugin, Mesa (llvmpipe), Noto fonts with Arabic, CA certificates, locales ar_SY/en_US/de_DE, a replacement `xdg-utils` (fails with a message: no bus in the cells). Never systemd, udev, an SSH server, snapd, polkit (apt pins at priority -1); no set-id program (removed at build time and by a `DPkg::Post-Invoke` hook after every dpkg run); no device node. Output: the tree, an ext4 image with its SHA-256, and with `ULTAR_PKG_KEY` a signed ultar-pkg component `foreign.linux-runtime-<version>-<arch>.uap` (kind `data`: the image, its checksum and `runtime.txt`). |
| Service | `system/services/ultar-foreign`, user 842, `rootfs/etc/ultar/services/foreign.toml` | Offers `foreign.*` on the bus (contract `system/core/ultar-bus/src/messages/foreign.rs`, texts `locales/foreign/{ar,en,de}.ftl`). Inspects files, installs `.deb` files after the administrator confirmed, registers each visible program with the launcher, changes and removes them. |
| Launcher profile `app.foreign` | `system/core/ultar-launcher` (`foreign.rs`, `profile.rs`, `plan.rs`) | Kind `compat-deb`: the app's cell has the Linux area read-only (nosuid, nodev) as its root, its own user, home and `/tmp`, the Wayland bridge read-only at `/.ultar`, no bus socket. |
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
3. **Install**: `apt-get install` of the copy in the **installation cell** (no network until stage P0, see
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
in `/dev` only `null zero full random urandom` (single binds; the Ultar kernel falls back to its gated
device tree); the inbox read-only at `/run/inbox`; no new privileges; every Ultar right dropped (and
checked); stage 2 is pid 1 and ends with the parent. A cell that cannot be built does not start; its exit
code `140 + n` names the failed step (`cellrun::SetupStep`).

| Cell | Area | Identity | Capabilities (Linux) | Runs |
|---|---|---|---|---|
| inspection | read-only | nobody | none | `dpkg-deb --field` |
| installation | writable, nosuid, nodev | root of the cell | bounding set CHOWN, DAC_OVERRIDE, FOWNER, FSETID, SETUID, SETGID | `apt-get install/remove`, `dpkg --purge` |

## Who may do what (bus policy, `rootfs/etc/ultar/bus-policy.toml`)

| Identity | May |
|---|---|
| `system.foreign` | register `foreign`, publish `foreign.progress`/`foreign.changed`, call `launcher.install_foreign`, `launcher.uninstall`, `sysdialog.authenticate`; holds the right `launcher.foreign` (pseudo method: the launcher takes unsigned `app.foreign` manifests and uninstalls foreign apps from this identity only) – never `launcher.install`, `launcher.admin`, `launcher.grant` |
| `app.files` | `foreign.inspect`, `foreign.install`; subscribe `foreign.progress` |
| `app.settings` | `foreign.list`, `set_choices`, `remove`, `runtime_state`, `runtime_update`; subscribe `foreign.changed`, `foreign.progress` |
| everyone else | nothing (checked by `ultar-busd` `the_foreign_service_and_its_callers_get_exactly_their_part`) |

Rights of the service (`foreign.toml`): `mount` (the cells' views) and `set_id` (nobody, root of the cell),
group `userfiles` (836) to read the chosen file. The launcher checks independently: a foreign manifest gets
an id `foreign.*`, the fixed signer `sha256("ultar.launcher.install_foreign.v1")`, never replaces an app of
another profile, and store keys may not be named "foreign".

## The app's cell (launcher, profile `app.foreign`)

Strictly required (no `accept_missing`): own user ids, the cell, memory groups, single device binds. Without
the network permission the cell has its own empty network namespace; **with** it the kernel's barrier
"internet only" (stage P0) is required – until it exists such an app does not start. Limits: memory
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
| Ultar kernel (QEMU) | `AREA=OUT tools/kern-test/foreign-e2e.sh` | `FOREIGN: ALLES OK` or `FOREIGN: FEHLENDE KERN-FUNKTIONEN: …` |

The Linux end to end uses the shipped bus policy, the real launcher (in-process, real cells) and the
service binary: callers without a rule and files outside the areas are refused, "cancelled" installs
nothing, mousepad is installed and registered, its cell is checked from inside (own uid, area read-only,
host hidden, no bus, only harmless devices, only `lo`, no capabilities, `NoNewPrivs`, a set-id copy runs as
the app, private `/tmp`), more permissions ask again and network without the barrier refuses the start, a
hostile package (set-id file, `/proc/sys` write, write outside, mount, icon link out, entry pointing out)
stays in the installation cell, and removal leaves the area's package set as before.

## The Ultar kernel (QEMU, 28.11.2026)

Ubuntu 26.04's own programs (`dpkg-deb`, `apt-get`, glibc and ld.so of the area) run unchanged; mount and
pid namespaces, read-only binds, remount nosuid/nodev, tmpfs, proc, `pivot_root`, `PR_SET_PDEATHSIG`,
`PR_SET_NO_NEW_PRIVS`, switching to nobody and root writing the area's files work. Missing:

| Missing | Errno | Consequence |
|---|---|---|
| network namespace (`CLONE_NEWNET`) | `EINVAL` | neither cell of the service nor an app without network can be built (both refuse) |
| single-file bind (device node into the cell) | `ENOTDIR` | the launcher refuses foreign apps (`device-binds` strictly required); the service's cells fall back to the gated device tree |
| `chown` by root of a cell without rights | `EPERM` | dpkg/apt cannot give files to other owners (packages with non-root files, apt's `_apt` sandbox) – needs a right scoped to the area, not `file_admin` |
| network barrier "internet only" | – | stage P0: apps with network do not start; the installation cell cannot reach the sources (archive installs and area updates answer `Failure::Network`) |

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

## Open points

* Stage P0: the barrier (`ultar_launcher::foreign::set_net_barrier` is a placeholder returning `ENOSYS`;
  `Platform.net_barrier` is false); then archive installs (`InstallFrom::Archive`) and `runtime_update`.
* Installing the signed component on a device (verify against `/etc/ultar/trusted-keys`, unpack into
  `/data/ultar/foreign/linux`) is not implemented; the images do not ship the service yet
  (`tools/vm/make-vmware.sh` `SERVICES_UP`).
* Linux hosts: without a seccomp filter a cell can create a user namespace (all app cells, not only these;
  reported by the end to end). The Ultar kernel has no user namespaces.
* The mobile system dialog does not yet load the `foreign` texts (the desktop's does).
* UI: Files and Settings do not call `foreign.*` yet (policy and contract are ready).
* Windows: icons of MSI packages (their `Icon` table) are not read; a program with several start-menu
  entries becomes one app (the main one); installers that need the network (web installers) fail without
  the network permission; the installer copy stays as the app's own (repair/reinstall) until removal.
* Folders (`files` areas) for foreign apps are wired through the launcher but not covered by the end to end.
