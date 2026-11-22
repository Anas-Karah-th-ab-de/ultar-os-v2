# ultar-settingsd – integration notes

Area: **system core (area 3)**. Target folder `system/core/ultar-settingsd`.
Crate: `/home/claude/agents/settingsd/ultar-settingsd` (version 0.7.0, `#![forbid(unsafe_code)]`,
`unwrap`/`expect` denied outside tests, `#![warn(missing_docs)]`).
Library `ultar_settingsd` + binaries `ultar-settingsd` (daemon) and `usettings` (CLI).
Principle: *surface = app, state and rules = service* – Settings app, quick settings and CLI only show what this service offers.

Dependencies: path crates `ultar-bus`, `ultar-i18n`, `ultar-profile` (copies next to the crate; adjust paths), `toml` 0.8
(`parse`, `display`; already used by `ultar-profile`). No C code. `Cargo.toml` has an empty `[workspace]` table – remove on integration.

## What exists

| Part | Files |
|---|---|
| Values (`Value`: bool, int, float, string, list) and conversions bus/TOML/text | `src/value.rs` |
| Schema registry: keys and `<id>` templates, types + validation, defaults, risk, scope, panels, migrations | `src/schema/{key,types,def,registry,migrate,mod}.rs` |
| Seed schema, 111 settings in 10 panels | `src/seed/{mod,display,desktop,system}.rs` |
| Store: file format with CRC line, atomic writes, quarantine, load + schema check, coalescing | `src/store/{format,disk,load,mod}.rs` |
| Clock trait, system clock, manual clock | `src/clock.rs` |
| Display combinations (pure): stable ids, combination id, first-time defaults | `src/displays.rs` |
| Appliers: trait, set per domain, `MockApplier`, `LogApplier` | `src/apply.rs` |
| Core state machine (no threads, no sockets, no real time) | `src/core/{mod,write,pending,displays,list}.rs` |
| Permissions (`Access`, `PolicyAccess`, `Caller`) | `src/access.rs` |
| Bus service | `src/server/{mod,convert,stats}.rs` |
| Daemon start-up / command line | `src/daemon.rs`, `src/bin/ultar-settingsd.rs` |
| CLI | `src/cli.rs`, `src/bin/usettings.rs` |
| Catalogs, 364 keys each | `locales/{ar,en,de}.ftl`, `src/i18n.rs` |
| Typed errors | `src/error.rs` (+ `SchemaError`, `ValueError`, `StoreError`, `ApplyError`, `DecodeError`, `DisplayError`, `CliError`, `DaemonError`) |

## Public API (summary)

```rust
// Schema
SettingDef::{flag,int,float,choice,text,list}(key, panel, ..).needs(&[Capability]).risk(Risk).scope(Scope).domain(Domain).computed()
Registry::builder().version(n).setting(def).settings(defs).migration(Migration{to_version, steps}).build() -> Result<Registry, SchemaError>
registry.resolve("display.edp-1.scale") -> Result<Resolved{def, key, instance}, LookupError>
ValueType::{Bool, Int{min,max,step}, Float{min,max,step}, Enum(&[..]), Text(TextFormat), List{item,max_len}}  .validate/.accept/.parse_text
TextFormat::{Free{max_chars}, Time, Resolution, DisplayId, Name, Currency, TimeZone, Host, Path}
Risk::{Safe, ConfirmRevert{seconds}, AdminOnly}   Scope::{System, User, DisplayCombination}
trait DefaultProvider { fn computed_default(&self, def, instance) -> Option<Value> }   // "computed from device"
seed::registry() / seed::definitions() / seed::SCHEMA_VERSION / seed::CONFIRM_SECONDS (15)

// Store
Store::open(StoreConfig{dir, user, write_delay}, &registry, Arc<dyn Clock>) -> Result<(Store, Vec<Notice>), StoreError>
store.get/set/remove(Slot::{System, User, Combo(id)}, key)  use_combo  latest_combo  journal_add/journal_remove
store.tick() / flush() / next_deadline() / stats()

// Core (what tests and the server drive)
Core::open(CoreConfig{store, registry, clock, appliers, defaults, capabilities}) -> Result<(Core, Vec<Notice>), SettingsError>
core.get(key)  core.set(&Caller, key, Value) -> Result<bool,_>  core.reset(&Caller, key)
core.list(&Caller, Option<panel>) -> Result<Vec<PanelInfo>,_>
core.confirm(Option<key>) / core.revert(Option<key>) -> Result<u32,_>
core.set_displays(&Caller, &[DisplayDescriptor]) -> Result<SetDisplaysReply,_>
core.set_capabilities(Capabilities)  core.tick()  core.next_deadline()  core.take_events() -> Vec<Event>  core.flush()

// Appliers
trait Applier: Send { fn apply(&mut self, change: &Change{key, template, instance, value, provisional}) -> Result<(), ApplyError> }
Appliers::new().with(Domain::Display, Box::new(applier)).with_all(&cloneable)

// Server
Server::start(ServerParts{bus, core, access, clock, notices, on_error, register_wait}) -> Result<Server, BusError>
server.tick() (manual clocks)  server.stats()  server.wait()  server.stop()
```

