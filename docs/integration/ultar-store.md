# ultar-store – integration notes

Area: **apps (area 6)**, crate `apps/ultar-store` (`ultar-store` lib + `ultar-store`
binary). Boards: `design/boards/project/{Store,SurfaceStates,CompactStore,CompactStoreApp}.dc.html`.
Spec: DESKTOP-SPEC §7.2/§7.3 · clicks: docs/ui/CLICKS.md · rules: docs/APP-RULES.md.

## Open points first

1. **One launcher contract (wave 9 integration).** The Store uses main's
   `system/core/ultar-bus/src/messages/launcher.rs` (launcher v2, documented in
   [ultar-launcher.md](ultar-launcher.md)) unchanged; its own copy of the
   contract was dropped. `launcher.list` reports no size and no last use, so
   the Installed rows show "size unknown" instead of a number.
2. **No free-space contract.** main's launcher has no `free_items` / `free`
   and no "not enough space" error code. The Store shows the board's
   placeholder list (three items, sizes `—`); "Free" answers
   `store.error.no-launcher` and frees nothing. The `NoSpace` surface is only
   reachable from the scripted backend until the launcher grows such a code
   (it must not reuse 12 = `ERROR_NO_OPEN`).
3. **No install-progress topic.** `launcher.install` is one call; until a
   `launcher.progress` topic exists the ring is indeterminate and the percent
   label shows the loading text. The UI supports percents (tested via the
   scripted backend); nothing is faked on the wire.
4. **Phone chrome is the shell's.** `mobile.status` and `mobile.home` on the
   CompactStore/CompactStoreApp boards belong to the phone shell (like
   `win.*` belongs to the compositor) and stay `missing` in
   `design/conformance/coverage.toml`; every `store.*` action of the four
   boards is `ok`.
5. **Account page is informational.** Sign-in and the account services belong
   to Settings (DESKTOP-SPEC §10); the Store page explains that and adds no
   duplicate controls. A future `store.account` bus contract can light it up.
