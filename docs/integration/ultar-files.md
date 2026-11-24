# ultar-files – the Files app (and the desktop icons)

Base: `main` 7744e78. Crate `apps/ultar-files` (lib `ultar_files` + bin `ultar-files`), boards
*Main* and *DesktopTop* (Files window, desktop icons), CLICKS `files.*` and `desk.icon`.

## What it is
- **Window** (`ultar-files [URI]`): sidebar (Home, Documents, Downloads, Pictures, Music, Videos;
  mounted drives with eject; Trash), folder toolbar (back, forward, title, item count, search,
  new folder / empty trash, grid–list), items as tiles (board) or as a sortable table, job bar
  with progress and Stop, dialogs (rename, empty trash, delete permanently, properties,
  name conflict, drive busy, failure with a plain hint), toast with Undo.
- **Desktop** (`ultar-files --desktop`): `Role::Background` surface with the desktop icons
  (home, each mounted drive, then the items of the `desktop` area). Double click / Enter opens
  them in a Files window via `launcher.start`; menu Open, Rename, Move to trash, Properties
  (+ Eject on a drive).
- **Compact** (logical width ≤ `layout.compact_max`): no sidebar; a places button opens a menu.

## Security (cell)
- The app sees only `/areas/<name>` (`ULTAR_AREAS`): `home`, `documents`, `downloads`,
  `media-pictures`, `media-music`, `media-videos`, `desktop`, `trash`, `removable/<drive>`.
  `fs::Fs` opens each area once and walks every path one name at a time with
  `openat(O_NOFOLLOW|O_DIRECTORY)`; a symbolic link is shown as an item but never followed
  (walk, open, copy = copied as link, delete = the link only). An area that is itself a link is
  not granted. Names are checked (`/`, NUL, control characters, `.`/`..`, 255 bytes).
- Every URI (`ULTAR_OPEN_URI`, `files://`, `trash:///`, drive `file://` URIs) is parsed as
  hostile input (`uri.rs`, tests with traversal, encoded `/`, unknown areas, control chars).
- Trash records (`trash/info/*.trashinfo`, area-relative `Path=`) are size-limited and fully
  checked; a fixed-seed fuzz test feeds random records. Thumbnails: PNG (`png` 0.17) and JPEG
  (`zune-jpeg` 0.5), file ≤ 16 MiB, edge ≤ 8192, ≤ 24 MP, decoder limits set, box-filtered to
  96 px, decoded on a worker thread; fuzzed with mutated pictures.
- Copies are written as `.<name>.part` + fsync + rename; cancel removes the part file.

## Wiring needed (other owners)
1. **Launcher (`ultar-launcher`)**: bind each granted file area read-write at `/areas/<name>`
   in the cell (today the launcher records `files` but grants nothing: "no file service yet")
   and set `ULTAR_AREAS=/areas`; manifest `apps/ultar-files/ultar-app.toml` declares
   `[open] schemes = ["trash", "file", "files"]`, `multi_window = true`.
2. **Bus contracts**: `launcher.start` v2 and `volumes.state`/`volumes.eject` are not on
   `main`; `service/bus.rs::contract` mirrors their wire layout (same fields, order, versions;
   round-trip test). On integration with the launcher/system-services branches replace
   `contract::*` by `ultar_bus::messages::{launcher, volumes}`. Bus policy for
   `org.ultar.files`: `launcher.start`, `launcher.list`, `volumes.eject`, subscribe
   `volumes.state`, `notify.post`.
3. **Compositor**: the desktop surface (`--desktop`) uses `Role::Background`, which needs
   `Rights::SHELL`. Least privilege would be a right for the background only; until then the
   session starts `ultar-files --desktop` with `ui.shell` or the desktop icons are not shown.
   The shell (`ultar-desktop`) must not open its own background on the desktop shape.
4. **Default apps**: `model::FileKind::default_app` (office, images, media, web) until
   Settings → Apps publishes the per-type choice.

