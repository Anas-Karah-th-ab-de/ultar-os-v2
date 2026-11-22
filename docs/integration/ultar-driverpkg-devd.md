# Integration: `ultar-driverpkg` + `ultar-devd`

Implements plan §5 (driver packages, depot in the installation medium, runtime device daemon) and the
resolution of the two test machines from `docs/DRIVER-TARGETS.md`.

| Crate | Area (§3.0) | Suggested place | Kind |
|---|---|---|---|
| `ultar-driverpkg` | 3 system core (tooling for area 2) | `system/core/ultar-driverpkg` | library + bin `udrv` |
| `ultar-devd` | 3 system core | `system/core/ultar-devd` | library only (no I/O, no paths) |

Both carry an empty `[workspace]` table (remove on integration). `ultar-devd` depends on
`../ultar-driverpkg` by path. Dependencies of `ultar-driverpkg` are the ones of the old `ultar-pkg`
(`ed25519-dalek 2`, `sha2 0.10`, `rand 0.8`, `base64`, `hex`, `tar` without default features,
`flate2` rust backend, `serde`, `toml 0.8`); `ultar-devd` adds only `serde` + `toml`.
No own cryptography, no `unsafe` (`#![forbid(unsafe_code)]`), no `-sys` crates in the tree.

## 1. `ultar-driverpkg`

### Package format `.utr`
Outer tar (uncompressed) with exactly three entries – anything else is rejected:

```
ultar-driver.toml   manifest
payload.tar.gz      module / firmware / HAL configuration
sig.ed25519         "<key id>\n<base64 signature>\n"
```
Signed message: `"ultar-driver-v1\n<sha256(manifest)>\n<sha256(payload)>\n"`.
Key id = first 16 hex digits of SHA-256 of the public key (same as `.uap`).

### Manifest (English keys)
```toml
id = "gen-intel-eth"            # [a-z0-9][a-z0-9_-]{0,63}
version = "1.2.0"               # semver, own parser/ordering (pre-release supported)
class = "net"                   # display input audio net wifi bluetooth storage modem camera sensor platform bus security other
kernel_abi = ["ultar-kern-abi-1", "linux-6.8.0-*"]   # trailing * allowed
origin = "workshop"             # hand | workshop | vendor
rank = 50                       # -1000..=1000, default 0
name_key = "driver.gen-intel-eth.name"        # i18n KEY, literal text is rejected
description_key = "driver.gen-intel-eth.description"   # optional
[match]
pci = ["8086:0d4c", "8086:*"]   # vendor:device, device may be *
pci_class = ["0c0330"]          # 2/4/6 hex digits, prefix match
usb = ["8087:0026"]
usb_class = ["e00101"]
dt = ["samsung,decon"]
acpi = ["PNP0C0A"]              # compared upper-case
dmi = ["ThinkPad P1*"]          # machine model globs; restricts AND prefers the driver
platform = ["ps2"]              # named devices without bus id
[content]
module = "drivers/gen_intel_eth.uko"
firmware = []
hal_config = ""
```
Unknown keys, duplicates, bad patterns, absolute/`..` content paths, missing match rule or content are
typed `ManifestError`s. Content paths must exist in the payload (checked at pack **and** verify).

### Public API (re-exported from the crate root unless a module is named)
- `keys`: `PrivateKey::{generate, from_hex, to_hex, public, sign}`, `PublicKey::{from_hex, to_hex, key_id, verify}`,
  `TrustedKeys::{new, parse, insert, find, len, is_empty}`, `sha256_hex`.
- `sign`: `package_message`, `index_message`, `SignatureBlob::{create, parse, encode, check}`,
  `SignatureError::{Malformed, NoTrustedKeys, UntrustedKey{key_id}, Invalid}`.
