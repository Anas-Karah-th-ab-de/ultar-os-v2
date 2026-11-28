# ultar-foreign – apps from other systems (stage P2: Linux)

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
| Android runtime | `tools/foreign/make-android-runtime.sh AREA OUT` | `ANDROID-RUNTIME: OK` |
| Android end to end (root) | `tools/foreign/android-e2e.sh [OUT]` | `FOREIGN-ANDROID-E2E: ALLES OK` |
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
* Folders (`files` areas) for foreign apps are wired through the launcher but not covered by the end to end.
