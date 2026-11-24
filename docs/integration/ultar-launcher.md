# ultar-launcher – the `launcher.*` bus contract

Area: **system core (area 3)**. The contract lives in one place —
`system/core/ultar-bus/src/messages/launcher.rs` — and this document is its
reference. Every caller (the shell, the Ultar Store, `ulaunch`, the installer)
and the launcher implementation follow this file; a change to the contract is a
change here **and** there, never somewhere else.

> **Implementation status (honest):** the launcher *daemon* (`ultar-launcher`:
> cells, uids, profiles) is developed on the branch `wip/app-launcher` and is
> not merged to `main` yet. The Store codes against this contract; when the
> service is absent, every call answers `BusError::UnknownService`, which the
> Store maps to `store.error.no-launcher` and shows as such — it never fakes a
> successful install, launch or removal (APP-RULES §8).

## 1. The service

| | |
|---|---|
| Service name | `launcher` |
| Caller identities | `system.shell`, `app.store`, `system.admin`, `ulaunch` (root socket) |
| Policy | `rootfs/etc/ultar/bus-policy.toml`; `launcher.install` / `launcher.uninstall` / `launcher.free*` additionally require the pseudo method `launcher.admin` in the caller's `call` list |
| Wire | typed `Call` structs, `VERSION = 1`, postcard (see ultar-bus.md) |

## 2. Methods

| Method | Request | Reply | Meaning |
|---|---|---|---|
| `launcher.list` | `List {}` | `ListReply { apps: Vec<AppStatus> }` | the installed apps: id, uid, profile, version, instances, installed size, last-used time |
| `launcher.start` | `Start { app, role, activation_token }` | `StartReply { state, pid, already }` | start an app (single instance per role; a running instance is reported, not started twice) |
| `launcher.stop` | `Stop { app, role }` | `StopReply { state }` | stop an instance (a service stays stopped) |
| `launcher.permissions` | `Permissions { app }` | `PermissionsReply { declared, bus, files }` | what an app declared and what was granted |
| `launcher.grant` / `launcher.revoke` | `Grant/Revoke { app, permission }` | `PermissionReply { granted, stopped }` | user granted/took back a permission; dependent instances are stopped |
| `launcher.install` | `Install { package }` | `InstallReply { app, uid, profile, declared }` | register a signed `.uap` that the package installer put in place; an update is the same call with the new package |
| `launcher.uninstall` | `Uninstall { app, keep_data }` | `UninstallReply { stopped }` | remove an app; its data goes with it unless `keep_data` |
| `launcher.free_items` | `FreeItems { for_app }` | `FreeItemsReply { items, required, available }` | what can be freed for the "not enough space" surface (Store, DESKTOP-SPEC §7.3) |
| `launcher.free` | `Free { ids, resume }` | `FreeReply { freed, stopped, install_done }` | free the selected items; with `resume` continue the blocked installation |

`AppStatus { app, uid, profile, version, instances, size: Option<u64>,
last_used_ms: Option<u64> }` — `size` and `last_used_ms` were added for the
Store's Installed page ("Liste mit Größe, zuletzt benutzt", DESKTOP-SPEC §7.3);
`None` means "not known yet" and is drawn as `—` (board footnote).

`Role = Ui | Service`; `InstanceState = Starting | Running | Stopping | Backoff
| Stopped | Crashed | Failed`.

## 3. Error codes (typed `BusError::Service(code)`)

| Code | Constant | Meaning for the caller |
|---|---|---|
| 1 | `ERROR_UNKNOWN_APP` | no installed app has this id |
| 2 | `ERROR_NOT_DECLARED` | the manifest does not declare the permission/role |
| 3 | `ERROR_NOT_GRANTABLE` | the profile forbids the permission |
| 4 | `ERROR_PACKAGE_REFUSED` | **signature/manifest refused – nothing was written** (Store: `store.error.package-refused`) |
| 5 | `ERROR_PLATFORM` | kernel lacks an isolation feature |
| 6 | `ERROR_LIMIT` | user ids / running apps exhausted |
| 7 | `ERROR_START_FAILED` | the cell could not be started |
| 8 | `ERROR_SHUTTING_DOWN` | the launcher is stopping |
| 9 | `ERROR_STORAGE` | state could not be stored |
| 10 | `ERROR_NOT_GRANTED` | a needed permission is not granted |
| 11 | `ERROR_INVALID` | bad app id or path |
| 12 | `ERROR_NO_SPACE` | not enough free space – the caller shows `launcher.free_items` first (added for the Store) |

## 4. How the Ultar Store uses it

`apps/ultar-store` is a pure bus client (APP-RULES §4: apps never run system
programs or touch package files themselves):

| Store action | Call |
|---|---|
| `store.launch` (Open) | `launcher.start { role: Ui }` |
| `store.install` (Install/Update) | `launcher.install { package: <path from the signed catalogue> }` |
| Installed list, updates | `launcher.list` (version compare against the catalogue) |
| Remove (confirm dialog) | `launcher.uninstall { keep_data }` |
| "Not enough space" surface | `launcher.free_items`, then `launcher.free { ids, resume }` |
| `store.log` | `log.query` (logd) + a file in `app:data` |

Package integrity stays where it belongs: the launcher verifies every `.uap`
through `ultar-pkg` (hard gate: without a valid signature there is no
installation, no matter what any catalogue claims). The Store's own
contribution is the **catalogue** signature (`ultar-catalog-v1`, Ed25519 over
`sha256(body)`, verified against the same `/etc/ultar/trusted-keys` directory —
see `apps/ultar-store/src/source.rs`).

## 5. Open points

1. **Progress:** `launcher.install` is one call; there is no progress topic
   yet. The Store shows the indeterminate ring (and a percent when a future
   `launcher.progress` topic reports one). Not faked either way.
2. **`launcher.free.*` provider:** the free-space sources (trash, temporary
   files, old update slots) live with files/settings/updater; the launcher
   aggregates them. Until then the Store falls back to the board's placeholder
   list with `—` sizes (exactly what the board draws).
3. **App identity plumbing:** `app.store` as a bus identity needs the cell
   manager's socket injection (launcher branch §8); today the policy rule
   exists and the store connects when `ULTAR_BUS_SOCKET` is set.