- `package`: `pack(manifest, payload, &key, &limits) -> Vec<u8>`, `read_package`, `verify(bytes, &trusted, &limits)
  -> VerifiedPackage` (`.manifest`, `.key_id`, `.entries`, `.payload()`, `.unpack(dest)`), `info`, `assemble`,
  `Limits` (manifest 64 KiB, payload 256 MiB, signature 1 KiB, unpacked file 256 MiB / total 1 GiB, 4096 entries),
  `PackageError` – distinct variants: `Unsigned`, `Signature(UntrustedKey)` (foreign key),
  `Signature(Invalid)` (tampered), `AbsolutePath`, `PathTraversal`, `UnsafeEntryType` (links, devices),
  `TooLarge{part, limit}`, `TooManyEntries`, `DuplicateEntry`, `UnexpectedEntry`, `MissingContent`, …
- `payload`: `build_payload(dir)` / `build_payload_from(files)` (reproducible), `list_payload`, `unpack_payload`
  (validates everything first, writes nothing on error, never writes through symlinks), `normalize_path`.
- `manifest`: `Manifest::{parse, from_bytes, supports_kernel}`, `MatchRules`, `Content`; `DriverClass`, `Origin`.
- `hardware`: `HardwareId::{Pci{vendor,device,class,subsys}, Usb{vendor,product,class}, Dt, Acpi, Platform}`,
  `DeviceDescriptor{address, primary, aliases, label, machine}` + `identity()`.
- `matching`: `candidates(&device, &index, kernel_abi) -> Vec<Candidate>`. Order: exact id > vendor wildcard >
  class (longer prefix first) → machine-tuned (`dmi`) → rank → version → id → file. Fully deterministic.
- `index`: `DriverIndex::{new, parse, parse_verified, to_toml, sign, entries, get, find, hits, retain_files}`,
  `IndexEntry::{from_package, check_file}` (file name, sha256, size, key id, manifest). Hash-map lookup per
  bus (exact id / vendor / class prefix) and per name space.
- `depot`: `scan_directory`, `write_signed_index`, `load_signed_index` (files `index.toml` + `index.toml.sig`,
  message `"ultar-driver-index-v1\n<sha256(index)>\n"`), `resolve(devices, index, abi) -> Plan`
  (`devices[].{chosen, alternatives}`, `driverless()`, `blueprint_requests()`),
  `select_for_install(&plan, all) -> InstallSelection{packages, index_files}`.
- `hwlist::parse_hardware_list` – text format for `udrv match` (documented in the module).
- `cli::run` / `msg` – `udrv` and its ar/en/de tables (Arabic first, `ULTAR_LANG`).
- Every error implements `Coded::code()` (stable key such as `driverpkg.package.unsigned`); `Display` prints
  code + technical detail, user text comes from `msg::text(lang, code)`.

### CLI
```
udrv keygen
udrv pack <manifest> <payload-dir> <out.utr> <privkey>      # privkey: file with hex, or hex
udrv verify <file.utr> <trusted-keys-file>
udrv info <file.utr>                                         # display only, no trust
udrv index <dir> <privkey> [--trusted <keys-file>]
udrv match <index.toml> <hardware-list> [--abi <abi>] [--trusted <keys-file>]
```
`match` without `--trusted` prints a warning that the index signature was not checked.

### How other crates use it
- **Installer:** HAL inventory → `Vec<DeviceDescriptor>` → `load_signed_index(depot, trusted)` → `resolve` →
  show `Plan` (device · driver · alternatives; `blueprint_requests()` for "create workshop blueprint") →
  `select_for_install(plan, all)` → copy files, checking each with `IndexEntry::check_file`.
- **Installed system / loader:** `load_signed_index` → `index.retain_files(&present_packages)` → hand to
  `ultar-devd`; before loading a module: `package::verify(bytes, trusted, limits)?.unpack(dir)`.
- **Workshop:** writes the manifest next to the generated driver; `udrv pack`.
- **Kernel stage A (§5.5):** the built-in driver register can reuse `Manifest`/`candidates` for activation by rank.

