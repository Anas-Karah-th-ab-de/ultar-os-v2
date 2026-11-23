# A/B start chain – loader, slot table, raw partitions, installer (K1–K4, part of K6)

Closes the "no real boot backend" point of `ultar-update` (INTEGRATION §4 b): on the Ultar kernel
the updater now installs into the other slot, the loader starts it once with a counter, and a release
that does not come up is given up without anybody's help.

## 1. Pieces

| Where | What |
|---|---|
| `system/core/ultar-slots` (new, `no_std`, no `unsafe`) | **One source** for the contract: slot table (`table`: `SlotTable`, `select`, `plan_store`, `seq_newer`), layout (`layout`: partition names, offsets, sizes, `BootOptions` = load options), slot record (`meta`: verity root), strict GPT reader (`gpt`). Used by the updater, the loader, the installer and the kernel. |
| `kernel/ultar-loader` (new, UEFI app, own workspace) | `\EFI\BOOT\BOOTX64.EFI` on `ULTAR-EFI`. `lib` = the whole decision logic over a `Disk` trait, host-tested; `main.rs` + `uefi.rs` + `blockio.rs` = firmware binding (all `unsafe` with `// SAFETY:`). 15 KiB. |
| `kernel/ultar-kern` | `bootslot.rs` (load options → running slot, trial, disk GUID; `/proc/ultar/boot_slot`; trial start that fails → restart), `rawpart.rs` (K4), `install_ab.rs` (K1), `flock.rs` (`flock(2)`, needed by `ultar-updated`), `ext4.rs` (extent trees of depth 1, BLOCK_UNINIT groups), `fs::store_file_res` + `sys_close` (fsync/close/rename report EIO/ENOSPC), small hooks in `main.rs`, `fs.rs`, `procfs.rs`, `syscall.rs`, `proc.rs`, `efi.rs`, `driver.rs`, `drivers/{virtio_blk,nvme,gen_ahci}.rs`. |
| `system/core/ultar-update` | `partctl::PartitionBootControl` (new backend), `slottable` now re-exports `ultar-slots`, `bootctl::Slot` = `ultar_slots::Slot`, config keys `partition_dir` + `running_slot_file`, error `LayoutNotAb` (+ ar/en/de text). |
| `tools/kern-test/ab-test.sh`, `abtest.py`, `gptimg.py`, `ab-agent/` | The A/B gate in QEMU/OVMF, called from `ci.sh` (`AB_TEST=0` skips it). |
| `scripts/build-loader.sh`, `make-install-medium.sh` | Loader build; the Ultar-kernel medium now carries `ULTAR-EFI` (loader), `ultar-kernel`, `ultar-compat`, `ultar-data`. |
| `tools/check-layers.toml` | one `[[allow]]`: kernel → `system/core/ultar-slots` (contract crate, like drivers → `ultar-driverpkg`). No baseline entry added. |

## 2. Disk layout (installer, `install_ab.rs`)

```
GPT name        size                      content                                written by
ULTAR-EFI       as on the medium          FAT: \EFI\BOOT\BOOTX64.EFI = loader     installer
ULTAR-BOOTCTL   1 MiB (min 128 KiB)       raw: table copy 0 @ 0, copy 1 @ 32 KiB, installer (factory),
                                          slot record A @ 64 KiB, B @ 96 KiB      loader, updater
ultar-kernel-a  max(32 MiB, image)        raw: kernel PE image                    installer, updater
ultar-kernel-b  same                      raw (first MiB zeroed at install)       updater
ultar-system-a  max(64 MiB, 2 × medium)   ext4, mounted read-only as /            installer, updater
ultar-system-b  same                      (first MiB zeroed at install)           updater
ultar-data      as on the medium          ext4, writable, /data, both slots       installer
```
Records are 32 KiB apart, so a torn write of one block (512 B … 32 KiB) never touches another.
Type GUIDs: ESP for `ULTAR-EFI`, Linux data for ext4, an Ultar raw GUID for table and kernels.
The medium must carry `ULTAR-EFI`, `ultar-kernel`, `ultar-compat`, `ultar-data`; a medium without
`ultar-kernel` (Linux desktop edition) is still installed in the old single-root way.

