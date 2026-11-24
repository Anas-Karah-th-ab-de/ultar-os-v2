# Settings app and graphical installer – integration notes (w5/sysapps)

Base: HEAD `ce287d2` + the audit test patch (`w5/uiaudit/CHANGES.patch`, included in this patch) + the boards patch
(`w5/boards/CHANGES.patch`, **not** included – apply it first; it brings CompactSettings/CompactDisplay and the CLICKS
additions `settings.back`, `display.autobrightness`).

## Open points (decide / do before merging)

1. **Layer rule:** `apps/ultar-settings` depends on `platform/ultar-surface` (the compositor client: window, input,
   clipboard). `tools/check-layers.toml` allows apps only `platform/app-api` (does not exist) and `platform/ultar-ui`,
   so `ultar-check layers` reports exactly this one finding. Changing the rule was not mine to do. Options: add
   `platform/ultar-surface` to the apps allow-list, or create `platform/app-api` re-exporting the window part of
   `ultar-surface`. Everything else of `ultar-check all` is clean (no baseline entry added).
2. **Display service not implemented:** contract `ultar-bus/src/messages/display_service.rs` (topic `display.outputs`
   retained; calls `display.identify`, `display.detect`) is defined in one place here; the compositor/display service
   must publish/answer it. Without it the Displays page shows the stored settingsd values with a warning bar, the
   resolution/refresh choices offer only the current mode, and identify/detect report "service not running".
   (Compositor `OutputInfo` has no mode list, EDID name or settingsd id – that is why a bus contract was needed.)
3. **Account service not implemented:** contract `ultar-bus/src/messages/account_service.rs` (topic `account.state`,
   10 calls). `ultar-cloud` has the HTTP client but no bus daemon. Without it the Account page shows an error bar and
   disables sign-in/create/server with the reason.
4. **Keep-or-revert dialog** (`display.keep`, `.revert`, `.keep.more`) is asked by `ultar-sysdialog` (w5/shell), which
   follows `settings.pending`; the Settings app only sets the risky key and follows `settings.pending_resolved` /
   `settings.changed`. Not part of this patch's tests (reported as OWNED).
5. **Updater contract mirror:** `apps/ultar-settings/src/service/update_contract.rs` copies `update.check`,
   `update.offline`, `update.progress` field by field (apps may not depend on `ultar-update`). Move the contract into
   `ultar-bus` and delete the mirror. Firmware updates = the system updater (no separate firmware service exists).
6. **Installer limits (disabled with the reason, never pretending):** "beside existing system" and "manual" (needs
   shrinking foreign partitions), encryption passphrase/TPM (needs an encryption service; no own crypto), "fetch newer
   drivers" (no network service). The installer writes the A/B layout of `ultar-slots` onto the *whole* chosen disk.
   The user's choices go into an `UIAN` record in `ULTAR-BOOTCTL` at 128 KiB (`engine/answers.rs`); **a first-boot
   reader of that record does not exist yet**. No user password is collected (first start sets it).
7. **Kernel text installer** (`kernel/ultar-kern/src/install_ab.rs`) still has its own copy of plan/GPT/bootctl;
   `ultar_slots::install` is now the shared pure source – switching the kernel to it is a kernel-area change.
8. **SurfaceStates** board = the Store's states (`store.*` actions); there is no Store on HEAD. Reported as OWNED.
9. **Boards patch side effect:** it changed States/StatesLight, so the audit's `design/conformance/states.tsv` is
   stale (`run.sh` stops at the manifest check when Chrome is present). Regenerate with `make_manifest.py` and update
   `known-differences.txt` together with the toolkit (w5/uikit) – not done here.
10. Disks for the installer come from the launcher (`--disk PATH=NAME`, `--source PATH`): no `/dev/` path in the app;
    disk enumeration belongs to the device HAL (not existing yet).

## What exists

| Part | Where |
|---|---|
| Settings app (lib + bin `ultar-settings`) | `apps/ultar-settings/` – `action.rs` (CLICKS ids, registry), `areas.rs` (33 areas incl. compact `cellular`), `state/`, `update*.rs` (what each click does), `view/` (ultar-ui only), `service/` (ports, bus impl, in-memory fake), `locales/` ar/en/de (259 keys), `ultar-app.toml` |
| Installer (lib + bin `ultar-installer-ui`) | `platform/ultar-installer-ui/` – `wizard.rs` (16 steps, pure), `components.rs`, `engine/` (inspect read-only, plan, write, answers record), `view/`, `app.rs`, `ports.rs`, `locales/` (220 keys) |
| Shared install layout | `system/core/ultar-slots/src/install.rs` (plan, needed, bootctl image, GPT into caller buffers; no_std) |
| Bus contracts | `ultar-bus/src/messages/{display_service,account_service}.rs` + table rows in `messages/mod.rs` |
| settingsd | new key `display.auto_brightness` (needs `sensors`) + ar/en/de labels; 112 settings, display panel 23 |
| ultar-surface | `UiWindow::offer_text` + answers `SendRequest` (clipboard copy for `account.copy`) |
| Conformance | `tools/design-conformance/make_board_manifest.py` → `design/conformance/boards/*.tsv` (11 boards); tests `apps/ultar-settings/tests/design_conformance.rs`, `platform/ultar-installer-ui/tests/design_conformance.rs`; ratchet `design/conformance/known-boards.txt`; `coverage.toml` updated for all my actions + the boards patch's new actions |

