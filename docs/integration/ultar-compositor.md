# ultar-compositor – integration notes (P5 compositor core)

Design, protocol, security model and kernel gaps: `docs/COMPOSITOR.md`. Patch against a5eb384:
`CHANGES.patch` (`patch -p1` in the repository root) plus three binary snapshot files (list below).

## 1. What exists

| Crate / file | Area | Content |
|---|---|---|
| `system/hal/ultar-hal-gfx` (new, lib) | HAL (3) | `shm` (sealed memfd buffers, read-only mapping + copy), `fdpass` (`SCM_RIGHTS`, ≤ 4 per read), `kms::Kms` (DRM/KMS dumb buffers, every connected connector, implements `ultar_hal::Display`), `fbdev::FbDev` (`fb0`, `pwrite` of damaged rows, implements `Display`), `evdev::Evdev` + `Decoder` (raw `input_event`, keys, rel/abs pointer, single/multi touch, implements `ultar_hal::Input`). The only `unsafe` of the stack (2 mmaps, 2 munmaps, 1 copy, fbdev/evdev `ioctl`s), each with `SAFETY:` |
| `platform/ultar-surface` (new, lib) | platform (5) | `wire` (frames, bounded reader/writer), `proto` (28 requests, 30 events, types, rights), `client::{Client, Window, WindowConfig, ClientError}`, `ui::{UiWindow, translate, device_size}` (feature `ui`, default) |
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
2. Service file for `ultar-init`: `rootfs/etc/ultar/services/compositor.toml` (fix round 1):
   `manual = true` (until `ultar-shell` moves onto it – both would drive the display), own user
   811, right `raw_device` only, `notify`, socket directory `/run/ultar/ui` (`public_dirs`),
   `ultar-compositor --socket /run/ultar/ui/compositor.sock --rights /etc/ultar/compositor-rights.toml`;
   checked by `ultar-init`'s `tests/shipped_services.rs`. `--headless WxH` runs without
   hardware, `--inline` forces the inline transport, `--lang ar|en|de`, `--light`, `--no-cursor`,
   `--kms PATH`, `--fb PATH`, `--allow-root` (development only).
3. Rights file (`uid = "ui.shell,ui.capture"`; names `ui.shell`, `ui.input-method`,
   `ui.lock-screen`, `ui.capture`, `ui.overlay`, `ui.system-dialog`): shipped empty
   (`rootfs/etc/ultar/compositor-rights.toml`); root and `u32::MAX` are refused, a uid twice is
   refused (`--allow-root` accepts uid 0 for development). See §8 "Identities" for what is
   missing.
