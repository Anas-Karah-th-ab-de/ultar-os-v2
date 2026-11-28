# Installer boot mode (w7/inst, "Bau Kern" + "Bau Dienst")

Patch: `agents/w7/inst/CHANGES.patch` against `fda77ad` (`wip/settings-installer`), `git apply` format with
binaries (the patch deletes the committed test binaries below).

## Open points (first)

1. **End-to-end installation: green (2026-11-26).** `tools/kern-test/installer-e2e.sh` builds a medium whose
   marker asks for the installer session, boots it with an empty NVMe disk on VMware-like hardware and drives
   `ultar-installer-ui` by keys through the QEMU monitor (all 16 steps incl. typing the account step), presses
   **Restart** on the last step, waits for the machine to restart and then starts the installed disk alone:
   `ultar-firstboot` creates the account, the check service lists it. The typed characters that were lost in
   round 1 arrive now (the PS/2 path was reworked by the VMware fixes, `da001dfa`). It runs by default in
   `ci.sh` (`INSTALLER_E2E=0` skips it) and as its own GitHub job (`installer`, about 15 minutes under TCG).
   Fixed on the way: the test's account check ran as root without a right and could not read the account file
   (mode 0600, owner authd) – it has the right `file_admin` now, like `ultar-firstboot`. The VMware machine for the installation is built now (2026-11-28): `INSTALLER_IMG=1 tools/vm/make-vmware.sh` writes `out/vmware/UltarOS-v2-installer.zip` (medium as SATA disk, empty NVMe target, README); GitHub workflow `Installer image` builds it on demand.