**Migration of a single-root installation: refused with a reason, no in-place conversion.** Such a
disk keeps starting as before (the new kernel without load options mounts `ultar-compat` like the
old one). `ultar-update` on it stops with `update.error.layout_not_ab` ("reinstall from a current
medium", ar/en/de) – it never writes anything there. Reinstalling erases the disk after the
installer's confirmation screen; carrying `ultar-data` over is not implemented.

## 3. Formats

* **Slot table** (64 B, two copies): `docs/SYSTEM-UPDATE.md` §2.5, unchanged. Writer rule
  `plan_store`: overwrite the copy that does not hold the current table with seq + 1, then flush.
* **Slot record** (64 B, `meta.rs`): `"USLM"`, format 1, slot, flag, verity root (32 B), CRC-32 of
  0…59; all other bytes zero. Written (and flushed) before `mark_bootable` flips the table; a
  damaged record of a bootable slot is an error, of an unbootable slot ignored.
* **Load options** (UTF-16, words): `ultar.slot=a|b`, `ultar.trial=1` (counted attempt),
  `ultar.disk=<32 hex digits>` (GPT disk GUID the table was read from), `ultar.loader_tsc=<ticks>`.
  Foreign words are ignored; a malformed, unknown or repeated `ultar.` word refuses the whole
  string – the kernel then mounts **no** system partition (it never guesses its root).

## 4. Flows

**Loader** (`boot::run`): own `LoadedImage.DeviceHandle` → whole-disk Block I/O whose device path
is the disk part of the ESP's → GPT (primary, backup if the primary is damaged; duplicate names
refused) → both table copies → `newest` → `select` → `plan_store` → WriteBlocks + **FlushBlocks**
→ read the kernel partition, PE length from the headers (machine checked, ≤ 64 MiB, ≤ partition)
→ `LoadImage` from memory (device path = ESP + `\EFI\ULTAR\KERNEL-<X>.EFI`, so the kernel's
`DeviceHandle` is still the ESP) → load options → (trial: firmware watchdog 300 s) → `StartImage`.
* decision cannot be written (write or flush error) → a trial is **not** started; the slot a
  decision without trial names starts instead (an uncounted attempt could repeat forever);
* a trial whose kernel cannot be loaded or returns → its attempt is spent, decide again (next
  attempt, then the fallback) – at most 8 rounds;
* no valid table, no bootable slot, no usable kernel on a regular start → rescue:
  `\EFI\ULTAR\RESCUE.EFI` if present, else a message (ar/en/de) and back to the firmware, which goes
  on with its next boot option (installation stick).

**Kernel**: options read before ExitBootServices; `[boot] slot b (trial), loader N us` in the log;
`rawpart::scan` finds the disk whose GPT disk GUID the loader named; `/` = `ultar-system-<slot>`
of that disk read-only, `/data` = `ultar-data` of that disk; no other slot partition is ever
mounted (also not under `/media`). Root missing on a trial, or a panic on a trial → the kernel
restarts the machine itself (ACPI reset register / keyboard controller, as `neustart`), and the loader
spends the next attempt. (Calling the firmware's ResetSystem from the panic path faulted in OVMF.) `/proc/ultar/boot_slot` = `a`, `b` or `-`.

**Raw partitions (K4)**: `/dev/ultar/part/{ULTAR-BOOTCTL, ultar-kernel-a/b, ultar-system-a/b}` –
only those, only on the boot disk. Open: effective uid 0 **and** the raw-device right; write access
to the running slot's partitions → `EBUSY` (also every slot partition when the running slot is
unknown). `read`/`pread`: straight from the device, there is no cache; `write`/`pwrite`: through to
the device (read-modify-write of partial sectors, ≤ 1 MiB per call); `lseek` (`SEEK_END` = size),
`fstat` (block device, size); `fsync`/`fdatasync` = `BlockDev::flush`: virtio-blk write-through
(no feature negotiated), NVMe Flush (opcode 0), AHCI FLUSH CACHE EXT; a driver without flush
(USB mass storage today) makes `fsync` fail → the updater fails closed.

