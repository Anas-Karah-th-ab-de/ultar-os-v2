# Integration: `ultar-profile` and `ultar-hal`

Both crates belong to area 3 (system core). Target folders: `system/kern/ultar-profile`
and `system/hal/ultar-hal`. Each `Cargo.toml` has an empty `[workspace]` table to build
standalone; remove it on integration. `ultar-hal` depends on `ultar-profile` through
`path = "../ultar-profile"` (adjust the path).

## 1. `ultar-profile` (replaces `ultar-edition`)

Pure logic, no hardware paths. Dependencies: `serde` (derive), `toml` 0.8 (`parse`, `display`).

### Public API

| Item | Purpose |
|---|---|
| `Edition { Phone, Desktop, Server }` | Build/service preset only. `parse` (old aliases `handy`, `mobile`, `tablet`, `laptop`, `pc` kept), `as_str`, `key`, `label(Lang)`, `has_ui`, `matches(&[S])` |
| `resolve_edition(cmdline, env, file, detected)` | Order: `ultar.edition=` (last valid wins) > env > file (first non-comment line) > detected > `Phone` |
| `FormFactor { Phone, Tablet, Laptop, Desktop, Server, Board }` | `parse`, `as_str`, `key`, `label`, `edition_hint`, `from_edition`, `is_handheld` |
| `form_factor_from_chassis(u8)` | SMBIOS type 3 table (lock bit ignored) |
| `detect_form_factor(&DetectInputs)` | Kernel info > chassis > device tree + battery (tablet from 178 mm diagonal) > device tree alone (`Board`) |
| `edition_from_kernel_info(&str)` | Reads `edition=` / old `ausgabe=` of the Ultar kernel description |
| `Capability` (19 values), `CapabilitySet` (u32 bit set) | `of`, `all`, `with`, `insert`, `remove`, `set`, `contains`, `contains_all`, `intersects`, `union`, `missing_in`, `iter`; serialized as a list of names |
| `Capabilities { flags, display_count, memory_mb, cpu, primary_display }` | 5 fields; `CpuTopology { performance_cores, efficiency_cores, simd: SimdLevel }`, `DisplayGeometry { width_px, height_px, width_mm, height_mm, dpi }` |
| `DeviceProfile { device_id, form_factor, edition, capabilities }` | `to_toml`, `from_toml`, `from_toml_for_device` (rejects another device/kernel) |
| `device_id(model, kernel_id)` | `"<model> \| <kernel>"`, same format as the old `hw.rs` |
| `Mode { Mobile, Desktop, Headless }`, `ModePreference { forced, auto_switch }` | `choose_mode(&profile, &pref)` |
| `requirements_met(&CapabilitySet, &Capabilities)` | For app/panel/service manifests |
| `PanelId` (25 panels), `panel_rule`, `visible_panels(&Capabilities)` | Dynamic settings; keys `settings.panel.*` |
| `ProfileSource` trait, `MemorySource`, `detect(&dyn ProfileSource)` | Real source belongs to the HAL backend |
| `ProfileError`, `msg::{Lang, text, keys}` | Typed errors; ar/en/de table, Arabic first |

### Behaviour kept from `ultar-edition`

All three old tests are carried over (`edition::tests::source_order`, `setting_beats_detection`,
`service_list_matching`, `form_factor::tests::old_detection_cases`). Mapping of old names:
`resolve`/`resolve_auto` → `resolve_edition`, `from_chassis` → `form_factor_from_chassis` +
`edition_hint`, `erkennen` → `detect_form_factor`, `detect()` → `detect(&source)`.

Deliberate differences:
- `label` takes `msg::Lang` instead of a string.
- No global `current()` cache and no file reading: the caller owns the source and the profile.
- Chassis 8/9/10/14/31/32 are now `Laptop`, 30 is `Tablet` (edition hint unchanged).
- Device tree without battery is `Board` with **no** edition hint, so the default stays
  `Phone` exactly as before.

### `choose_mode` rules (first match wins)

1. `forced` mode of the user. 2. Server form factor or edition → `Headless`.
3. Phone/tablet with external display or keyboard + pointer → `Desktop` if `auto_switch`, else `Mobile`.
4. Laptop with touch and no keyboard → `Mobile` if `auto_switch`, else `Desktop`.
5. Desktop → `Desktop`. 6. Board: no display → `Headless`; touch without keyboard → `Mobile`; else `Desktop`.

## 2. `ultar-hal`

