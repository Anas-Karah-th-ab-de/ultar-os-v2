# Installer boot mode (w7/inst, "Bau Kern" + "Bau Dienst")

Patch: `agents/w7/inst/CHANGES.patch` against `fda77ad` (`wip/settings-installer`), `git apply` format with
binaries (the patch deletes the committed test binaries below).

## Open points (first)

1. **End-to-end installation is not green yet (task 3).** `tools/kern-test/installer-e2e.sh` builds a medium
   whose marker asks for the installer session, boots it with an empty NVMe disk on VMware-like hardware and
   drives `ultar-installer-ui` by keys through the QEMU monitor, following the installer's `--report` lines.
   Proven in QEMU: the kernel starts ultar-init with the profile `installer`; logd, bus, devd, seat, compositor
   and the installer come up; the installer lists `/dev/ultar/disk/nvme0` and is driven by Tab/Enter through
   language, accessibility, keyboard, network, try/install, kind, drivers, components, extras, disk and
   encryption to the account step. **There it stops:** characters typed through the monitor are lost (of
   "anas" one to three characters arrive, of a 14-character password 4–9), on the USB keyboard and on the PS/2
   keyboard alike, also with 2.5 s between characters, and Super+Space (Latin layout) does not take effect.
   Tab and Enter, which the driver confirms by the next report line, always arrive. Cause not found in this
   round; candidates: the kernel only polls the keyboards (no interrupts) inside `read`/`poll` of an input
   node (`eingabe::pumpen`), and USB keys of a node nobody reads go to the console path. Because of this the
   step is **opt-in** in `ci.sh` (`INSTALLER_E2E=1`); the marker `INSTALLER: ALLES OK` and the first start
   (`ultar-firstboot` → account, check service lists it) are implemented but have not run green.
   `make-vmware.sh --installer` (an installer VM with an empty second disk) is **not done**.
2. **Encryption (task 4) not done – the installer option stays disabled** with its reason
   (`installer.reason.encryption`: "the Ultar kernel cannot unlock an encrypted disk at start"). Nothing of a
   LUKS2 writer or a kernel XTS layer is in this patch; no half-working encryption ships. Plan: (a) a no_std
   crate writing/reading the LUKS2 on-disk format (binary header + JSON area, argon2id keyslot, AF splitter
   with sha256, pbkdf2-sha256 digest) on RustCrypto `argon2`, `aes` + `xts-mode`, `sha2`, `hmac`/`pbkdf2`,
   tested against `cryptsetup luksDump`/`open --test-passphrase` both ways (cryptsetup-bin on B2); (b) the
   kernel opens `ultar-data` through an XTS block device after a passphrase prompt on the early console, key
   only in kernel memory, zeroized on shutdown; (c) the installer formats the data partition and drops the
   disabled reason.
3. **Restart after the installation:** Done → Restart ends `ultar-installer-ui` with code 0; nothing reboots
   the machine yet (no power call of the service manager in this base). The e2e test restarts by itself.
4. **authd is not in this base** (`wip/desktop-shell`). `ultar-firstboot` writes the account in authd's file
   format (`/data/ultar/auth/users`, header `#ultar-auth crc32=…`, `user <name> admin=1 display=<hex>
   password=<argon2id phc> pin=-`, owner 813 = authd's id there). The installer profile does not start authd
   (the installer does not need it: the password is hashed by the installer itself).
5. Ids: `installer = 840` (`ids.toml`); 813–832 are taken in the shell/service branches. Merge both lists.

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