6. **System updates are the Updater's.** The Updates page separates system
   from app updates (§7.3) and states that the system slot switch is managed
   by the updater service (`update.*` contract); the Store does not duplicate
   A/B handling. App update history and version rollback (§7.3 "Verlauf,
   Zurückrollen") are not built yet — they need launcher-side support
   (keeping the previous `.uap`); noted here rather than faked.
7. **Bus rights of `app.store` (needs the lead's decision).** The policy
   gives `app.store` `launcher.install` / `uninstall` **and `launcher.admin`**,
   i.e. the Store identity may install any signed package and remove any app
   with its data. The UI asks first (remove dialog), but the launcher does not:
   a compromised Store process could uninstall apps silently. Better: the
   launcher asks `sysdialog.authenticate` for install/uninstall requested by a
   non-admin identity, and `app.store` loses `launcher.admin`. (`log.query`,
   which the Store never calls, was removed.)
8. **Search-field caret:** the window loop rebuilds the tree after every
   change and restores the focus to the search field, so typing works; the
   caret restarts at the end of the text (`TextField::set_text` behaviour),
   mid-string editing jumps to the end. A partial-rebuild path (or caret
   restore) is a small follow-up in `main.rs`.
9. **Layers exception:** `tools/check-layers.toml` now allows apps to use
   `platform/ultar-surface` (the compositor client) until `platform/app-api`
   exists — exactly as docs/integration/ultar-compositor.md §4 prescribes.
   The Store binary is `required-features = ["gui"]`; the library and every
   test build on any host without bus or surface.
10. **Permissions shown are the catalogue's.** The app page lists the
    permissions the signed catalogue declares; the launcher's `InstallReply.
    declared` (from the package manifest) is not compared with them yet. A
    package declaring more than its catalogue entry should be refused or shown
    again before it is granted.
11. **No package digest in the catalogue.** The catalogue names the `.uap`
    file but does not pin its sha256; the launcher's own signature check is the
    only guard on the package (it is sufficient against forgery, not against a
    different validly signed version on the stick).
12. **Trusted-key override.** `ULTAR_TRUSTED` (tests, dev images) replaces
    `/etc/ultar/trusted-keys`; the cell manager must not pass it to the Store
    in production images.

## What was built

| Part | Where |
|---|---|
| Catalogue data (apps, permissions in plain words, categories, file types, sources) | `src/catalog.rs` |
| **Signed local catalogue**: `ultar-catalog-v1` = Ed25519 over `sha256(body)`, detached `.sig`, verified against `/etc/ultar/trusted-keys` (same key files as `.uap`); `CatalogSource` trait + file implementation + embedded image catalogue + stick discovery | `src/signature.rs` (strict Ed25519, owner-only key files, signer = verifying key), `src/source.rs` (single capped read: verified bytes = parsed bytes) |
| Search: name/description/file type, Arabic tolerant (`ultar_i18n::normalize_for_search`), ranked; `.xlsx` finds the office component | `src/search.rs` |
| Pure model: areas, app page, back-stack, operations (install/update/remove with progress), no-space flow, offline/error/loading surfaces | `src/state.rs`, `src/action.rs`, `src/update.rs` |
| System backend (bus `launcher.*`, files) + scripted backend for tests | `src/backend.rs`, `src/scripted.rs` |
| Screens: desktop (Store board), phone (CompactStore/CompactStoreApp), all five SurfaceStates panels, every area page | `src/view/*` |
| Texts: `locales/{ar,en,de}.ftl` (Arabic first; the boards' strings verbatim), brand texts come from `ultar-brand` via `ultar-i18n` | `src/i18n.rs` |
| Window binary (compositor loop + backend worker thread; nothing blocks the UI) | `src/main.rs` (`--features gui`) |
| App manifest + icon | `ultar-app.toml`, `assets/icon.svg` |
| Launcher bus contract + policy + docs | `system/core/ultar-bus/src/messages/launcher.rs`, `rootfs/etc/ultar/bus-policy.toml`, [ultar-launcher.md](ultar-launcher.md) |

## Clicks (docs/ui/CLICKS.md)

Every `store.*` action is implemented and registered `ok` in
`design/conformance/coverage.toml` with impl + test:

`store.area` · `store.search` · `store.search.clear` · `store.open` ·
`store.back` · `store.launch` · `store.install` · `store.retry` · `store.log` ·
`store.offline` · `store.free.item` · `store.free.cancel` · `store.free.go` ·
`store.more`

Actions beyond the boards (needed by DESKTOP-SPEC §7.3, documented here):
`store.update.all`, `store.remove`, `store.remove.confirm`,
`store.remove.cancel`, `store.cancel`, `store.category`, `store.file.install`,
`store.source.use`. No click is dead: every clickable node is bound in the
`HitMap`, and `tests/clicks.rs` walks the map.

Behaviour notes:
- **Cards never install directly** (CLICKS `store.open`): the card/row opens
  the app page; Install/Update live there.
- **`store.back`** returns to the list the page was opened from with its query
  intact (same scroll model: the tree is rebuilt from the same state).
- **`store.install`**: Install → primary button turns into the progress ring
  with the percent as its label, Cancel next to it → afterwards Open + Remove.
  Nothing extra is asked when the rights are only those shown on the page.
- **`store.remove`** asks first (keep data / delete data / cancel); Escape and
  Enter take the safe way out.
- **`store.free.go`** frees the selected items and continues the blocked
  install; **`store.free.cancel`** cancels and frees nothing.
- **`store.log`** writes `store-error-report.txt` into `ULTAR_STORE_DATA` and
  reports the path; a failure is shown, not swallowed.
- **`store.retry`** reloads the catalogue (offline and error surfaces).
- **`store.offline`** opens Offline sources; `store.source.use` switches the
  active signed catalogue (USB stick / folder).

## Tests

`cargo test -p ultar-store` – 91 tests, no network, no bus, no files outside
temp dirs:

| Target | What |
|---|---|
| lib unit tests (26) | catalogue parse/validate, signature domain binding, sign/verify/tamper, search ranking + Arabic folding, error keys in all three languages, catalog key parity |
| `tests/logic.rs` (13) | the built-in catalogue (12 components, permissions, requirements), search behaviour, surface transitions |
| `tests/transitions.rs` + `transitions2.rs` (18) | every action/event of the model: areas, back, install gating (requirement, package), no-space, free selection, cancel, update-all chain |
| `tests/clicks.rs` (19) | every board `store.*` driven through real pointer/key events on the headless tree |
| `tests/flow.rs` (8) | full command→backend→event loops: boot, offline boot, install success/refusal, no-space→free→continue, remove with data choice, search flow, log flow |
| `tests/screens.rs` (7) | every page × every state rendered headless in ar+en × dark+light, desktop 1280×800 and phone 360×800; board strings asserted verbatim; no unresolved keys |

`tools/design-conformance/run.sh` runs the store suites as part of the design
conformance run; `design/conformance/coverage.toml` names the proving test per
action.

## Environment of the binary

| Variable | Meaning |
|---|---|
| `ULTAR_COMPOSITOR_SOCKET` | compositor socket (default `/run/ultar/ui/compositor`) |
| `ULTAR_BUS_SOCKET` | bus socket for `launcher.*` (cell-injected; unset → honest `no-launcher` errors) |
| `ULTAR_STORE_CATALOG` | directory with `ultar-catalog.toml` + `.sig` (stick/folder); unset → the image catalogue |
| `ULTAR_TRUSTED` | trusted-keys directory (default `/etc/ultar/trusted-keys`, same as `upkg`) |
| `ULTAR_STORE_DATA` | the app's data area (error reports) |
| `ULTAR_MEDIA_DIR` | where sticks are looked for (default `/media`) |

## Catalogue format (`ultar-catalog-v1`)

```toml
format_version = 1
generated = "YYYY-MM-DD"

[[app]]
id = "km-office"          # safe directory name (ultar_pkg::valid_id rules)
category = "office"       # store.category.<id> names it
icon = "office"           # IconId name (docs/ui/ICONS.md)
publisher = "KM Code Craft"
source = "ultar"          # ultar | linux | windows | android
version = "1.4.0"         # optional; None → `—` and no update claims
size_download = 123       # optional; None → `—`
file_types = [".docx"]    # searched directly (".xlsx finds Sheets")
requires = ["linux-programs"]
package = "km-office.uap" # file next to the catalogue; the launcher verifies it again
[app.name]    ar = "…" en = "…" de = "…"   # de optional
[app.summary] …
[app.description] …
[[app.permissions]] id = "files"
[app.permissions.reason] ar = "…" en = "…"  # plain words, Arabic first
```

Signature: `<file>.sig` = `"<key-id>\n<base64 ed25519 signature>"` over
`ultar-catalog-v1\n<hex sha256(body)>\n`. Unsigned or unknown-signer
catalogues are refused (`store.error.unsigned` / `store.error.unknown-signer`)
— except the catalogue compiled into the signed system image. Package
signatures are verified **again** by the launcher through `ultar-pkg` at
install time; the catalogue signature never replaces the package gate.
