# Integration: `ultar-devd` as a running service

Directory layout below `/home/claude/agents/devd/` mirrors the repository (`system/core/…`, `system/hal/…`,
`drivers/`), so every crate can be copied over its counterpart. `drivers/` is an unchanged copy (needed by tests).

## 1. What changed, crate by crate

| Crate | Change |
|---|---|
| `system/hal/ultar-hal-system` | **new** modules `hardware` (+ `class_names`, `device_info`, `kernel_log`, `sysfs`) and `kernel_drivers`; new dependency `ultar-driverpkg`; re-exports in `lib.rs`. Existing profile source untouched. |
| `system/core/ultar-devd` | pure logic kept; **new** `store::file`, `builtin`, `caps`, `bus::{sink,convert}`, `daemon`, `service`, `depot`, `msg`, `cli`, `runner`, bins `ultar-devd` + `udevctl`, `locales/{ar,en,de}.ftl`. New dependencies: `ultar-hal-system`, `ultar-bus`, `ultar-i18n` (dev: `ultar-profile`). |
| `system/core/ultar-bus` | **one new file** `src/messages/device_service.rs` (contracts of the bus service `devices`) + `pub mod` line and table row in `messages/mod.rs`. Nothing else touched. |
| `system/core/ultar-driverpkg` | four small additions: `payload::read_payload_file`, `VerifiedPackage::read_file`, `DriverIndex::empty`, `TrustedKeys::extend` (+ tests). |

Changes to the existing `ultar-devd` logic (all additive, old tests unchanged and green):
`LoadFailure::Unsupported` (`"unsupported"`), `AttemptOutcome::Unsupported` (passed over, **no quarantine**),
`DriverLoader::can_switch(&mut self, device, target) -> bool` (default `true`), `DevdError::Unsupported{device,driver}`
(`devd.error.unsupported`) returned by `switch_driver`/`roll_back` before anything is unloaded,
`DeviceManager::store_mut()`, `store::MAX_QUARANTINE = 4096` (oldest dropped), `store.rs` → `store/mod.rs`.

Layer rule: `system-core → system/hal/ultar-hal-system` is already allowed in `tools/check-layers.toml`.

## 2. What the Ultar kernel really exposes (read from `kernel/ultar-kern/src`, version 0.4)

> **Superseded by `kernel-device-interfaces.md`:** the six interfaces listed at the end of this section now exist
> (sysfs device tree `/sys/bus/{pci,usb,platform}`, driver register, `bind`/`unbind`/`enable`/`ultar_enabled`,
> `/sys/kernel/ultar/driver_abi`, uevents with `SEQNUM`, `ultar_probe`/`ultar_health`). The HAL reads only that
> tree now; the kernel-log parser (`hardware::kernel_log`, `hardware::class_names`) and `InventoryNote` are gone.
> The rest of this section describes the kernel before that change.

`procfs.rs` serves exactly `/proc/ultar/{verbund,firewall,sicherheit,geraet,anzeige,protokoll}`. **There is no device
file and no driver file.** Device and driver facts reach user space only through:

| Source | Kernel code | Content | When present |
|---|---|---|---|
| `/proc/ultar/protokoll` `[treiber] bb:dd.f vvvv:dddd -> name (origin) ok` / `… -> name FEHLER: text` | `driver.rs::bind_all` | devices a built-in driver **matched**; no class code | every boot |
| same file, `[pci] N Gerät(e) gefunden` | `main.rs` | PCI function count | every boot |
| same file, table `bb:dd.f vvvv:dddd  <class name>  <driver\|FEHLER\|->` (+ `Fehler: text`) | `selftest.rs`, console `geraete` | all PCI functions, class only as German **name** | self-test boot or console command |
| same file, `WERKSTATT {json}` | `werkstatt.rs::report` | driverless devices with exact class + subsystem | self-test boot or console `werkstatt` |
| same file, `name origin n Gerät(e)` | console `treiber` | complete driver register | only after the console command |
| same file, `[xhci] Controller …` / `[xhci] Port p: … vvvv:pppp` | `drivers/gen_xhci.rs` | USB devices, no class (keyboard/disk/hub implied) | every boot with xHCI |
| `/proc/ultar/geraet` `hersteller=`, `modell=`, `anzeige=`, `grafikN=vvvv:dddd Treiber name \| Firmware-Bildspeicher…` | `geraet.rs::text`, `grafik.rs::text` | machine model, which driver runs each graphics card | always |
| `/proc/version` contains `Ultar-Kern` | `procfs.rs` | kernel identification | always |