## Bus contract (service `settings`)

Unchanged from `ultar-bus`: `settings.get` (`Get`→`GetReply`), `settings.set` (`Set`→`SetReply`), topic `settings.changed` (`Changed`).
Note: `GetReply.value` is always `Some(value in effect)`; an undeclared key is the error `UNKNOWN_KEY`, not `None`.

**Changes in my copy of `ultar-bus` (every one of them):**

1. `src/messages/settings.rs`: enum `SettingValue` got the appended variant `List(Vec<String>)` (older values keep their
   encoding, so `VERSION` stays 1); `pub use super::settings_ext::*;`; one more value in the round-trip test.
2. New file `src/messages/settings_ext.rs`:
   calls `Reset`→`ResetReply{changed}`, `ListRequest{panel}`→`ListReply{panels: Vec<PanelInfo>}`,
   `Confirm{key: Option}`/`Revert{key: Option}`→`PendingReply{resolved}`, `SetDisplays{displays: Vec<DisplayDescriptor>}`→
   `SetDisplaysReply{combination, ids, first_time}`, `Shutdown`→`()`;
   topics `settings.pending` (`PendingConfirmation{key,value,previous,seconds}`), `settings.pending_resolved`
   (`PendingResolved{key,outcome,value}`, outcomes `Kept, Reverted, TimedOut, RevertedAtStart, DisplaysChanged`),
   `settings.store_report` (`StoreReport{notices}`, **retained**);
   data types `PanelInfo`, `ItemInfo`, `ItemKind`, `ChoiceInfo`, `RiskInfo`, `ScopeInfo`, `StoreNotice`, `StoreNoticeKind`;
   module `code` with the service error codes 1–11 carried as `ErrorCode::Service(n)`:
   `UNKNOWN_KEY, TYPE_MISMATCH, OUT_OF_RANGE, INVALID_VALUE, UNAVAILABLE, UNKNOWN_INSTANCE, NOT_PENDING, LIMIT,
   APPLY_FAILED, STORE_FAILED, BAD_REQUEST`. "Not permitted" is the existing `ErrorCode::PermissionDenied`.
3. `src/messages/mod.rs`: `mod settings_ext;`, three lines in the doc table, the new calls/topics added to
   `all_contract_names_are_valid`.

The repository's `ultar-bus` has moved on since I copied it (it now has `messages/device_service.rs` and a changed
`messages/mod.rs`); the three changes above must be merged by hand, they do not touch anything else.
Gate of the modified copy: fmt clean, clippy clean, 141 tests pass (139 + 2 new).

`StoreNotice.text` is a `Text` with key `settingsd.store.<kind>` and the arguments in the order *file, detail*
(the catalog entries name them `{file}` and `{detail}`).