### Examples and real machines
`examples/manifests/`: `nvme`, `gen-xhci`, `ahci`, `gen-intel-eth` (incl. 8086:0d4c), `gen-ixgbe` (8086:57b0),
`gen-hda`, `intel-display-gen9` (8086:9bc4), `intel-display-xe` (8086:7d51), `intel-wifi` (06f0, 272b),
`intel-bt-usb` (8087:0026, 8087:0036). `tests/fixtures/*.hw` are the two inventories; `tests/machines.rs` asserts:

- ThinkPad P1 Gen 3: 10 devices with driver (nvme, 3× gen-xhci, gen-intel-eth, intel-display-gen9, intel-wifi,
  intel-bt-usb, 2× gen-hda), 16 driverless (host bridge, LPC, NVIDIA GPU, UCSI, Thunderbolt controller + switch,
  2× I2C, SMBus, Genesys reader, thermal, MEI, SPI, AMT-SOL, camera, fingerprint).
- Beelink GTi15 Ultra: 10 with driver (2× gen-ixgbe, 2× nvme, 2× gen-xhci, intel-display-xe, gen-hda, intel-wifi,
  intel-bt-usb), 4 driverless (O2 Micro reader, NPU, fingerprint, USB audio).

## 2. `ultar-devd`

Pure runtime logic; everything external is a trait the daemon binary / HAL implements:

```rust
trait DriverLoader   { fn load(&mut self, &DeviceDescriptor, &Candidate) -> Result<(), LoadError>; fn unload(..); }
trait HealthCheck    { fn check(&mut self, &DeviceDescriptor, &Candidate) -> HealthVerdict; }  // Healthy(caps) | Unhealthy(f) | DeviceGone
trait ChoiceStore    { fn load(&mut self) -> Result<StoredState, StoreError>; fn save(&mut self, &StoredState) -> ..; }
trait CapabilitySink { fn emit(&mut self, CapabilityChange); }                                  // → ultar-bus
```
`DeviceManager::new(Config::new(kernel_abi), index, loader, health, store, sink)`, then:

- `handle_event(DeviceEvent::Added(desc) | Removed(address)) -> Result<ProbeReport, DevdError>`
- settings panel: `devices() -> Vec<DeviceView>` (device, phase, active driver, capabilities, alternatives with
  quarantine reason, roll-back target), `switch_driver(address, &DriverKey)`, `roll_back(address)`,
  `clear_quarantine(&QuarantineFilter::{All, Device, Driver})`, `rescan(address)`, `replace_index(index)`,
  `quarantine()`, `state()`, `phase(address)`, `store_error()`.
- `ProbeReport`: phases entered, every attempt with outcome, active driver, `used_remembered`,
  `remembered_failed`, `restored`, `store_error`.
- `StoredState::{to_toml, from_toml}` is the on-disk form (choices, previous, quarantine); `MemoryStore` included.

State machine (`fsm::transition`, total over 6 phases × 7 signals, table-tested):
`Detected →(CandidatesFound) Probing →(ProbeSucceeded) Active`; `Detected →(NoCandidates) NoDriver`;
`Probing →(CandidatesExhausted) Failed`; `Active →(SwitchRequested) Probing`; `NoDriver|Failed →(Rescan) Detected`;
any → `Removed` on `Unplugged` (terminal).

Behaviour decisions worth knowing:
- Remembered driver is tried first and alone if it works (store is then not rewritten); if it fails it is
  quarantined, the search continues and the store is corrected (same principle as the old `hw.rs`).
- Quarantine is per device identity (`address=primary id`) **and** driver version; reason is typed
  (`load.init-failed`, `health.no-response`, …). A new driver version is tried again.
- `LoadFailure::PackageMissing` (package not copied to this system) skips without quarantine.
- Hot-unplug during load or health check: device → `Removed`, the driver is unloaded, failures recorded during
  that probe are discarded (they say nothing about the driver), the remembered choice is kept.