**Updater** (`partctl`): config `partition_dir=/dev/ultar/part`,
`running_slot_file=/proc/ultar/boot_slot` (instead of `boot_dir`). Running slot = what the kernel
says (not `last_booted`); payload `kernel` → `ultar-kernel-<slot>`, `system` → `ultar-system-<slot>`
(role `boot` refused: the ESP is not updated through slots); payload larger than the partition →
`NoSpace`; every table/record write = `write_all_at` + `sync_all`. `PartitionBootControl::format`
(installer/tests; refuses a partition that already holds a valid table) and `loader_start` (the
loader's two shared functions, for tests and VMs without the loader).

## 5. What is measured / tested

| Check | Result |
|---|---|
| `ultar-slots` unit tests | 21 (table incl. all bit flips, `plan_store`, records, GPT damage/duplicates/geometry, options) |
| loader host tests (`cargo test --lib --target x86_64-unknown-linux-gnu`) | 10: 512/4096 B blocks, countdown + fallback durable, unrecordable trial not started, write cache lost before flush, torn write, start loop, rescue, backup GPT, missing/small BOOTCTL, PE length |
| `ultar-update` | all previous suites + config (1) + `partctl` unit (3) + `tests/partition_backend.rs` (8): full cycle, fallback after 3 attempts, failed health check, power loss at **every** checkpoint of apply+commit, torn table write, refusals, legacy layouts, slot records |
| `cargo test -p ultar-update -p ultar-slots` | 273 passed, 0 failed – 3 of 3 runs (on 6e4854d) |
| A/B gate in QEMU (`ab-test.sh`) | see §6 |
| Loader time | entry → `StartImage` (TSC ticks passed in the load options, converted by the kernel: `[boot] slot a, loader N us`). The gate asserts the **median** over all starts < 50 ms and prints min/max. QEMU TCG without KVM on a shared 2-core host: 10–45 ms per start (median ≈ 15–20 ms); this is emulation time – reading 2 MiB through emulated NVMe and the firmware's `LoadImage` dominate. Not measured on hardware or KVM. |

The gate (`AB-TEST: ALLES OK (n checks)`): real graphical installer (A/B layout) on NVMe → slot A,
raw rules (running slot `EBUSY`, other slot writable + fsync, table readable) → `ultar-updated` +
`uupdate offline` + `apply` with a `.uup` from a FAT stick → trial start of B → confirmation →
release C whose kernel panics: 3 restarts, then B again ("returned to the previous version" in the
history) → release D that starts but never confirms: 3 attempts, then B (history) → release E: power
loss 2 s after the daemon is up (staging) and 0.3/1.5/4 s after "Installing the update" (table valid,
A not startable or E complete, a valid slot starts), then E applied and confirmed in A → release F
pending in B: 6 power losses 0…300 ms after the firmware reports "starting Boot…" (the loader's
read–decide–write–flush window; a valid table after each) → the device starts the slot the table
names (F confirmed, or given up if the cuts spent its attempts) → loader time.

Gate runs on 6e4854d + this change (QEMU 8 TCG, OVMF 4M, 2 shared cores): `ab-test.sh` 3 of 3
`AB-TEST: ALLES OK` (44 checks each; 46 when no cut of E lands after its apply), 23–24 QEMU starts
each. Kernel gate `tools/kern-test/ci.sh` (`AB_TEST=0`, the A/B gate ran separately): 3 of 3 green
at `SMP=2` and 3 of 3 at `SMP=4` (18 checks each, incl. spawntest, weckertest and both
`ext4-durable.sh` runs). `ultar-check all`: ok, no baseline entry added (one `[[allow]]` in
`tools/check-layers.toml`, see §1). The updater daemon keeps exactly one kernel right, raw devices
(`ultar-updated.rs`), which the A/B partitions need; without it the service runtime drops it.

## 6. Open / not done

* **K5** (stage0 uses the running slot's verity root from the slot record) – not done; the record
  is written and readable (`PartitionBootControl::verity_root`).
* **K6**: no hardware watchdog after ExitBootServices (the firmware watchdog only covers the
  firmware phase; a kernel that *hangs* in a trial needs a manual restart, a kernel that *panics*
  restarts by itself). No rollback index in an EFI variable / TPM (`firmware_rollback_index` stays
  `None`).
* aarch64 UEFI: the loader is arch-neutral except `pe::MACHINE` and `ticks()` (CNTVCT_EL0) –
  not built or run for aarch64. A30 / vendor boot loaders: no backend.
* The loader's write-back cut: kill points are timed from the firmware's "BdsDxe: starting" line;
  whether a cut fell before, during or after the table write is not observable from outside, the
  gate checks the invariant (one valid copy, correct start) after each cut.
* (historic) The first patch was against a5eb384; against the then current main (`cfdf365`…) three hunks conflict
  (`.github/workflows/ci.yml`, `Cargo.lock`, `ultar-update/Cargo.toml` dependency list), the kernel
  hunks apply with offsets. `daemon.rs` there was moved onto the service runtime: the
  `BootStore` dispatch (`run` → `run_with`) has to be carried over by hand.
* `make-install-medium.sh` (A/B medium) was not run here (no `sgdisk` on this machine); the gate
  builds its medium with `gptimg.py` and runs the real installer on it.
* On Linux (`/dev/disk/by-partlabel` as `partition_dir`) the read-back after `fsync` may come from
  the page cache; only the Ultar kernel guarantees an uncached read-back.
* `flock` deviation: lock tied to the process, released at `LOCK_UN` or process end, not at the
  close of one descriptor; shared = exclusive; a blocking request returns `EWOULDBLOCK`.
* **Fixed – kernel ext4 durability:** the ext4 writer now builds extent trees of depth 1 (up to
  4 leaf blocks, checksummed: 1008 extents with 1 KiB blocks, 4080 with 4 KiB), allocates from
  BLOCK_UNINIT groups (bitmap derived like the Linux kernel: superblock backup, descriptors,
  flex_bg metadata, padding), flushes the device after every stored file, and never keeps a file
  in memory instead of the disk: `fsync`, `close` and `rename` return `ENOSPC`/`EIO`. Self-test
  `ext4-durable.sh` (1 KiB and 4 KiB blocks): free space fragmented, 64 MiB + 18 MiB written with
  fsync, hard stop, re-read byte for byte, extent tree checked, `e2fsck -fn` clean, a file larger
  than the free space fails with ENOSPC. Still not handled: depth ≥ 2 (more fragmented files fail
  honestly), meta_bg/sparse_super2 layouts (their uninitialised groups are not used). The 4 KiB
  workaround for `ultar-data` is removed.
* **Fixed – updater notice:** the "release did not start / failed its check" message is derived
  from the history (last entry `rolled back`) whenever the status is refreshed, so it survives the
  first automatic run and a daemon restart and ends with the next history entry
  (`tests/r4_boot_notice.rs`, checked to fail without the fix).
* vfork/posix_spawn release at exec is the kernel agent's fix (`kernel-process-wakeups.md`); this
  change keeps only `flock(2)` and the `spawntest` self-test (vfork + flock).