## Behaviour other crates can rely on

- **Keys** are bus-style names (`a-z 0-9 _ -`, dot separated, ≤128 bytes). `<id>` in a declaration stands for one
  display (`display.<id>.scale`, only for connected displays) or one app (`notifications.app.<id>.sound`, any id).
  i18n keys follow from the key without `<id>`: `settings.<key>.label`, `.desc`, `.choice.<choice>`; panel titles
  `settings.panel.<panel>`.
- **Rejections are typed**: unknown key, unknown display, type mismatch, out of range / off step, invalid choice or
  format, missing capability, not permitted, nothing pending, limit, applier refused, store failed.
  A whole number is accepted for a fractional setting; nothing else is converted.
- **Order of a set**: validate → applier of the setting's domain → store in memory → events published (confirmed) →
  reply. A caller holding its reply can rely on `settings.changed` being routed already. If the applier fails, nothing
  is stored or announced and the caller gets `APPLY_FAILED`.
- **Store**: directory given at start with `system.toml`, `user-<name>.toml`, `displays.toml` (one table per display
  combination), `pending.toml` (journal) and `quarantine/`. First line `#ultar-settings crc32=<8 hex>` over the TOML
  body. Writes: temp file + fsync + rename + directory fsync. A file that is cut, altered, not UTF-8, larger than
  4 MiB or from a newer schema → copy in `quarantine/<file>.0` (5 kept per file), defaults, notice; older schema →
  migrations, notice; undeclared key / invalid value / wrong scope → dropped, notice. Notices go to stderr (daemon)
  and to the retained topic `settings.store_report`. **The files cannot be edited by hand** (checksum) – use `usettings`.
- **Coalescing**: the first change starts the delay (daemon default 250 ms, `--write-delay-ms`, max 60 s); later
  changes do not extend it; one write per touched file. Failed writes are retried after 50 ms doubling to 30 s.
  `settings.shutdown` and `Server::stop` write everything first.
- **Confirm or revert**: the journal entry is written durably *before* the applier is called; the provisional value
  lives in memory only and is stored on `confirm`. Timeout, `revert`, or setting the previous value again restore
  the stored value through the applier and publish `settings.changed` + `settings.pending_resolved`. A second change
  while pending keeps the original "previous" and restarts the 15 s. At most 32 changes pend at once.
  **Restart while pending**: on start every journal key gets its stored value applied again and announced
  (`RevertedAtStart`), then the journal is cleared.
- **Display combinations**: display id = manufacturer-product-serial from the display's own data (connector only when
  it reports nothing; identical displays are told apart by connector), combination id = FNV-1a of the sorted ids
  (`c-<16 hex>`, part of the file format). `settings.set_displays` (administrator, i.e. the display service) switches
  to the combination: all its values are applied, differing ones announced. First time: extend, primary = built-in
  display (else first by id), side by side from the primary, element size from DPI (25 % steps), preferred
  resolution and refresh rate. 64 combinations are remembered (least recently used goes), 16 displays per combination.
  Pending display changes are dropped (`DisplaysChanged`) when the set of displays changes; the same set reported
  again keeps them. After a restart the combination used last is assumed until the display service reports.
- **Listing**: panels by `ultar_profile::visible_panels` (my panels map to `Display, Appearance, Power, Language,
  DateTime, Security, Accounts`); the three panels the profile crate does not know (desktop bar, windows,
  notifications) follow "needs a display" for the first two. Items need their own capabilities (no battery → none of
  the 6 battery keys). Each item carries label/description keys, kind with limits and choices, value, default, risk,
  scope, `writable` for this caller, `pending`. Per-app families are listed as one template item (`<id>` in the key)
  plus at most 128 apps with stored values.
- **Capabilities**: from `--profile FILE` (TOML of `DeviceProfile`) at start, then from the retained topic
  `devices.capabilities` (ids parsed with `ultar_profile::Capability::parse`; `display` / `external_display` set the
  display count). Displays reported through `set_displays` count as well.