The log is a 64 KiB ring (`klog.rs`); once it wraps, boot lines are gone. The kernel has no `/sys`.

### Kernel interfaces that are missing (exact list for the kernel work)
1. **Device list** – e.g. `/proc/ultar/geraete`, one line per PCI function/USB device with address, ids,
   6-digit class, subsystem, bound driver, probe error. Without it a normal boot shows only bound devices, without class.
2. **Driver register** – e.g. `/proc/ultar/treiber` (`name origin devices`), today only printed by a console command.
3. **Bind/unbind/enable/disable of a built-in driver** – none exists: `bind_all` runs once, first match in `drivers::ALL`
   order; only `verbund` and `firewall` accept writes; no system call. Plan §5.5 stage A ("activation only via
   profile/rank") is therefore **not implemented in the kernel**.
4. **Kernel driver ABI number** – not exposed; `ultar-kern-abi-1` is assumed whenever `/proc/version` says `Ultar-Kern`.
5. **Device events (hot-plug)** – none; the daemon re-reads on `devices.rescan`.
6. **Per-device probe/health results** (link up, sector read) – the self test prints them per block device name, not
   linkable to a PCI function. Only the display path (`grafikN`) is usable.

## 3. Public API

### `ultar_hal_system::hardware`
- `HardwareReader::{running(), at(root), root(), device_info(), inventory()}`.
- `Inventory { devices: Vec<DeviceDescriptor>, machine, sources: Sources{sysfs_pci,sysfs_usb} }`.
  Addresses: `pci:bb:dd.f` (sysfs domain ≠ 0 kept: `pci:0001:02:00.0`), `usb:<sysfs name>` (`usb:1-2`). Devices sorted by address.
- `HardwareError::{NoSource, Io{path,kind}, TooManyDevices}` with `key()`. Files are read capped at 1 MiB, directories at 4096 entries.
- `device_info::KernelDeviceInfo::parse(text)` → `vendor`, `model`, `display`, `graphics`, `graphics_driver(v,d)`.
- `sysfs::{read_pci, read_usb, machine, pci_address}`.

### `ultar_hal_system::kernel_drivers`
- `KernelDrivers::read(&reader)` → `drivers()`, `driver(name)`, `exists(name) -> Option<bool>` (`None` = register not
  readable, absence proves nothing), `binding(address) -> BindState::{Bound{driver}, Failed{driver,error}, Unbound, Unknown}`,
  `register_complete`, `control: ControlInterface::{None, SysfsBind}`, `device_count()`, `health(address)`
  (`ultar_health` lines), `candidates(address)` (`ultar_drivers`), `bind_device(driver, address)`, `unbind_device(address)`;
  `KernelDriver{name, origin, bound, failed, unbind: Option<bool>, enabled: Option<bool>}`; `sysfs_pci_name(address)`.
- `kernel_abi(root)` (`ultar-kern-abi-<n>` from `/sys/kernel/ultar/driver_abi`, or `linux-<release>`), `ULTAR_KERNEL_ABI`;
  `uevent_seqnum(root)` (`/sys/kernel/uevent_seqnum`).

### `ultar_devd` (new parts)
- `store::file::FileStore::new(dir)` implements `ChoiceStore`; `take_events()`, `events()`, `saves()`, `path()`;
  `encode`/`decode`; `StoreEvent::Recovered{problem,kept}`, `StoreProblem::{Header,Length,Checksum,Body,UnsupportedVersion(n),TooLarge}`.
  File `state.toml`: header `# ultar-devd state v1 sha256=<hex> bytes=<n>` + `StoredState::to_toml()`. Write = temp
  file + `sync_all` + rename + directory sync. A damaged or newer file is moved to `state.toml.corrupt`, the daemon
  starts empty and reports it. Limits: file 4 MiB, 16 events, quarantine 4096 entries.
- `builtin::Builtin::new(reader, depot_dir, trusted)` implements `DriverLoader` **and** `HealthCheck` (clone it);
  `refresh()`, `set_trusted()`, `binding()`, `control_interface()`, `detail(address,&DriverKey) -> Option<LoadDetail>`, `forget()`.
  `LoadDetail::{PackageMissing, PackageRejected(code), WrongPackage, NoActivationRecord, BadActivationRecord,
  ProbeFailed(text), NotBound{kernel_driver,bound_to}, DeviceGone, Loaded{kernel_driver}}`.
- `bus::{BusSink, Publisher, MemoryPublisher, Published, UnservedDevice}`; `Publisher` is implemented for `ultar_bus::Client`.
- `daemon::{Daemon, DaemonConfig{root,depot,keys,state,kernel_abi}, StartError::{UnknownKernel,Hardware}, ScanResult}`:
  `Daemon::start(config, Box<dyn Publisher>)`, `list()`, `switch()`, `rollback()`, `clear_quarantine()`, `rescan(Option<&str>)`,
  `scan()`, `state()`, `state_saves()`, `capabilities()`, `kernel_control()`; `Daemon::register(&Client)`, `handle(request) -> Flow`, `serve(&endpoint)`.
- `depot::{load(depot,keys) -> DepotState{trusted,index,notices}, load_trusted_keys(dir)}`; `caps::{class_capability, hardware_hint}`;
  `msg::Messages`; `cli::{run, parse, execute, print_list, …}`; `runner::{run, parse}`.

### Bus contracts (`ultar_bus::messages::device_service`, service `devices`, all `VERSION = 1`)
`List → ListReply{devices: Vec<DeviceInfo>, notices: Vec<Text>, kernel_switch_supported}`, `Switch{address,driver,version}` /
`Rollback{address}` → `OperationReply{outcome: Outcome::{Done(Box<DeviceInfo>), Refused(Refusal)}, restored}`,
`ClearQuarantine{address,driver}` → `{released: Result<u32,Refusal>}`, `Rescan{address}` → `{devices: Result<Vec<DeviceInfo>,Refusal>, added, removed}`,
`Shutdown`. `Refusal{kind: RefusalKind, reason: Text}`; kinds `Unsupported, UnknownDevice, NotACandidate, Quarantined,
AlreadyActive, NothingToRollBack, WrongPhase, BadRequest, SystemUnreadable, Internal`.
Published topics (existing contracts): `devices.capabilities` (retained, complete set, one message per operation) and
`devices.driver_state` (`Active`, `Removed`, `Failed`, `Missing`; only changes).
Capability ids: profile names where they exist (`gpu`, `audio_out`, `wifi`, `bluetooth`, `modem`, `camera`, `sensors`, `npu`),
else the driver class (`net`, `storage`, `bus`, …). `Unavailable` + note `devd.capability.no-driver` = hardware present
(judged by class code) without working driver.

## 4. Behaviour of stage A (drivers built into the kernel)

"Load" = read the package from the depot, verify its signature against the trusted keys, check that it is the
indexed driver/version, read `builtin/<id>.toml` (`kernel_driver = "..."`, unknown keys rejected), and confirm
through the HAL that this kernel driver is bound to the device. If it is not, and the kernel offers
`bind`/`unbind` (`ControlInterface::SysfsBind`), the loader binds it – after unbinding the current driver, which is only
done when the kernel reports a teardown path (`ultar_unbind = supported`) – and reads the kernel's state again.

| Situation | Result |
|---|---|
| kernel driver bound to the device | active; capability of the driver class published |
| kernel printed `FEHLER` for this driver on this device | `load.init-failed`, quarantined, reason carries the kernel text |
| signature/structure/record invalid, other package under the file name | `load.package-rejected`, quarantined, reason carries the exact code |
| package file not on the system | skipped, no quarantine |
| package without activation record (loadable `.uko`, stage B) | `Unsupported` outcome, no quarantine, reason `devd.reason.not-loadable` |
| package's kernel driver not bound, kernel can bind it (driver exists, matches, current driver unbindable) | bound through sysfs, then confirmed from the kernel's new state |
| package's kernel driver not bound and the kernel cannot bind it (or the bind did not take) | `Unsupported` outcome, no quarantine, reason `devd.reason.not-bound` |
| kernel health of the device (`ultar_health`): `probe` not `ok` or `media=failed` | health check fails (`SelfTestFailed`) |
| kernel runs the device with a driver that has no package (`bruecke`, `gen-smbus`, virtio) | `NoDriver`, reason `devd.reason.kernel-only` |
| **switch / rollback** to a package for the **same** bound kernel driver (other version, machine-tuned variant) | **works** (no kernel call needed) |
| **switch** to a package for another kernel driver | works when the current driver can be unbound (gen-intel-eth, virtio-net); otherwise **`Unsupported`**, refused before anything is touched |

`unload` unbinds the package's kernel driver when it can be unbound safely. `ListReply::kernel_switch_supported` is
`true` when the kernel has the bind/unbind files. Between requests the daemon looks at `/sys/kernel/uevent_seqnum`
once per second and reads the hardware again when it changed (hot-plug, bindings changed by others).

## 5. How to wire it

```
ultar-devd --depot <dir> --keys <dir> --state <dir> --bus <socket> [--root <dir>] [--kernel-abi <name>] [--once] [--lang ar|en|de]
udevctl [--bus <socket>] [--lang ar|en|de] list | switch <address> <driver> <version> | rollback <address>
        | rescan [address] | clear-quarantine [address [driver version]] | shutdown
```
- `--keys`: directory with `*.keys` files (one hex public key per line). `--depot`: `*.utr` + `index.toml` + `index.toml.sig`
  (index must be signed by a trusted key; it is limited to the packages present).
- Start order: connect to the bus (waits up to 30 s for the broker) → register service `devices` → enumerate → resolve →
  manage → publish → serve. Exit codes: 0 clean stop, 1 start/bus failure or refused operation, 2 usage.
- `--once`: no bus; prints the device list and exits (first check on a real machine).
- **Stop:** `udevctl shutdown` (bus method `devices.shutdown`). `std` has no signal API and the crate forbids `unsafe`, so
  SIGTERM is **not** handled: it ends the process immediately. That is safe (every change is on disk by atomic replace
  before the reply), but nothing is announced. The init service entry should use `udevctl shutdown` as stop command.
- Bus policy needed: daemon identity `register = ["devices"]`, `publish = ["devices.capabilities", "devices.driver_state"]`;
  panel `call = ["devices.*"]`, `subscribe = ["devices.*"]`.
- Settings panel: subscribe to both topics, call `devices.list`; show `reason`/`notices`/refusals through their keys
  (catalogue: `system/core/ultar-devd/locales/*.ftl`, positional arguments are `{a}`, `{b}`; an argument of kind `Key`
  may be a `driverpkg.*` code from `ultar_driverpkg::msg`).
- Default bus socket of `udevctl`: `/run/ultar/bus/system.sock`.

## 6. Test results (rustc 1.97, `-j1`, full gate run three times: `gate.sh`, log `gate.log`)

`cargo fmt --check` clean and `cargo clippy --all-targets -- -D warnings` clean for all four crates.

| Crate | `cargo test` ×3 | Breakdown |
|---|---|---|
| `ultar-hal-system` | **24 passed, 0 failed** each run | 9 unit (5 existing + 4 new) · `hardware` 10 · `kernel_drivers` 5 |
| `ultar-devd` | **100 passed, 0 failed, 1 ignored** each run | 31 unit · `builtin` 5 · `e2e_machines` 7 · `e2e_faults` 10 · `e2e_bus` 4 · `e2e_tools` 6 · `service` 1 · `file_store` 5 · `events` 5 · `manual` 12 · `probe` 12 · `store_faults` 2 · `bench` 1 ignored |
| `ultar-bus` | **141 passed, 0 failed, 1 ignored** each run | as before + 2 for `device_service` |
| `ultar-driverpkg` | **79 passed, 0 failed** each run | as before + 3 |

`ultar-check` (repository tool, run on this tree with the repository's layer table): `size`, `unwrap`, `paths`, `unsafe`,
`text`, `layers`, `linux-calls` clean for the four crates; the only finding is the pre-existing `edition` hit in
`ultar-hal-system/src/lib.rs:27`. Largest file 461 lines; no function over 60 lines; no struct over 15 fields.

Required end-to-end cases (all with a fake system root and a depot packed and signed in the test with a throwaway key):
- (a) `e2e_machines`: Beelink on Ultar kernel texts (self-test boot): 7 active (2× gen-ixgbe, 2× nvme, 2× gen-xhci,
  gen-hda), 3 with package but not loadable (display, WLAN, Bluetooth), 4 without package; capabilities
  `audio_out, bus, net, storage` available, `gpu, npu, wifi` unavailable; one retained message, 14 driver states.
  Beelink after a normal boot: only the 7 bound devices + 3 USB, class-matched drivers not resolvable (reported).
  ThinkPad on a sysfs tree: 26 devices, 6 active, 4 failed (2 not loadable, Bluetooth, cAVS audio "not bound"),
  16 driverless of which 4 are "kernel-only".
- (b) `e2e_faults`: manifest byte changed inside a signed package → `driverpkg.signature.invalid`, both NVMe drives
  driverless with reason, quarantine on disk, `storage` unavailable; repaired package + rescan → active again.
  Also: swapped package, package signed by a foreign key, missing file, tampered index, missing/garbage keys.
- (c) manual choice (`nvme 1.0.0` instead of `1.1.0`) and roll-back target survive restarts; second start writes nothing.
- (d) truncated / garbage / bit-flipped / newer-version state file → daemon works, notice, damaged file kept, valid
  file rewritten, next start clean; state directory unusable → runs in memory with notice.
- (e) `e2e_bus`, `e2e_tools`: real in-process broker; client lists, switches (success within the bound kernel driver,
  `Unsupported` otherwise, typed refusals), subscriber gets states and the retained set; `udevctl` in ar/en/de; the two
  **binaries as real processes** (start, list, switch, refused switch, shutdown with exit code 0).

Measured (release build, default profile, shared 2-core machine, `cargo test --release --test bench -- --ignored --nocapture`):
start with remembered choices incl. signature verification of every used package: **2.1 ms** (Beelink, 14 devices,
kernel log) and **2.7 ms** (ThinkPad, 26 devices, sysfs). Binaries unstripped with the standalone default release
profile: `ultar-devd` 2.8 MB, `udevctl` 1.0 MB (the workspace profile `opt-level = "s"`, LTO, strip will be smaller).

## 7. Real kernel interface versus fixtures – honest statement

**Update (kernel device interfaces):** `tools/kern-test/devd-qemu.sh` (part of `ci.sh`) runs the static `ultar-devd
--once` on the Ultar kernel in QEMU: it reads the kernel's sysfs tree and driver ABI, finds a signed test package for
the e1000 whose driver was unbound before, binds `gen-intel-eth` through `/sys/bus/pci/drivers/gen-intel-eth/bind` and
lists the device as working; the kernel then reports it bound. The fixtures (`kernel_fmt.rs`, `sysfs_tree.rs`) now write
the kernel's sysfs format (`devices/sysfs.rs`) instead of log texts. The paragraphs below describe the state before.


- **Nothing was run on the Ultar kernel** (no QEMU run, no real machine). The Ultar-side parsers were written from the
  kernel's formatting code; the test texts are produced by `tests/common/kernel_fmt.rs`, which copies those format
  strings and the `matches()` rules of `drivers::ALL` by hand. If the copy and the kernel differ, the tests would not notice.
  The only text taken from a real run is the QEMU device table in `docs/legacy/TREIBER.md`
  (`tests/fixtures/protokoll-qemu-geraete.txt`; its `[treiber]`/`[xhci]` lines are again built from format strings).
- **On an Ultar kernel as it is today, after a normal boot,** the daemon sees only devices a built-in driver matched,
  without class code. Packages matched by id work (gen-ixgbe, gen-intel-eth, gen-hda by id); packages matched only by
  class (`nvme`, `gen-xhci`, `ahci`) are **not** found and the device is listed as "kernel-only"; driverless devices are
  invisible. The full picture exists only after a self-test boot or the console commands. This is reported through
  notices, and it needs interfaces 1 and 2 of §2 to be fixed.
- The sysfs reader was additionally run once by hand against the real `/sys` of the build container
  (`ultar-devd --once --root /`, Linux 6.18, 10+ virtio PCI functions listed with their `virtio-pci` binding).
  The ThinkPad sysfs fixture combines a Linux-style tree with Ultar driver names – a combination no kernel offers
  today; it tests the reader and the loader logic, not a real system.
- The class codes of the two machine fixtures are the assumptions already documented for `ultar-driverpkg`.

## 8. Not done / not tested

- Switching between kernel drivers works only where the current driver has a teardown path (today gen-intel-eth,
  virtio-net); enabling/disabling drivers (`ultar_enabled`) is not used by the daemon.
- Hot-plug: the daemon polls the kernel's event counter once per second (no netlink socket: the crate has no `unsafe`
  and no libc); no periodic health re-check; devices already active are not re-validated on `rescan`.
- Health check = binding + the kernel's `ultar_health` (`probe`, `media`) + display path; link state is not judged.
- USB on the Ultar kernel: only the xHCI log lines, one controller counter, class mostly unknown; never checked on hardware.
- `DriverPhase::Loading` is never published (loading is synchronous inside one request).
- Capabilities carry presence only (no counts, DPI, link speed). Capabilities without a driver class (battery, touch …)
  are not this daemon's.
- Retained `devices.capabilities` is not re-published after a broker restart (the client reconnects, but the daemon
  gets no reconnect event); a `rescan` that changes the set or a daemon restart repairs it.
- SIGTERM handling (see §5). A bus that never appears ends the daemon after 30 s with exit code 1 (not covered by a test).
- Firmware update, workshop blueprint export, loadable modules (stage B): not started.
- Remembered choices are bounded only by the 4 MiB file limit (a save beyond it fails with `devd.store.write-failed`
  and the daemon continues in memory); no pruning of devices that never return.
- The package is verified at load time only; nothing watches the depot for later replacement (harmless in stage A,
  must be revisited for stage B).
- Static/musl/aarch64 build not run (only `x86_64-unknown-linux-gnu` installed); no new C or `-sys` dependency, no `unsafe`.
- Not covered by tests: `HardwareError::TooManyDevices` (needs > 4096 directory entries), write failures of `udevctl`
  output, `Messages::new` fallback for an unparsable embedded catalogue (guarded by a test that they parse).
- Arabic and German texts were written by the agent, not reviewed by a native speaker. Driver display names
  (`driver.<id>.name`) are still keys without texts.
- No `[workspace]` tables were added (the copies come from the integrated repository and build standalone here because no parent workspace exists); the `Cargo.lock` files in the crate folders stem from those standalone builds and can be dropped. `target/` (build output), `gate.sh` and `gate.log` are not deliverables.

## 9. Seat device nodes (`ultar-devd-seat`, compositor review N14)

A second program of this crate, run as the service `seat` (`rootfs/etc/ultar/services/seat.toml`): root with
the single right `file_admin`, no bus, reads `/sys` and `/etc/ultar/devd-seat.toml`, writes only seat nodes and
`/run/ultar/seat/modes`. It is apart from the daemon so the daemon keeps running as user 804 without rights.

| Node | Owner:group mode |
|---|---|
| `/dev/input/event*` of hardware | `root:seat` (812) `0660` |
| `/dev/input/event*` of a software device (sysfs below `/devices/virtual/`) whose `name` is in `virtual_allowed` | `root:seat` `0660` |
| any other software device, or an event node without a sysfs entry | `root:root` `0600` (fail closed) |
| `/dev/dri/card*`, `/dev/fb*` | `root:seat` `0660` |
| `/dev/uinput` | `root:root` `0600` |

* When: every node at start (after subscribing to device events, so nothing added in between is missed), then
  each `add`/`change`/`bind` event naming a seat node; lost events (`ENOBUFS`) → full scan. After giving an
  input node to the seat it writes `change` into the node's sysfs `uevent`, so the compositor (which rescans on
  that event) tries it again. Event contents only trigger a new evaluation; the decision comes from sysfs.