- `Config::last_resort_retry` (default on): when every regular candidate failed, quarantined drivers get one
  retry; one that works leaves quarantine. Prevents a transient fault from leaving a boot device driverless.
- Failed manual switch: requested driver is quarantined, previous driver restored (`restored = true`);
  if that fails too the remaining candidates are tried.
- Store errors never stop the daemon: decisions hold in memory, `store_error` reports it, the next
  successful save writes everything.

## 3. Test results (run with `-j1`, toolchain 1.97)

| Crate | `cargo fmt --check` | `cargo clippy --all-targets -- -D warnings` | `cargo test` |
|---|---|---|---|
| `ultar-driverpkg` | clean | clean | **76 passed, 0 failed** (26 unit + cli 7, depot 8, machines 3, manifest 9, matching 7, messages 1, package 12, payload 3) |
| `ultar-devd` | clean | clean | **41 passed, 0 failed** (12 unit + events 5, manual 11, probe 11, store_faults 2) |

Size rules checked by script over all `.rs` files including tests: no file > 500 lines, no function > 60 lines,
no struct > 15 fields; no `unwrap/expect/panic!` and no `"/sys/"`/`"/dev/"` outside `#[cfg(test)]`.
`#![deny(missing_docs)]` is on in both crates.

## 4. Open points (honest list)

1. **Static build not verified here** – only `x86_64-unknown-linux-gnu` is installed (no musl / aarch64 target).
   The dependency tree has no C library or `-sys` crate, so it should build static; CI must confirm.
2. **Class codes in the fixtures are assumptions.** The two `hardware.md` files list vendor:device ids only
   (except NVMe 01:08:02). I used the standard class code per device type; the audio controllers are
   8086:06c8 = `040100` and 8086:7728 = `040300` by assumption, which is why `gen-hda` also matches both by id.
   The first self-test run on the machines should replace the fixtures with real data.
3. Not in the fixtures: ThinkPad externals (DisplayLink dock, hubs, peripherals), Beelink Thunderbolt DMA
   functions (inventory gives no ids) and the ASMedia hubs (served by the xHCI bus driver).
4. Firmware file names in `intel-wifi` / `intel-bt-usb` are placeholders until the firmware is packaged.
   Neither machine has AHCI; the `ahci` manifest is tested with a synthetic device.
5. **Differences to the plan text §5.1** (plan should be updated): manifest keys are English and the file is
   `ultar-driver.toml` (plan shows German keys / `ultar-treiber.toml`); `origin` has no `linux-modul`
   (task says hand|workshop|vendor); added `usb_class`, `platform`, `name_key`, `description_key`; the index
   signature is a separate file `index.toml.sig`.
6. `msg` tables live inside the crate; move to the i18n crate when available. The driver display-name keys
   (`driver.<id>.name`) are only keys – their ar/en/de texts must be added to the i18n catalogue.
7. Subsystem ids (`subsys`) are carried in `HardwareId::Pci` but no manifest rule matches on them yet.
8. The hardware-list text format is defined by this crate (no existing format found); the HAL inventory
   may emit it or build `DeviceDescriptor`s directly.
9. `ultar-devd` is logic only. Missing and not started: the daemon binary, real `DriverLoader`/`HealthCheck`
   backends, a file-backed `ChoiceStore` (atomic write of `StoredState::to_toml`), the `ultar-bus` adapter for
   `CapabilitySink`, periodic health re-checks of active drivers, firmware update and blueprint export from the
   panel. Calls are synchronous; a slow load blocks the manager.
10. Not covered by tests: write-failure paths of `write_signed_index`, `CliError::Output`,
    `IndexError::Encode`, `PackageError::Archive` while assembling, `DevdError::Transition` and
    `DevdError::WrongPhase` from `switch_driver` (both unreachable through the public API today;
    `WrongPhase` is covered via `rescan`). `Lang::from_env` is covered only through `from_tag` and a manual run
    of the binary.