- **Permissions**: the broker decides who may call `settings.set` etc. An identity that the bus policy lets call the
  pseudo method **`settings.admin`** is an administrator (the method itself does not exist): only administrators may
  change `AdminOnly` settings, call `set_displays` and `shutdown`. `--allow-all` makes everyone an administrator.

## How to wire it

Policy rules (same file for `ultar-busd` and `ultar-settingsd --policy`):

```toml
[[rule]]
identity  = "system.settingsd"
register  = ["settings"]
publish   = ["settings.*"]
subscribe = ["devices.*"]

[[rule]]                       # display service / compositor
identity  = "system.displayd"
call      = ["settings.set_displays", "settings.admin"]

[[rule]]                       # every app: read and follow
identity  = "app.*"
call      = ["settings.get", "settings.list"]
subscribe = ["settings.*"]

[[rule]]                       # the Settings app: everything, administrator
identity  = "app.settings"
call      = ["settings.*"]
```

Start: `ultar-settingsd --store /var/lib/ultar/settings --socket /run/ultar/bus/system.sock --user <name>
--policy /etc/ultar/bus-policy.toml --profile <device profile>` (socket also from `ULTAR_BUS_SOCKET`; waits up to
10 s for the socket and for the name `settings`). It prints one line on stdout when it answers requests.
Real appliers: construct `Appliers` with one `Applier` per `Domain` (`Display, Appearance, Shell, Windows,
Notifications, Power, Locale, Time, Privacy, Account`) in the daemon (`daemon::open_core`); today the daemon registers
none (consumers follow `settings.changed`) or the logging applier with `--log-apply`.
Device-computed defaults beyond the display ones: implement `DefaultProvider` and pass it as `CoreConfig::defaults`.
New settings: add a `SettingDef` in `src/seed/*` **and** label/description (and choices) in all three catalogs – the
seed tests fail otherwise. Renaming/removing a stored key: raise `SCHEMA_VERSION` and add a `Migration`.

CLI: `usettings [--socket PATH] [--lang ar|en|de] [--token T] get KEY | set KEY VALUE | reset KEY | list [PANEL] |
watch [--count N] | confirm [KEY] | revert [KEY]`; exit 0 / 1 (service) / 2 (usage).

## Seed schema (111 settings)

| Panel | Count | Keys |
|---|---|---|
| display | 22 | `display.mode`, `.mirror_displays`, `.single_display`, `.primary`; per display `position_x/y`, `resolution`, `scale`, `refresh_rate`, `variable_refresh`, `orientation`, `hdr`, `color_profile`, `brightness`, `overscan`; `display.night_light.{enabled,schedule,start,end,temperature}`, `display.remember_window_positions`, `display.lid_close_with_external` |
| appearance | 9 | `appearance.theme` (dark), `.accent_color` (gold), `.wallpaper.{mode,picture,custom_path,slideshow_folder,slideshow_minutes}`, `.font_size`, `.icon_size` |
| desktop_bar | 15 | `bar.layout` (`bottom_bar` standard, `top_bar_dock`), `bar.icon_alignment` (center/start), `bar.displays`, `bar.autohide`, `bar.show_trash`, `bar.show_volumes`, `dock.position`, `dock.size`, `desktop.icons.{enabled,size,start_corner}`, `bar.clock.show_{seconds,weekday,date,hijri}` |
| windows | 8 | `windows.edge_snap`, `.snap_suggest`, `.switch_current_workspace_only`, `.hot_corner`, `.focus_mode`, `workspaces.{mode,count,primary_display_only}` |
| notifications | 11 | `notifications.do_not_disturb`, `.dnd_schedule.{enabled,start,end}`, `.lock_screen.{show,show_content}`, per app `enabled, banner, sound, lock_screen, important` |
| power | 12 | `power.mode`, `.charge_limit.{enabled,percent}`*, `.screen_dim_after`, `.screen_off_after`, `.suspend_after_ac`, `.suspend_after_battery`*, `.button_action`, `.lid_action`, `.auto_saver.{enabled,percent}`*, `.show_battery_percent`* (*battery) |
| region | 9 | `region.language`, `.format.{date,numbers,currency,measurement,paper}`, `.first_weekday`, `.calendar`, `.month_names` |
| date_time | 5 | `datetime.automatic`†, `.timezone.automatic`†, `.timezone.name`†, `.clock_format`, `.hardware_clock`† († admin) |
| privacy | 14 | master switches `privacy.camera`, `.microphone`, `.location`, `.sensors`, `.network` (confirm 15 s); `.tracker_blocking`, `.diagnostics`, `.allow_unsigned_packages`†, `.lock.{after,on_suspend}`, `.file_history`, `.auto_clean.{trash,temp,after_days}` |
| account | 6 | `account.calls` (off), `.vpn`, `.store_purchases`, `.backup`, `.mesh_over_internet`, `.server`† |