* Safety: only character devices, never through links, only below `/dev` directories of root that nobody else
  may change; the mode is narrowed first, then owner and group, then the final mode (no step grants more than
  start or end); every change is read back.
* Kernels: on Linux and on the Ultar kernel (device node modes, `kernel-compositor-gaps.md` §4) owners and modes
  are kept (`modes` = `kept`); the compositor's group check then applies and its `kernel-gated` relaxation is
  unused. Older Ultar kernels refused `chown`/`chmod` below `/dev` (they gated the nodes by `raw_device` only):
  detected from that refusal on the Ultar kernel only (`kernel-gated`); a refusal on Linux, or any other error,
  is `failed` – the compositor then keeps its group check and opens nothing (fail closed). `--once` prints one
  line per node and the verdict.
* Tests: `src/seat/tests.rs` (configuration bounds, classification, real nodes via `mknod` as root, links and
  writable directories refused, verdict, state file, event parsing incl. 20 000 random messages with a fixed
  seed); `tests/seat.rs` (the program: `--once`, and as a service a node added later is set after a device
  event sent over netlink – hot-plug on Linux); `tools/kern-test/devd-qemu.sh` (Ultar kernel in QEMU:
  `fb0`, `input/event0`, `input/event1` set to `root:812 0660`, verdict `kept`). The Ultar kernel gives a USB
  keyboard its own node from `event2` on with `add`/`remove` events (`geraete-hotplug.py`); not tested there: the
  service reacting to such an event, a real DRM card, a real uinput device (the Ultar kernel has neither).

