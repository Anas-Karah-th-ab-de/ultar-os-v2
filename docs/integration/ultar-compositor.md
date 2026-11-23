# ultar-compositor – integration notes (P5 compositor core)

Design, protocol, security model and kernel gaps: `docs/COMPOSITOR.md`. Patch against a5eb384:
`CHANGES.patch` (`patch -p1` in the repository root) plus three binary snapshot files (list below).

## 1. What exists

| Crate / file | Area | Content |
|---|---|---|
| `system/hal/ultar-hal-gfx` (new, lib) | HAL (3) | `shm` (sealed memfd buffers, read-only mapping + copy), `fdpass` (`SCM_RIGHTS`, ≤ 4 per read), `kms::Kms` (DRM/KMS dumb buffers, every connected connector, implements `ultar_hal::Display`), `fbdev::FbDev` (`fb0`, `pwrite` of damaged rows, implements `Display`), `evdev::Evdev` + `Decoder` (raw `input_event`, keys, rel/abs pointer, single/multi touch, implements `ultar_hal::Input`). The only `unsafe` of the stack (2 mmaps, 2 munmaps, 1 copy, fbdev/evdev `ioctl`s), each with `SAFETY:` |
| `platform/ultar-surface` (new, lib) | platform (5) | `wire` (frames, bounded reader/writer), `proto` (25 requests, 28 events, types, rights), `client::{Client, Window, WindowConfig, ClientError}`, `ui::{UiWindow, translate, device_size}` (feature `ui`, default) |
| `platform/ultar-compositor` (new, lib + bins `ultar-compositor`, test helper `ultar-compositor-testclient`) | platform (5) | `server::Compositor` (loop), `state::*` (clients, requests, windows, layers, input, keys, grabs, transfers, outputs), `render::Renderer`, `pixels`, `decor`, `wm`, `keymap`, `output::Layout`, `backend::{Backend, Headless, HalBackend}`, `config::{Config, Limits, RightsTable}`, `msg` (ar/en/de) |
| `docs/COMPOSITOR.md` (new) | docs | the design |
| `tools/kern-test/compositor-qemu.py` (new) | tools | QEMU run on the Ultar kernel + screenshot + verdict |
| `Cargo.toml` (root) | – | 3 workspace members added |
| `Cargo.lock` | – | the 3 new packages (no new external crate: `rustix` 1.1.6 and `drm` 0.15 were already locked) |
| `tools/check-layers.toml` | – | platform may use `system/hal/ultar-hal-gfx` (as it may `ultar-hal-system`) – reason in the file |
| `kernel/ultar-kern/src/selftest.rs` | kernel | one entry in the self-test program list (`ultar-compositor-testclient kerncheck`) – the only kernel touch |
| `scripts/build-kern.sh` | – | copies the two static binaries into the image when they are built |

`unsafe`: none in `ultar-surface`, `ultar-compositor` (`#![forbid(unsafe_code)]`). C libraries: none
(`rustix` linux_raw, `drm` pure Rust). Static: `RUSTFLAGS="-C target-feature=+crt-static" cargo build
--release -p ultar-compositor --bins --target x86_64-unknown-linux-gnu` → static-pie, 6.3 MB each
(fonts of `ultar-ui` included).

## 2. Wiring

1. Apply the patch; copy the binary files (§6). The three crates are workspace members already.
2. Service file for `ultar-init` (not written – `rootfs/etc/ultar/services/` belongs to the service
   agent): `ultar-compositor --socket /run/ultar/ui/compositor --rights /etc/ultar/compositor-rights.toml`,
   user `ui`, readiness via `ULTAR_READY_FD`, right `RAW_DEVICE`. `--headless WxH` runs without
   hardware, `--inline` forces the inline transport, `--lang ar|en|de`, `--light`, `--no-cursor`,
   `--kms PATH`, `--fb PATH`, `--allow-root` (development only).
3. Rights file (`uid = "ui.shell,ui.capture"`; names `ui.shell`, `ui.input-method`,
   `ui.lock-screen`, `ui.capture`, `ui.overlay`): to be written by the app installer
   (`ultar-apps`) from the app manifest when apps get their own uids (kernel gap K9).