Confirm-or-revert (15 s): mode, mirror set, single display, primary, position, resolution, scale, refresh rate,
orientation, network master switch (11 declarations). Computed from device: primary, single display, position,
resolution, scale, refresh rate.

Arabic wording follows the glossary of the design review where it defines a term (وضع العرض، توسيع، تكرار، شاشة واحدة
فقط، الشاشة الرئيسية، الدقّة، حجم العناصر، معدّل التحديث، الاتجاه، ملف الألوان، الضوء الليلي، عدم الإزعاج، السطوع، الطاقة،
متوازن، الشريط، المرسى، سلة المحذوفات، ذاكرة USB، نظرة عامة، الإشعارات، المظهر، الخصوصية والأمان، الترابط، متجر Ultar، قفل،
خيارات الطاقة per R31, «يُتراجَع عنها تلقائياً خلال … ثانية» per R34, الشاشة الرئيسية description per R35).

## Test results (rustc/cargo 1.97.0, x86_64 Linux, `-j1`, shared 2-core machine)

`cargo fmt --check` clean · `cargo clippy --all-targets -- -D warnings` clean · `cargo doc` with `-D warnings` clean.
`cargo test` run **3 times in a row: 164 passed, 0 failed, 1 ignored (the measurement) each time.**

| Binary | Tests |
|---|---|
| lib unit tests | 142 |
| `tests/service.rs` (real broker in process, token identities, manual clock) | 10 |
| `tests/concurrency.rs` (50 clients, 50 keys) | 1 (+1 ignored measurement) |
| `tests/power_loss.rs` | 5 |
| `tests/e2e.rs` (real `ultar-settingsd` + real `usettings` binaries, temp socket) | 6 |

Lib unit tests by module: core 41 (general 17, confirm-or-revert 15, display combinations 9) · schema 28 (types 8,
registry 7, key 5, migrate 4, def 3, defaults 1) · store 28 (store 17, format 6, disk 5) · seed 10 · displays 7 ·
cli 5 · daemon 4 · apply 4 · value 4 · server 3 · i18n 2 · error 2 · clock 2 · access 2.

Covered as asked: schema validation · every seed key + choice + panel title in ar/en/de via `ultar_i18n::check_dir`
and per-key lookups, no stray `settings.*` text · power loss: a 100-odd byte file cut at **every** length, every byte
garbled with 5 patterns, old/new mixed at every offset, temp file cut at every length · coalescing (100 sets → 1
write, deadline not extended) · permissions (broker policy + risk class, three identities) · confirm/timeout/revert/
second change/limit/applier failure/restart (core **and** over the bus **and** `kill` of the real daemon) ·
combination profiles (home/office/road, order independence, restart) · capability filtering (no battery → no battery
keys, headless → no display panels, live `devices.capabilities`) · 50 concurrent clients · end to end.