Traits only. `#![forbid(unsafe_code)]`. Feature `mock` (off by default, switched on for
this crate's own tests through a self dev-dependency).

### Traits (all object-safe, `Send + Sync`, methods take `&self`)

`Display`, `Renderer`, `Input`, `Audio` (+ `AudioStream: Send`), `Power`, `Network`, `Wifi`,
`Bluetooth`, `Radio`, `Camera` (+ `CameraStream: Send`), `Sensors`, `Gnss`, `Haptics`, `Leds`,
`Storage`, `Tpm`.

Common types: `HalError` (9 variants, `Copy`, `key()`, `is_transient()`), `HalResult<T>`,
`Rect`, `Size`, `PixelFormat`, `Rotation` (incl. the old `phys_to_logical` mapping), `Level`
(permille, for brightness/volume/strength), ids (`DisplayId`, `InputDeviceId`, `EndpointId`,
`CameraId`, `SensorId`, `LedId`, `BlockDeviceId`), events (`DisplayEvent`, `InputEvent`).

### Backend selection

- `Backend`: `name()`, `probe()`, `system() -> SystemSummary`, one `Option<&dyn Trait>` per
  class (default `None`), `provides(DeviceClass)`. Implemented for `Arc<T>` as well.
- `ChoiceStore` trait (`recall`, `remember`, `forget`) + `MemoryChoiceStore`
  (`from_pairs`, `pairs`, `writes`).
- `BackendSet::select(candidates, &mut store)`: per class the remembered backend first and
  alone; otherwise the candidates in order; result written back. Each candidate is probed at
  most once, unneeded candidates are not probed. `reselect(class, &mut store)` probes afresh
  for one class. `chosen`, `selection`, `selections`, `candidate_names`, `store_error`.
  `BackendSet` itself implements `Backend`.
- `capabilities_of(&dyn Backend) -> ultar_profile::Capabilities`. A class counts only if
  usable (no touch screen → no touch, no battery → no battery, modem absent → no modem);
  a class that answers with an error counts as absent. NPU and fingerprint come from
  `SystemSummary::extra` because they have no trait.

### Mock backend (`ultar_hal::mock`)

`MockBackend` with presets `phone()`, `laptop()`, `server()`, plus `empty`, `full`,
`with_classes`, `named`, `set_probe_result`, `probe_count`, `set_class`, `set_system`.
Devices are public fields: `ui.{display, renderer, input, audio, camera, haptics, leds}`,
`links.{network, wifi, bluetooth, radio, gnss}`, `base.{power, sensors, storage, tpm}`.
Every device has `fail_next(HalError)` (one-shot) and scripting calls, e.g.
`ui.display.plug/unplug`, `ui.input.inject/inject_tap/inject_key/add_device`,
`links.wifi.add_network`, `links.radio.set_modem_state`, `base.power.set_battery`.
Clocks and "random" numbers are fixed, so runs are reproducible.

## 3. How other crates use it

- **devd**: builds the candidate list (device backend first, generic last), implements
  `ChoiceStore` on the stored device file (bound to `DeviceProfile::device_id`), calls
  `BackendSet::select`, then `capabilities_of(&set)` and `ultar_profile::detect` with a
  `ProfileSource` from the backend; publishes the `DeviceProfile` on the bus and repeats
  `capabilities_of` on hotplug events.
- **Services** (net, audio, power, radio …): take `&dyn Backend` and use `backend.wifi()` etc.
- **Platform**: `choose_mode(&profile, &pref)` after every profile change.
- **settingsd / app installer**: `visible_panels`, `requirements_met`.
- **Tests of any crate**: `ultar-hal = { ..., features = ["mock"] }` as dev-dependency.
- **init / service files**: `Edition::matches` for `editions = [...]`.

## 4. Test results (run with `-j1`)

| Crate | Unit | Integration | Total | Result |
|---|---|---|---|---|
| `ultar-profile` | 51 | 8 (`profile_flow`) | 59 | all pass |
| `ultar-hal` | 38 | 48 (`backend_set` 9, `capabilities` 7, `mock_base` 9, `mock_links` 8, `mock_ui` 15) | 86 | all pass |

145 tests, 0 failed, 0 ignored. `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings` clean for both crates; `ultar-hal` also clean
without the feature and with `--features mock`; `cargo doc` without warnings
(`#![warn(missing_docs)]` is on in both crates). Largest file 481 lines, no function over
60 lines, no struct over 15 fields.

## 5. Open points and limits

- **No real backend exists.** Only the mock implements the traits; nothing was run on
  hardware. The trait shapes come from reading the old shell, not from a port.
- **No real `ProfileSource` and no real `ChoiceStore`** (file-backed); both belong to the
  backend/devd crates.
- **Static/aarch64 build not run**: only `x86_64-unknown-linux-gnu` is installed here.
  All dependencies are pure Rust.
- **`msg` tables are interim**: both crates carry their own ar/en/de table until
  `ultar-i18n` exists; keys are stable (`profile.*`, `hal.error.*`, `hal.class.*`).
  Panel titles (`settings.panel.*`) are keys only, without texts.
- **Capabilities without trait**: fingerprint and NPU have no HAL trait yet.
- **Traits are synchronous.** `Input::poll` has a timeout; an fd or async interface for the
  compositor event loop is not designed. `Radio` covers state only (no calls, SMS, data,
  USSD); `Bluetooth` covers power and scan only (no pairing); `Wifi` has no saved-network
  list. These were outside the requested scope.
- **`Display::present`** takes a borrowed CPU frame; buffer sharing with a GPU renderer
  (dmabuf) is not modelled. `Renderer` is a capability query only.
- **Mock TPM is not cryptographic** (documented in the type); sealing only binds a blob to
  register values so that callers can be tested.
- **Decisions to confirm**: a forced user mode overrides even "server → headless";
  the 7 inch tablet threshold; `WifiCredentials::validate` accepts 8–63 bytes without
  control characters (as the old shell did, so Arabic passphrases work) instead of
  ASCII only.
- `Cargo.lock` files were generated in both crate folders; `target/` in
  `/home/claude/agents/hal/target` can be deleted.