4. Apps: `Client::connect(path, app_id)` → `Window::create` + `Window::present(pixels, size,
   damage)` for any renderer, or `UiWindow::new(client, &WindowConfig::toplevel(title), ui)` +
   `UiWindow::dispatch(timeout)` for an `ultar-ui` tree (returns the toolkit's actions).
   Reconnect after a compositor restart: `Client::connect_retry`. Apps should depend on
   `ultar-surface` through `platform/app-api` when that crate exists (the layer table allows apps
   only `app-api` and `ultar-ui`).
5. Shell: bar/dock = `Role::Panel{edge, size}`, notifications = `Role::Overlay`, lock screen =
   `Role::Lock`, keyboard = `Role::InputPanel` + `ImeCommit`, wallpaper = `Role::Background` – each
   needs its right in the rights file.
6. Ultar kernel CI: build the two static binaries, `IMG=1 scripts/build-kern.sh`, then
   `python3 tools/kern-test/compositor-qemu.py out/ultar-kern-esp.img /tmp/shot.ppm` (exit 0 + line
   `COMPOSITOR-CHECK: OK windows=2 …`). The self-test entry runs the same scenario without
   `--stay` when the self-test file is on the stick.

## 3. Public API (summary)

```rust
// compositor
Compositor::new(Config, Box<dyn Backend>, Box<dyn Clock>) · set_listener(UnixListener) · set_wake(UnixStream)
  · add_stream(UnixStream) · dispatch(Option<Duration>) · render_now() · render_reference(OutputId)
  · frame(OutputId) · inject(&[InputEvent]) · rescan_outputs() · take_wake() · flush() · stats · state
Config { lang, theme, layouts, limits: Limits, inline_only, rights: RightsTable, cursor }
Backend trait: outputs · present(hal, &Image, &[Rect]) · input_fds · read_input · hotplug
Headless::new(&[(w,h)]) · push_input · plug · unplug        HalBackend::open(&OutputKind::{Auto,Kms,Framebuffer})
State: windows, layers, focus, pointer, keyboard, layout, damage; hit_test, scene, work_area,
  set_window_state, focus_window, unminimize, request_close, outputs_changed, damage_all
Layout: add/remove/set_scale/set_primary/set_mode(DisplayMode::{Extend,Mirror,Single}) · primary · output_at · bounds
wm: snap_rect · snap_target · keyboard_snap · place_new · constrain · clamp_size
// client
Client::connect/connect_retry/with_stream · send · send_with_fd · next_event · transport · rights · outputs
Window::create · present · handle · can_draw · configure · close_requested · destroy
UiWindow::new · dispatch · draw · ui_mut · client_mut · window
```

## 4. Tests (run 3× on 23.11.2026, x86_64 Linux, `-j1`, debug, `CARGO_INCREMENTAL=0`)

`cargo test -p ultar-hal-gfx -p ultar-surface -p ultar-compositor`: **93 passed, 0 failed,
2 ignored** (the release-only budgets) – identical in all three runs.

| Target | Tests | What |
|---|---|---|
| `ultar-hal-gfx` unit | 20 | shm create/seal/map/copy, unsealed/foreign/too small refused, fd passing incl. flood, fbdev geometry + damage writes, KMS mode/copy, evdev decoder (keys, rel, abs, single/multi touch, SYN_DROPPED, bounds) |
| `ultar-surface` unit | 12 | every request/event round trip, truncation/trailing/limits, wire, client errors, damage clipping, toolkit translation |
| `ultar-compositor` unit | 32 + 1 (bin) | geometry/regions, pixel ops (over formula, clipping, scaling, corners, shadow), outputs (DPI scale, extend/mirror/single, primary), wm snapping/placement, keymap (Arabic/English, lam-alef, shortcuts), decorations (RTL buttons, hit test, token colours), rights file, conn, surfaces/commit, transfers, cursor, command line |
| `tests/input_routing.rs` | 9 | focus by click, coordinates relative to the surface, keys only to the focused client (Arabic default), implicit grab, title drag → snap, Super+arrows, double click, close button + Alt+F4 (not passed on), Arabic close button on the left, touch, Alt+Tab, minimise/restore |
| `tests/damage.rs` | 4 | **differential**: 60 random operations, incremental frame == full composition after each, one and two outputs, 125 % scale, killed client; Arabic snapshot |
| `tests/protocol_fuzz.rs` | 7 | random bytes (300 sessions), random requests with hostile fields (40 × 60), huge sizes, order (no Hello, double Hello, id 0, roles without right), unsealed/too small/pipe descriptors, descriptor flood, lying stride, proper shared buffer shows its pixels |
| `tests/processes.rs` | 7 | two client **processes** + PNG snapshot + focus/keys; `ultar-ui` app process (snapshot, button click → action in the app); hostile processes (foreign surface id, capture without right, flood – victim still served); never-reading client dropped; capture with right; kill -9 mid-grab + death mid-frame (half a message); **restart of the real compositor binary**, client reconnects and shows its window again |
| `tests/bench.rs` | 1 + 2 ignored | idle = no frame; full-HD and caret budgets in release |

Release budgets (`cargo test --release -p ultar-compositor --test bench -- --test-threads=1`),
best of 3 rounds × 100 frames, p99 of the thread's CPU time (wall time printed too):

| | budget | measured (3 runs) |
|---|---|---|
| 1920×1080, 1 maximised + 3 floating windows, full damage | 4 ms | CPU 2.1 / 2.7 / 3.0 ms (wall 8.9 / 18.2 / 10.2 ms) |
| caret-sized damage | 0.3 ms | CPU 3 / 10 / 12 µs (wall 4 / 10 / 14 µs) |

Wall time is far above CPU time because the 2-core machine was shared with five other agents
(load ≈ 3); the budget is asserted on CPU time and must be re-measured on a quiet machine / the
test PCs before it is a CI gate on wall time.

Gate: `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean for the three
crates (also `ultar-surface --no-default-features --lib`); `ultar-check all`: ok, 5579 baselined as
before (no new baseline entry), 132 warnings (127 before: five new "file getting long" warnings for
files of 401–479 lines).

Snapshots (checked by eye, then pinned): `two_windows_en.png` (two processes, English),
`two_windows_ar.png` (Arabic: buttons left, close outermost, Arabic title shaped), `ui_app_en.png`
(ultar-ui app process).

## 5. Ultar kernel in QEMU (run on 23.11.2026)

Kernel built from this tree (a5eb384 + the self-test line), static binaries as above, image made
like `build-kern.sh`, `tools/kern-test/compositor-qemu.py` (q35, 2 CPUs, OVMF, std VGA 1280×800):
the compositor opened `fb0` and the input devices with only `RAW_DEVICE` kept, both `ultar-ui`
apps connected over the inline transport, drew (`drawn` ×2 on the console) and were composed with
Arabic decorations, shadows and the pointer. Screenshot via the QEMU monitor (`screendump`):
`ultar-kern-compositor.png` (delivered next to the patch). Verdict line from the screenshot:
`COMPOSITOR-CHECK: OK windows=2 frame=1280x800 pixels={'info': 3132, 'success': 300866}`.

Not achieved there: the in-kernel verdict of `kerncheck` (capture over the protocol) did not
arrive within the wait – console lines of concurrent processes were also seen to go missing, so
the cause is open (suspected kernel gap K11). The kernel self-test entry was added but the
self-test mode itself was **not run**.

## 6. Binary files (not in the text patch)

| File | Copy to |
|---|---|
| `binary/platform/ultar-compositor/tests/snapshots/two_windows_en.png` | `platform/ultar-compositor/tests/snapshots/` |
| `binary/platform/ultar-compositor/tests/snapshots/two_windows_ar.png` | same |
| `binary/platform/ultar-compositor/tests/snapshots/ui_app_en.png` | same |

## 7. Not done / not tested (honest list)

* Kernel gaps K1–K13 (docs/COMPOSITOR.md §3): no shared memory or descriptor passing on the Ultar
  kernel (inline transport there), vfork parent blocked until child exit, no POLLOUT wake-up,
  whole-second poll timeouts, one framebuffer, simple input.
* Not run on real hardware (no DRM device here: `Kms` is only unit-tested; `FbDev` ran on the
  Ultar kernel in QEMU, not on Linux fbdev), not on aarch64, not with touch hardware.
* Input-to-photon latency not measured; wall-time budgets not met on the shared machine
  (CPU-time budgets are).
* No `ultar-bus` contract yet (display settings, "not responding" dialog, screenshots UI).
* P6 items (COMPOSITOR.md §13): overview, workspaces, switcher UI, window menu, snap suggestion,
  keyboard repeat, more layouts, GPU renderer, page flips, direct scan-out, XWayland.
* `ultar-wm` (Smithay-derived, MIT parts) is untouched: nothing was copied from it, so the new
  crates are plain `Proprietary`; `ultar-wm` and `app_desktop.rs` can be retired in P6 as planned.