2. **Encryption with a passphrase: done (2026-11-28).** Rule "keine eigene Krypto": every primitive is a
   reviewed RustCrypto crate (`aes`, `xts-mode`, `argon2`, `pbkdf2`, `sha2`); our code only lays out the
   on-disk format.
   - `system/core/ultar-luks`: writes and opens LUKS2 as cryptsetup does (both header copies with sha256
     checksum, one Argon2id keyslot – 256 MiB, 4 passes, 4 lanes –, AF split with sha256, pbkdf2-sha256
     digest, segment `aes-xts-plain64`, 512-byte sectors, data at 16 MiB). Other volumes (other ciphers,
     4 KiB sectors, integrity, reencryption) are refused, never misread.
   - Installer: with "passphrase" the data partition gets 16 MiB more (whole disk) or must have them
     (beside / manual, else `installer.reason.space`); it receives the header and the medium's data
     encrypted, every chunk read back. `installer.reason.encryption` is gone.
   - Kernel (`kernel/ultar-kern/src/crypt.rs`): a data partition with a LUKS2 header stays locked,
     `/sys/kernel/ultar/crypt` reads `locked`, the header is readable (never writable) as
     `/dev/ultar/part/ultar-data`. The kernel never sees the passphrase: root with the raw-device right
     writes `open <offset> <master key>`; the kernel maps the segment (software AES, the kernel keeps no
     SSE state) and mounts it as `/data` only if an ext4 is behind it (wrong key: `EKEYREJECTED`).
   - Start: ultar-init runs the graphical unlock screen `/usr/bin/ultar-unlock` (platform/ultar-unlock)
     before anything writes to `/data` and before the services (not on the phone, which uses
     `ultar-secure`/cryptsetup). It draws with the UI toolkit straight onto the framebuffer (look of the
     lock screen, language of the installation, Caps Lock warning) and reads the keys from the input
     devices – while they are open the kernel hands keys only to them, never to the console. The
     strokes are kept as key codes and tried as text in every keyboard layout (Arabic, English, German;
     the installation's layout first), so a passphrase typed in any layout at installation opens.
     Language and keyboard come from an `ultar-start` token in the LUKS2 header (cryptsetup keeps and
     ignores it). Without framebuffer or keyboard (exit 3) or when the screen fails, `ultar-luks unlock`
     asks on the console without echo.
   - Size: on a whole disk the data partition takes the rest of the disk; after the copy the medium's
     ext4 grows into it with `system/core/ultar-ext4grow` (offline like resize2fs: new groups
     BLOCK_UNINIT/INODE_UNINIT, reserved descriptor blocks, resize inode, backups) – plain or through the
     decrypted view of the LUKS2 segment. The kernel's ext4 writer now also takes inodes of
     INODE_UNINIT groups. Growth is bounded by the reserved descriptor blocks (1024 times the medium's
     data file system with mke2fs defaults, 512 GiB for the 512 MiB image); a layout it does not know is
     left as it is.
   - Tests: unit tests of ultar-luks; `tools/kern-test/luks-interop.sh` (cryptsetup 2.7 and dm-crypt read
     what ultar-luks writes and the reverse); `tools/kern-test/crypt-data.sh` (the kernel decrypts what
     dm-crypt encrypted and the reverse, e2fsck clean, no plain text on the disk; `ultar-luks unlock` on
     the serial console: wrong passphrase refused, right one mounts `/data`, nothing echoed); installer
     `installs_encrypted_data_as_luks2`; the graphical screen in QEMU (crypt-data.sh part 3: drawn on
     the framebuffer, keys through the PS/2 keyboard, wrong passphrase refused, right one mounts
     `/data`); renders of every phase in ar/en/de (`platform/ultar-unlock/tests/screen.rs`); growth
     against e2fsck, debugfs and a Linux loop mount (`ultar-ext4grow/tests`), the kernel writing into
     grown groups (`tools/kern-test/ext4-grow.sh`), installer whole-disk installs plain and encrypted
     (`platform/ultar-installer-ui/tests/grow.rs`). All scripts are in `tools/kern-test/ci.sh`.
   - Open: the key schedule stays in kernel memory until power off; TPM still shown as not available
     (no TPM driver); the kernel's ext4 allocator reads one descriptor block per group it looks at
     (slow on very large disks once the first groups are full, correct).
3. **Restart after the installation: done (2026-11-26).** Done → Restart asks the service manager over the bus
   (`init.power` with `Reboot`; the policy gives `system.installer` `init.power` and the pseudo method
   `init.shutdown`, the installer runs only in the profile `installer`). ultar-init ends every service in order,
   unmounts and restarts the machine. The installer prints `[installer-ui] restart requested` before the call
   (init may end it before the answer arrives) and `restart not possible: …` when the call fails; then it
   only ends (status 0) as before. Two kernel faults kept ultar-init from ever reaching the restart and are
   fixed (`weckertest`): a connection still queued at a listener that went away (here: the bus client
   reconnecting while busd ended) never saw end of file, and `SO_RCVTIMEO` was ignored, so the client's
   greeting waited forever and `BusLink::stop` never returned.
4. **All choices are applied at the first start (2026-11-28).** Before, only the account was taken over;
   language, keyboard, time zone, accessibility, device name, edition and components stayed unused in the
   record. `ultar-firstboot --settings-dir /data/ultar/settings --install-dir /data/ultar/install`
   (`src/firstboot_settings.rs`) now stores `region.language`, `region.keyboard.layouts/active`,
   `datetime.timezone.name` (+ `datetime.timezone.automatic = false`, only together with a valid name),
   `accessibility.large_text/screen_reader` and `sharing.device_name` in the settings service's own store
   format: every key resolved and checked against the seed schema, the file chosen by the key's scope
   (`system.toml` / `user-default.toml`), written atomically; a value already stored is kept, a damaged file is
   left for settingsd to quarantine, a refused value is reported, nothing of this fails the start (the account
   must not be lost). Edition, components (only known ids), codecs and "updates during the installation" go
   into `selection.toml` (`pending = true`) for the Store/update service, which install them once online (the
   medium carries no component packages). The log line `[firstboot] settings stored=… kept=… refused=[…]
   damaged=[…] selection=…` names keys only, never values. In the installation image authd **and settingsd**
   start after firstboot (`make-vmware.sh`); settingsd adopts the files at its start (init's `dirs`).
   `installer-e2e.py` checks the line (8 values from the defaults, nothing refused or damaged, selection
   written). Unit tests: right file per scope, settingsd's `Store::open` reads them without a notice, kept and
   damaged files, refused values, hostile component text. **The Store shows them (2026-11-28):** the chosen
   components also go to the setting `apps.install_selection` (their Store catalogue ids,
   `Component::catalog_id`, checked against the shipped catalogue in a test). The Store follows it like
   `updates.*`: Discover names how many are still missing (info bar) and their cards carry the accent badge
   "Chosen at installation"; each is installed from its app page with its rights shown (APP-RULES §3 – no
   silent bulk install). The Store only reads the setting (`settings.get`, no new bus right); installed or
   unknown ids drop out by themselves. `installer-e2e` expects 9 stored values.
5. **authd is not in this base** (`wip/desktop-shell`). `ultar-firstboot` writes the account in authd's file
   format (`/data/ultar/auth/users`, header `#ultar-auth crc32=…`, `user <name> admin=1 display=<hex>
   password=<argon2id phc> pin=-`, owner 813 = authd's id there). The installer profile does not start authd
   (the installer does not need it: the password is hashed by the installer itself).
6. Ids: `installer = 840` (`ids.toml`); 813–832 are taken in the shell/service branches. Merge both lists.

## Task 1 – flaky vmware-like gate: root cause

Not a race. `/dev/input/eventN` and `/sys/class/input/eventN` both come from the same table (`input::NODES`),
and every USB device present at boot is attached synchronously in `driver::bind_all` before any program runs.
The cause was a **stale binary in git**: `kernel/ultar-kern/user/fenstertest` was committed before `de69c3e`
changed `fenstertest-geraete.c` (the old binary still checks "no event2" unconditionally). `scripts/build-kern.sh`
runs `make`, and whether `make` rebuilds depends on the mtimes a checkout happens to give the `.c` file and the
binary – so some checkouts tested the stale program (fails on q35-ahci-usb, which has a USB keyboard), others
rebuilt it. Fix: the nine committed test binaries are deleted and ignored (`user/.gitignore`); they are always
built from source. Gate: vmware-like 5× green (15 boots).

## Task 2 – installer mode

Kernel (`kernel/ultar-kern/src/rawpart.rs`, `fs.rs`, `perm.rs`, `syscall.rs`, `procfs.rs`, `install.rs`, `main.rs`):
- Installer mode = started from a medium with the marker `\ultar-installer` (`efi::VOM_INSTALLER`). The medium is
  the **one** disk with a GPT partition `ULTAR-INSTALL`; with none or several nothing is offered.
- Nodes (only in installer mode): `/dev/ultar/disk/<device>` for every other block device (read/write, `lseek`
  END = size, unaligned writes read-modify-write, `fsync` = device flush) and `/dev/ultar/medium` (read only,
  `EROFS` for writing). `stat`: block device root:840, 0660 (medium 0440).
- Open needs the right `raw_device` **and** the installer's identity (euid 840 or group 840); root without the
  group is refused. The target disks are never mounted in installer mode (only the medium's file systems).
- Session: a marker line `session=init` makes the kernel start ultar-init with `/proc/cmdline` =
  `ultar.profile=installer` (new file); `session=console` gives the kernel console (tests, installation by hand);
  without a line the kernel's own installer runs as before (AB-TEST and make-vmware unchanged).
- Test `tools/kern-test/installer-disk.sh` + `user/installtest.c` (`INSTALLER-DISK: ALLES OK`, in `ci.sh`):
  normal start with two disks has no nodes and no profile; installer start has exactly one target, the medium
  is the boot disk (GPT with ULTAR-INSTALL) and not among the targets, refused for root without the group, for
  user 1000 with the right, for the group without the right; opens for uid 840 and for group 840 with the
  right; write/fsync/read back at the disk's end; medium read only.

ultar-init (`spec.rs`, `config.rs`, `main.rs`): boot profiles. A service file may have
`[profiles] <name> = "start" | "manual"`; without the table it belongs to `system` with its `manual` flag.
`manual` beside `[profiles]`, an empty table, more than 8 entries or bad names are refused. The active profile
is the last valid `ultar.profile=` of the kernel arguments, default `system`. Shipped: logd, bus, devd, seat
start in both; compositor `system = "manual"`, `installer = "start"`; `installer.toml` (only `installer`:
`ultar-installer-ui --source /dev/ultar/medium --disks-dir /dev/ultar/disk --report`, user 840, right
`raw_device`, console); `firstboot.toml` (system, oneshot, root, raw_device + file_admin). Bus: listener
`installer.sock=system.installer` (group 840), policy `devices.list/switch/rescan`.

Installer (`platform/ultar-installer-ui`): `--disks-dir DIR` (each entry a disk, max 32), `--report` (one line
per change of step, focus and Next state with its blocking reason key – never typed text; plus "installation
written|failed"). New bin `ultar-firstboot` + `src/firstboot.rs`: reads the `UIAN` record from ULTAR-BOOTCTL,
checks every value (user name rules of authd, Argon2id PHC only, display name bounds), writes the account file
atomically unless one exists, then wipes the record; `--list` prints the accounts. Unit tests incl. a
fixed-seed fuzz of the record.

## Gates run

fmt, clippy `-D warnings` (ultar-init, ultar-installer-ui), their tests, `ultar-check all` clean,
`tools/design-conformance/run.sh` OK, vmware-like 5× (15 boots green), `installer-disk.sh`; `ci.sh` run 1 fully
green (incl. INSTALLER-DISK and AB-TEST 44 checks); run 2 green up to AB-TEST, which failed one power-loss
check ("4.0 s after Installing the update", timing under load); `ab-test.sh` alone again: ALLES OK.
`ab-test.sh` on this base misses `cp -r system/core/ultar-brand` (fixed on main 7744e78; applied locally
for the gate, not in the patch).

## Applying on main 7744e78

`git apply` on 7744e78 succeeds for every file except: `platform/ultar-installer-ui/*` (needs the
`wip/settings-installer` branch merged first) and `kernel/ultar-kern/user/fenstertest` (main rebuilt that
binary: delete it anyway, `git rm`). No conflict in `kernel/ultar-kern` or `ci.sh`; the kernel builds there.
