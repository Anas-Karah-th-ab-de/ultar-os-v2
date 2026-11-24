# ultar-web – the Ultar Browser window (integration notes, wave 8)

Patch: `/home/claude/agents/w8/web/CHANGES.patch` (`patch -p1` against repo commit `d00853c`).
Scope (owner's budget cut, 2026-11-24): the browser **window** per the boards, the browser core,
`trait WebEngine` with a headless test engine. The WPE WebKit engine itself is a separate project
later – its plan is §6 below; nothing of it is built or shipped by this patch.

## 1. What changed

| Where | Change |
|---|---|
| `apps/ultar-web` | rewritten: the old copy (WPE FFI in Rust, own framebuffer compositor, `#[path]` includes of ultar-shell, ~5300 lines with 160 `unsafe`) is replaced by a pure-Rust browser core, `#![forbid(unsafe_code)]`, static, on the compositor (`ultar-surface`) with the window drawn by `ultar-ui` |
| `platform/ultar-ui` (additive) | `Widget::Picture` + `Ui::{set_picture, write_picture, clear_picture, picture_size}` (pixels an app supplies: the web page), `Canvas::{from_bgrx, write_bgrx}`, `Painter::draw_canvas`; `Card::region(fill, edge)` (`CardTone::Region`), `Card::chip(chosen)` (`CardTone::Chip`); `ButtonVariant::Tab(bool)` (closable document tab: padding 12, edge at the start, semibold when shown, role `Tab` + `selected`); `IconButton::status(icon, colour)` (two icons, 52 wide: the tracking-protection button); new field `IconButton::status` |
| `tools/check-layers.toml` | apps → `platform/ultar-surface` allowed (until `platform/app-api` exists) |
| `tools/check-baseline.toml` | every `apps/ultar-web/*` entry removed (0 violations left there) |
| `design/conformance/coverage.toml` | the 9 `browser.*` actions: `missing` → `ok` with implementation and test |
| `tools/design-conformance/browser_boxes.py` (new), `run.sh` | board boxes of the browser window → `apps/ultar-web/tests/fixtures/browser-board.tsv` (`--check` in run.sh), and `cargo test -p ultar-web --test design_conformance --test clicks` |

Breaking for exhaustive matches in other crates: new variants `Widget::Picture`,
`ButtonVariant::Tab`, `CardTone::{Region, Chip}`; struct literals of `IconButton` need `status`
(none exist outside ultar-ui).

## 2. Layout of the crate

| Module | Purpose |
|---|---|
| `engine` | `trait WebEngine` (tabs, load, back/forward/reload/stop, resize, visibility, pointer, keys, protection per tab, permission answers, download cancel, `poll` → `EngineEvent`, `frame` → BGRX pixels, `wake_fd`) |
| `engine::test_engine` | deterministic headless engine: solid-colour pages, history, scripted downloads / permission requests / blocked counts / new-tab requests / crashes by the address path |
| `engine::remote` + `ipc` + `cell` | the out-of-process engine side, ready for the WPE host: framed binary protocol (`ipc`, fuzzed), frames as shared memory passed with `SCM_RIGHTS` and read by damaged rows (`pread`, no mapping, no unsafe), host started with the socket as stdin, `env_clear`; the page cell description (`CellSpec::page`: bundle read-only, profile + downloads read-write, network; validated, fail closed) |
| `model` | tabs, history (5000, Arabic-tolerant suggestions), downloads (100, page file names made safe), site permissions (default ask; private answers in memory only), settings (search engine, protection, per-site exceptions); files written atomically (`store`) |
| `url` | typed text → address or search; `javascript:`/`data:`/`file:` refused, control characters, length 8192, hosts checked, fuzzed |
| `filter` | EasyList/EasyPrivacy → WebKit content-blocker JSON (port of the old `ultar-web-filter`, 150 000-rule limit keeps exceptions); bin `ultar-web-filter` |
| `chrome` | the window tree per board *DesktopBottom* (tab strip 48+1 `raised`, chips, toolbar 48+1, start page) and `panels` (page panel, download list, menu, settings, permission prompt) |
| `app` | `Browser<E: WebEngine>`: clicks, keys, pointer, engine events, overlays, page picture and size; `Handoff` ("Show in folder" → Files app `ultar-files --select`, toast when it is missing) |
| `window` | compositor loop: one toplevel, `ui.paint` into a BGRX canvas, `Window::present` (sealed shared memory where the kernel has it), waits on compositor + engine sockets together |
| `bin/ultar-web` | `ultar-web [ADDRESS] [--socket] [--engine] [--profile] [--downloads] [--filter] [--no-cell] [--lang] [--light] [--private]` |

The page is a `Picture` node of the window tree: menus, panels, dialogs and their shadows are
painted over it by the toolkit, under a dialog it is dimmed with everything else.

## 3. Clicks (docs/ui/CLICKS.md) and keys

`browser.tab` show · middle click closes; `browser.tabclose` (Ctrl+W; the last tab closes the
window); `browser.newtab` (Ctrl+T; caret in the start page's field); Ctrl+Shift+N private tab;
`browser.back`/`forward` (Alt+Left/Right, disabled without history); `browser.reload` (F5; while
loading the same place is "Stop loading", Esc stops too); `browser.address` (both fields: Enter
opens or searches, refused input is the field's error message, Ctrl+L, Down opens history
suggestions); `browser.shield` (page panel: blocked count when the engine counts, "Protection for
this site" – switching reloads the page –, camera/microphone/location/notifications Ask/Allow/Block,
connection state; opening switches nothing); `browser.downloads` (list with progress, Open – in a
new tab –, Show in folder, Cancel, Clear). Browser menu: Menu key, F10, Alt+F, secondary click on
the tab strip or toolbar (New tab, New private tab, Downloads, page panel, Settings). Permission
prompt: modal, "Block" focused and Esc, "Remember for this site" (disabled in private tabs); at
most 16 requests wait, more are refused.

## 4. Tests (local x86_64, `-j1`)

| Target | Tests |
|---|---|
| `ultar-web` unit (`src/`) | 24 (url incl. fuzz 20 000, ipc incl. fuzz 30 000, store, settings, permissions, history, downloads, tabs, cell, filter, test engine, catalogs, handoff) |
| `tests/clicks.rs` | 11 – every `browser.*` click, menu, settings, shortcuts, prompts, private tabs, page input, crash, suggestions |
| `tests/design_conformance.rs` | 2 – 12 board elements' boxes ±1 point, disabled states, 11 colour samples (strip, lines, chips, field, start page, protection green) |
| `tests/ui_headless.rs` | 3 – ar/en × dark/light: theme colours, RTL mirroring, page pixels in the window, roles/names/translations |
| `tests/remote_engine.rs` | 3 – stand-in host: ready, commands, shared-memory frame via SCM_RIGHTS read by rows, protocol violations, missing host |
| `ultar-ui tests/w8_browser_bricks.rs` | 2 – picture, region/chip, tab button, status button |

Gate results: see the coordinator report (fmt, clippy `-D warnings`, tests ×3 on a builder,
`ultar-check all`, `tools/design-conformance/run.sh`). `ultar-check all` reports only the kernel
findings that are already on the base commit (`kernel/ultar-kern/src/{main,selftest,pci}.rs`).

## 5. Not done / open

- **Engine:** no real web engine is built (owner's decision). `RemoteEngine` speaks the protocol
  but there is no host program yet; `bin/ultar-web` fails with "engine host could not start" until
  it exists. The test engine is the only engine.
- **Menu button:** the board has no menu button; the menu is reachable by keyboard and secondary
  click. Proposal for the boards: a `browser.menu` icon button at the end of the toolbar.
- **Permission prompt** is the browser's own modal dialog. A system-wide prompt (`Role::SystemDialog`)
  needs a permission service with the right `ui.system-dialog`; the browser is an app and must not
  hold that right. When the service exists, `Overlay::Prompt` is replaced by a bus request.
- **Show in folder** needs the Files app (`/usr/bin/ultar-files --select FILE`, not in v2 yet): a
  toast says it is missing. Open uses a `file://` tab (works once an engine renders files).
- Blocked counts appear only for engines that report them (WPE's public API has no counter).
- Not tested on a real display/compositor session, not on aarch64, not on the Ultar kernel.
- `scripts/build-ultar-web.sh` and `tools/ultar-web-br/` still describe the old in-process WPE
  build; they are rewritten with the engine project (§6).
- DESKTOP-SPEC §11 names Servo as the engine; the owner asked for WPE behind the trait. Both fit
  `trait WebEngine`; the decision belongs in the spec.

## 6. Plan: the WPE WebKit engine (separate project)

1. **Runtime** (`tools/ultar-web-br`, Buildroot 2026.08.2, WPE WebKit 2.54.3 with WPEPlatform,
   CPU rendering via patch 0002): add `ultar_x86_64_defconfig` (x86-64-v2, kernel headers 5.10,
   no v4l2 plugins) next to the aarch64 one. Build on B2 (`192.168.100.8`, 728 GB free):
   ~1.5–2 h with 16 threads (host gcc, then WebKit ~1 h). Builder needs `rsync bc cpio unzip file`
   (installed on B2 on 2026-11-24). The aarch64 output exists on the PC WSL in
   `/home/ak/ultar-web-build/out-aarch64` (user `ak`, not root).
2. **Engine host** `external/wpe-webkit/host/` (C, because `ultar-check unsafe` forbids `unsafe`
   outside kernel/drivers/HAL and the glue is GObject subclassing): `proto.{h,c}` mirroring
   `src/ipc` (kinds 1–16 / 101–114), `platform.c` (UltarDisplay/View/Toplevel/Screen;
   `render_buffer` copies the `WPEBufferSHM` into a per-tab memfd, sends `Buffer` once per size
   with `SCM_RIGHTS`, then `Frame`; holds the next buffer until `FrameDone`), `host.c` (one
   `WebKitWebView` per tab, persistent `WebKitNetworkSession` in the profile, ephemeral for private
   tabs; signals: `load-changed`, `notify::title/uri/estimated-load-progress`,
   `permission-request` → `Permission`, `create` → `NewTab`, `web-process-terminated` → `Crashed`,
   network session `download-started` → `decide-destination` into the downloads folder with
   `safe_name` rules → `DownloadStarted/Progress/Finished`; content filter from
   `share/ultar-web/filter.json` via `WebKitUserContentFilterStore`; keys via
   `wpe_unicode_to_keyval` + evdev table, keycode = evdev + 8; modifiers
   `WPE_MODIFIER_KEYBOARD_{CONTROL=1,SHIFT=2,ALT=4,META=8}`), `cell.c` executing `--cell-ro/-rw/
   --cell-net` before WebKit starts: `unshare(NEWNS|NEWPID|NEWIPC[|NEWUSER without root])`, fork
   (parent waits), tmpfs root, binds at the same paths, the bundle's `libexec/wpe-webkit-2.0` and
   `lib/wpe-webkit-2.0` bound at WebKit's fixed `/usr/...` paths, `/tmp` and `/dev/shm` tmpfs,
   `/dev/{null,zero,full,random,urandom}`, `/etc/{resolv.conf,hosts,nsswitch.conf,localtime}`,
   `/sys/devices/system/cpu` read-only, `pivot_root`, new `/proc`, root read-only, no new
   privileges, all capabilities and Ultar rights (`prctl 0x554c5201`) dropped; any failure exits
   (fail closed). Memory: WebKit memory-pressure settings (RLIMIT_AS breaks the gigacage).
3. **Bundle and package:** rewrite `scripts/build-ultar-web.sh` to build the host with the
   Buildroot cross gcc, patch interpreters/RUNPATH into `/usr/lib/ultar/web`, compile the filter
   lists with `ultar-web-filter`, and pack the bundle as a signed `.uap` (`ultar-pkg`, kind
   `native`, files below `/usr/lib/ultar/web`) – too large for the base image;
   `make-vmware.sh --with-web` would add the package.
4. **Tests on a builder:** a Rust integration test (ignored unless `ULTAR_WEB_ENGINE` points at
   the host) serving a page from `127.0.0.1` and checking a rendered pixel through
   `RemoteEngine::start`; the cell checked from inside (no `/home`, no other processes in `/proc`).
5. **Ultar kernel:** needs for WebKit: dynamic loading (glibc `ld.so`, `mmap` of shared objects),
   threads/futex, `memfd_create`, `SCM_RIGHTS` (not in the kernel yet per
   kernel-isolation-design.md §4), `epoll`/`eventfd`, `socketpair`, `clone` of the web/network
   processes. To be measured in QEMU before any promise.