4. Apps: `Client::connect(path, app_id)` → `Window::create` + `Window::present(pixels, size,
   damage)` for any renderer, or `UiWindow::new(client, &WindowConfig::toplevel(title), ui)` +
   `UiWindow::dispatch(timeout)` for an `ultar-ui` tree (returns the toolkit's actions).
   Reconnect after a compositor restart: `Client::connect_retry`. Apps should depend on
   `ultar-surface` through `platform/app-api` when that crate exists (the layer table allows apps
   only `app-api` and `ultar-ui`).
5. Shell: bar/dock = `Role::Panel{edge, size}`, notifications = `Role::Overlay`, lock screen =
   `Role::Lock` + `Unlock{surface}` after the password check, keyboard = `Role::InputPanel` +
   `ImeCommit`, wallpaper = `Role::Background`, password/permission prompts of system services =
   `Role::SystemDialog` (title = requesting app) – each needs its right in the rights file. The
   shell reacts to `SessionLocked{true}` by starting the lock screen client (Ctrl+Alt+Del / Super+L
   lock in the compositor itself). Launcher: on a click, `Client::request_activation(serial)` →
   `ActivationToken{token}` → start the app with `ULTAR_ACTIVATION_TOKEN=<token>`; the client
   library presents it with the first toplevel (`Activate`), which then gets the focus.
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

Fix round 1 (`FIX1.patch`) changes no binary file; the snapshots stay as they are (the tests now
click the newer window before the snapshot, because a new window no longer takes the focus by
itself).


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

## 8. Fix round 1 (review `docs/review/compositor-runde-1.md`) – `FIX1.patch`

Base: repository HEAD (after the UI round-4 fixes). The reviewer's six test files are part of the
suite (`tests/review_{security,robustness,outputs,fds,bench}.rs`, `ultar-hal-gfx/tests/review_hal.rs`);
all 17 FINDING tests pass, the SOUND ones still pass. `review_bench.rs` is now an asserted budget.

| Finding | Fix (root) | Tests |
|---|---|---|
| C1 lock fails open | `Session::locked` is compositor state: set by a lock commit and by Ctrl+Alt+Del / Super+L, cleared only by `Unlock{surface}` from a `ui.lock-screen` client showing a lock surface; dead lock client → ground colour, no input, a new lock client takes over | `lock_screen_stays_locked_when_its_process_dies`, `unlock_needs_the_lock_screen_and_a_new_lock_client_takes_over` |
| C2 IME to the app | one `text_target()` (lock surface while locked, else system dialog, else focus) for keys, `ImeCommit` and `ImeState` | `on_screen_keyboard_types_into_the_lock_screen_while_locked` |
| C3 no trusted path | popups drawn right after their parent window (below panels), clamped and cut to the work area of the parent's output, ≤ 8 per toplevel; hit test skips alpha-0 pixels; secure attention keys; `Role::SystemDialog` (right `ui.system-dialog`): above panels, modal, lock icon in a compositor title bar, everything below (panels too) dimmed | `an_app_popup_cannot_cover_the_shell_panel`, `an_invisible_popup_cannot_take_other_apps_clicks`, `secure_attention_keys_lock_and_are_never_forwarded`, `system_dialogs_are_above_panels_dim_the_rest_and_are_modal`, `decor::system_dialogs_carry_the_lock_mark` |
| M1 focus stealing | focus on map only if nothing/its client has it, a user action on the client since the last focus change (≤ 10 s), or an activation token (`RequestActivation` → `ActivationToken` → `Activate`, single use, 30 s, ≤ 16); an app's own restore from minimised does not focus | both `a_background_window_cannot_steal_focus_*`, `activation_tokens_move_the_focus_once`, `windows_map_cascade_and_only_the_first_takes_the_focus` |
| M2 client slots | uid without rights: ≤ 8 connections and never the last 8 of 64 slots | `one_uid_cannot_take_every_client_slot`, `reserved_client_slots_stay_for_system_clients` |
| M3 memory | 1 GiB for all clients (buffer creation and commit growth); sparse memfds refused (`st_blocks`), `SharedBuffer::create` `fallocate`s | `total_client_pixel_memory_is_bounded`, `sparse_buffers_are_refused`, `shm::sparse_memory_files_are_refused` |
| M4 pacing | callbacks without damage answered once per refresh interval (moves the pacing clock) | `frame_callbacks_without_damage_are_paced` |
| M5 flow control | a transfer's source is not read (nor polled) while a reader has > 2 MiB queued | `a_large_paste_does_not_disconnect_the_reader` |
| M6 input hot-plug | dead devices: held keys released, `DeviceRemoved`, out of the poll set; netlink uevents (input/usb/hid) and SIGHUP rescan `/dev/input`; the backend refreshes its poll set | `a_hung_up_input_device_is_dropped`, `evdev` unit tests, `uevent` parser test |
| M7 frame cost | popup cap + overdraw budget (translucent area of apps without rights: 2 screens per uid, 3 for all, bottom to top; the rest is left out of the scene); `opt-level = 3` for the compositor in release | `review_bench::translucent_stack_frame_cost_is_bounded` (asserted ≤ 16 ms CPU p99) |
| M8 rights/uids | rights file refuses uid 0 (unless `--allow-root`), `u32::MAX` and duplicates; the compositor runs as its own user 811 (`compositor.toml`) | `config::tests::rights_file`, `shipped_services` |
| m1 | every invisible format character removed from titles | `titles_lose_every_bidi_format_character`, `text::titles_lose_format_characters` |
| m2 | `Output::user_scale_milli` kept apart from the effective (mirror) scale | `mirror_then_extend_restores_the_scale` |
| m3 | `SYN_DROPPED` releases held keys and buttons | `syn_dropped_releases_held_keys` |
| m4 | pointer to the primary's middle when its display went | `unplugging_the_display_under_window_and_pointer` |
| m5 | move offset from the press point | `a_title_drag_keeps_the_grabbed_point_under_the_pointer` |
| m6 | recording indicator (compositor dot, 2 s after), capture paused while locked, key repeat/release to the press's surface, grab/drag/touch end on lock, shadow cache per output, fbdev `x/yoffset` + 32-bit `line_length`, rights file `u32::MAX`/duplicates | `capture_shows_the_indicator_and_pauses_while_locked`, `a_key_release_goes_where_the_press_went`, `fbdev` origin test, `rights_file` |

**Protocol (breaking for nobody outside this stack, version stays 1):** requests `Unlock` (26),
`RequestActivation` (27), `Activate` (28); events `ActivationToken` (29), `SessionLocked` (30);
`Role::SystemDialog` (7), `Rights::SYSTEM_DIALOG` (32, `ALL` = 63). Client library:
`ACTIVATION_ENV`, `Client::take_activation_token`, `Client::request_activation`.
**Compositor API:** `Limits` gained `clients_per_uid`, `reserved_clients`, `total_bytes`, `popups`,
`transfer_queue`; `RightsTable::parse_with(text, allow_root)`, `RightsError::{Duplicate,
Privileged}`; `State::{session, lock, locked, lock_surface, system_dialog, text_target,
scene_parts, total_bytes, throttled, recording, tick_indicator}`; `Window::{secure,
activate_on_map}`; `TitleKey::secure`; `Backend::rescan_input` (default: nothing); module
`cursor` (pointer and indicator images; `render` re-exports the pointer items).
**HAL:** `ultar_hal_gfx::uevent::{UeventSocket, Uevent}`, `Evdev::{empty, rescan}`,
`GfxError::Sparse`.

**Identities (M8) – what is still missing:** the compositor tells clients apart by kernel uid
only. Today the surface (`ultar-shell`: lock screen, keyboard, bar) runs as root and apps are
started by it without an id of their own, so the shipped rights file is empty and every app has
the same (no) rights. Needed: (1) the app launcher (`ultar-apps` / the shell's launcher) starts
each app as its own uid (kernel `SET_ID` and `ultar-sys::switch_identity` exist), (2) the
installer writes the app's rights line from its manifest, (3) the shell's lock screen, keyboard
and bar run as their own users with lines in the rights file, (4) `ultar-init` (or the shell)
restarts a dead lock screen client after `SessionLocked{true}`.

**Gate (fix round 1, 23.11.2026, x86_64 Linux, `-j1`, debug, `CARGO_INCREMENTAL=0`):**
`cargo test -p ultar-hal-gfx -p ultar-surface -p ultar-compositor`: **129 passed, 0 failed,
3 ignored** (the release-only budgets), identical in three runs; `ultar-init --test
shipped_services` 5 passed (with `compositor.toml`). `cargo fmt --check` clean; `cargo clippy
--all-targets -- -D warnings` clean for the three crates (also `ultar-surface
--no-default-features --lib`); `ultar-check all`: ok, 5556 baselined as on HEAD (no new baseline
entry), 141 warnings (139 on HEAD; "file getting long" warnings only).
Release budgets (3 runs, thread CPU p99, best of three rounds): full HD 4 windows 2.5 / 1.7 /
1.6 ms (budget 4 ms), caret 10 / 5 / 6 µs (0.3 ms), **hostile scene** (3 uids × 2 connections,
each a window + 8 translucent full-screen popups + 6 translucent full-screen toplevels: 9 layers
shown, 81 left out) 10.5 / 6.0 / 8.4 ms (budget 16 ms; before the fix 282 ms median with 58
layers). Not run in this round: the QEMU run on the Ultar kernel (the uevent socket and
`fallocate` there are untested; without uevents the backend still works and SIGHUP rescans),
real input hot-plug hardware, a real lock screen / dialog service (none exists yet).