Pages: Displays (wide: arrangement, mode with mirror/single rows, night light/remember/bar/lid, card of the selected
display with main-display badge or "Make this the main display" button – never "off" (§13 #64) –, resolution, scale,
refresh, orientation, HDR disabled with reason, colour profile, brightness; compact: one card, note), Night light
sub-page, Drivers (missing-driver alert, table, switch with confirmation, rollback, hardware plan, firmware check /
update from USB dialog, self-test = rescan), Ultar account (§12.1 states: signed out/in, offline, failed with attempts,
locked; services; devices; change password, sign out, delete with password, QR dialog, create-account page), all
settingsd panels as generic rows, areas without a service as empty state with reason, sidebar search (areas and row
labels; Enter opens the first result), compact list ↔ page with `settings.back`.

## Wiring

```
ultar-settings --compositor $ULTAR_COMPOSITOR --bus $ULTAR_BUS_SOCKET [--lang ar|en|de] [--light]
               [--area displays] [--catalogs /usr/share/ultar/locales] [--documents ~/Documents]
ultar-installer-ui --source <medium> --disk <dev>=<name> [...] [--plans <dir on medium>] --compositor … --bus …
   exit 0 = installed (restart), 3 = cancelled (nothing written), 1 failure, 2 usage
```
Bus policy for `app.settings`: `settings.*`, `devices.list|switch|rollback|rescan`, `display.*`, `update.check|offline`,
subscribe `settings.*`, `display.outputs`, `update.progress`; `account.*`. Installer (`system.installer`):
`devices.list|switch|rescan`. Service catalogs (settingsd/devd/updater keys) are read from `--catalogs DIR/<svc>/<lang>.ftl`.

Safety of the installer: only `engine::write::execute` opens a device writable; it needs an `EraseConfirmation`, which
only `Wizard::confirm_install` creates on step 14 with the erase line ticked, bound to the target path and size;
target = medium, changed size or another target → refused before any write; old GPT zeroed first, every chunk read
back, GPT written last, flushed and re-read.

## Tests (all green, 3 consecutive full runs + 4 more; one failure in an earlier run was not reproducible)

`cargo test -p ultar-settings -p ultar-installer-ui -p ultar-bus -p ultar-slots -p ultar-surface -p ultar-settingsd`:
413 tests. Mine: settings 7 unit + 9 clicks + 4 access + 2 conformance + 1 shots; installer 10 unit + 4 clicks +
4 safety + 4 engine (image file **and loop device**) + 3 access + 2 conformance + 1 shots; ultar-slots +4; ultar-bus +2.
fmt clean, clippy `-D warnings` clean (6 crates, all targets). Frame budget (release, opt 3, no LTO, full repaint):
settings 2 ms at 1440×860, installer ≤ 1 ms at 1280×800 (budget 8 ms).

Conformance (box ±2, colour ±6, state, name) – 0 MISSING on every board in my scope:

| Board | OK | DIFFERENT | owned elsewhere |
|---|---|---|---|
| SettingsDisplay | 32 | 21 | 6 (win.*, keep dialog) |
| SettingsDisplayLight | 9 | 44 | 3 |
| SettingsDrivers | 9 | 42 | 3 |
| Account | 10 | 44 | 3 |
| CompactSettings / CompactDisplay | 0 / 0 | 11 / 9 | 2 / 2 (mobile.*) |
| Installer / Disk / Drivers / Summary | 0 | 25 / 22 / 26 / 23 | – |
| SurfaceStates | – | – | 21 (Store) |

Every element exists in dark+light × ar+en with a translated name (test). DIFFERENT causes (details per element in
`known-boards.txt`): toolkit gaps below, sidebar/footer background, text-width differences of a few points, and
layout shifts that follow from them.

## Toolkit gaps found (reported to w5/uikit, not worked around)

G1 selectable **tile/card radio** (arrangement tiles, mode cards, installer scope/disk/encryption cards) – RadioGroup
draws a vertical list; G2 **absolutely positioned arrangement canvas** with dragging and snapping (display.select drag);
G3 container with the **bar/sidebar background** and end border; G4 **accessible name different from the visible
label** (boards: "بطاقة الرسوميات: استرجاع التعريف السابق" on "استرجاع السابق", "الخطوة 1: اللغة" on "اللغة");
G5 **programmatic focus** (`Ui::focus(id)`) – after a content rebuild focus is lost; G6 grid / basis-0 flex columns and
**wrapping rows** (driver table widths computed by hand; account buttons wrap on the board); G7 disabling **one option**
of a RadioGroup/Select with a reason (TPM, beside, manual); G8 **QR code** widget; G9 danger-toned **card**; G10 slider
value label shows the fraction of the range, not the value; G11 list item with sublabel is 32 high (T1);
G12 `ui.password.show` is a held reveal, boards show a toggle button outside the field; G13 removing a node that holds
an open dialog leaves the modal layer active (worked around by closing first: `view/mod.rs`).