## Changes outside the crate (additive)
- `platform/ultar-ui`: `ListItem::tile()` / `ListItem::thumbnail()` (new public fields
  `tile`, `thumbnail` – struct literals need them), `widgets::Thumbnail`, `TILE_WIDTH`,
  `Canvas::draw_rgba`, `Painter::image`. Tile = board file tile (104 wide, 48 box, 12 caption,
  selected = hover + control_border), selection/keyboard/menus/drag of the list items.
- `platform/ultar-surface` `UiWindow`: `take_unhandled_keys()` (keys the tree did not use:
  app shortcuts), `last_input_was_key()` (Enter vs click on an item), `set_shortcuts(filter)`
  (Alt+arrows reach the app before the tree moves focus with them).
- Workspace member `apps/ultar-files`; `design/conformance/coverage.toml` entries of
  `files.*`/`desk.icon`; `tools/design-conformance/run.sh` runs the app's conformance and
  click tests.

## Shortcuts
Ctrl+C/X/V copy/cut/paste, Ctrl+Z undo, Ctrl+A select all, Ctrl+Shift+N new folder, Delete
to trash (in the trash: delete permanently, asks), Alt+Left/Right back/forward, Alt+Up and
Backspace parent folder, Ctrl+1/2 grid/list, Esc clears the search, else the selection.
Drag an item onto a folder tile or a sidebar place: move; onto Trash: to the trash.

## Tests (`cargo test -p ultar-files`)
Gate on B2 (Rust 1.97), 3 runs: **39 passed, 0 failed** each (lib 8, fsops 12, clicks 13, design_conformance 3, screens 3); fmt and clippy `-D warnings` clean (ultar-files, ultar-ui, ultar-surface); `tools/design-conformance/run.sh`: OK (Chrome not on the builder: board rendering skipped). `ultar-check all`: the only new violation is `layers` apps → `platform/ultar-surface` (same as the Settings app: there is no `platform/app-api` yet – allow it in `tools/check-layers.toml` or add the API); the base's own 21 kernel/legacy violations remain. `ultar-ui` widget_snapshots `icon-set-normal` differs by 19–21 px on B2 also on the unchanged base (host-specific; passes on B4). Files: unit tests (names, locations, areas, URIs,
contract round trip), `tests/fsops.rs` (areas, symlink safety, copy policies, into-itself,
partial failures, permission errors, cancel without leftovers, move across mounts, trash
put/restore/purge/empty with Arabic names, damaged records + fuzz, undo of every step kind,
search by name and content, thumbnails + fuzz + size limit), `tests/clicks.rs` (every
`files.*`/`desk.icon` entry plus empty trash, copy/paste/conflict/undo, drag and drop,
permission error), `tests/screens.rs` (window grid/list/properties/trash/permission and desk
and compact in ar/en × dark/light rendered headless, mirroring, accessible names; pictures
with `ULTAR_FILES_SHOTS=<dir>`), `tests/design_conformance.rs` (sizes/gaps/paddings read
from the board HTML of Main and DesktopTop, board labels = accessible names).

## Not done / different (honest)
- `files.back`/`files.forward`: in the app's toolbar, not in the title bar (no client header
  protocol in the compositor) → coverage `different`.
- `files.eject`: "Eject anyway" and naming the app that holds files need a force flag and the
  holder in `volumes.eject` → `different`.
- `files.item`: Space preview is missing (no previewer) → `different`. F2 rename: the toolkit
  has no function keys (rename is in the menu and dialog).
- New folder: the name is not *selected* in the field (toolkit text fields have no selection).
- Drag to other windows/the bar's trash: no cross-surface drag protocol in the compositor.
- Desktop icons on DesktopTop start 24 below the screen top, not below the top panel (the
  background surface does not know the panel height).
- Network places, tabs, archives, multi-rename, permission editing (DESKTOP-SPEC §9) are not in
  this round. Trash on removable drives: items are copied into the user's trash.
- Snapshot reference images are not committed (binary files do not go through the patch);
  `screens.rs` checks paint, mirroring and names, and writes pictures on request.