Measured (`cargo test --release --test concurrency -- --ignored --nocapture`, one run, machine busy with other builds):
`get` 34 µs per call, `set` including the confirmed `settings.changed` 36 µs per call, `list` of all panels 371 µs /
15.8 kB. Static build (`RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target
x86_64-unknown-linux-gnu`): static-pie, `ultar-settingsd` 3.4 MB, `usettings` 2.2 MB, unstripped.
Largest file 494 lines, no function over 60 lines, no struct over 15 fields (checked by script).

## Not done / not tested – honest list

- **No real applier.** Only `MockApplier` and `LogApplier` (as specified). Nothing was run against real displays,
  power management, time or network; "revert" therefore means "the applier is called with the old value".
- **Ultar kernel / aarch64 not tested**; relies on what `ultar-bus` relies on plus `rename` atomicity and `fsync`
  on files and directories.
- **No signal handling** (std has none, the crate forbids `unsafe`): on SIGTERM/SIGKILL changes younger than the write
  delay (250 ms) are lost; a clean stop is the `settings.shutdown` call. Provisional changes survive a kill correctly.
- **One user per daemon instance** (`--user`); switching users at run time and per-user daemons under one name are
  not designed. User names must be one key segment (`a-z 0-9 _ -`).
- **Display geometry is not stored**: after a restart, until the display service calls `set_displays`, computed
  defaults (element size, resolution, position) of the assumed combination are the plain fallbacks (100 %, `auto`, 0).
  The values are applied again in full once the display service reports.
- **Applier failures at start and during a combination switch are only counted** (`CoreStats::apply_failures`) and
  reported through the error sink for timed-out reverts; there is no bus event for them.
- **A failed write after the reply** (disk full) is only logged by the daemon and retried; no bus event tells the
  shell. The value stays in effect in memory.
- **Event loss**: if publishing fails (bus down) the event is counted (`ServerStats::lost_events`) and not re-sent.
- **`settings.changed` is not retained** (as in the bus contract): a late subscriber must `get`/`list` first.
- **Cross-setting rules are not enforced** (e.g. `night_light.start` only meaningful with schedule `custom`,
  `account.server` "only while signed out" – the account service must enforce that; here it is `AdminOnly`).
- **Spec items that are actions or live state, not settings, are not in the schema**: identify/detect displays,
  battery level/history/health, night-light "now", wallpaper file picker, per-file-type defaults. Areas outside the
  task (Wi-Fi, network, Bluetooth, sound, mouse, keyboard, accessibility, storage, updates, backup, mesh, assistant …)
  are not seeded.
- **`ultar-profile` has no panel ids** for "desktop bar", "windows & workspaces", "notifications", "region" naming
  differs (`Language`); I mapped where possible and added the rule "needs a display" locally (`Panel::needs_display`).
  Suggest adding the three panels to `PanelId`.
- **`docs/ui/inventory.toml` does not exist yet**, so the settings are not entered there; `seed::definitions()` holds
  everything such rows need (key, type, default, capabilities, risk, scope).
- Decisions I took that the spec leaves open (please confirm): defaults `datetime.clock_format = h24`,
  `region.first_weekday = saturday`, `region.format.currency = SYP`, `datetime.timezone.name = Asia/Damascus`,
  `privacy.location = off`, `bar.clock.show_hijri = off` (calendar shows Hijri: `region.calendar = gregorian_hijri`),
  accent colours `gold, blue, green, red, purple, teal`; power settings are device-wide (`Scope::System`) but not
  admin-only; anyone allowed to call `settings.confirm` may confirm any pending change.
- Arabic and German texts were written by the agent and not reviewed by a native speaker.
- `ultar_settingsd::testdir` (temporary directories for tests) is a public module of the library because the
  integration tests need it; move it to a test-support crate if that is not wanted.
- The `[workspace]` table in `Cargo.toml`, `Cargo.lock`, and `/home/claude/agents/settingsd/target` (can be deleted).
