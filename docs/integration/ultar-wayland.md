# ultar-wayland – integration notes (FA stage P1: the bridge)

Plan: `docs/FREMD-APPS.md` (apps from other systems, security first). The compositor and its own
protocol: `docs/COMPOSITOR.md`, `docs/integration/ultar-compositor.md`.

## 1. What exists

| Crate / file | Area | Content |
|---|---|---|
| `platform/ultar-wayland` (new, lib + bin `ultar-wayland`) | platform (5) | A Wayland server in pure Rust (`wayland-server` 0.31 with its Rust backend: no libwayland, statically buildable). It serves **one** foreign app cell and passes every window on to `ultar-compositor` as one ordinary client of the compositor's own protocol. |
| `platform/ultar-wayland/examples/uwl-demo.rs` | example | a Wayland app without any library (wire format by hand), for the Ultar kernel where no Ubuntu app exists |
| `tools/kern-test/wayland-e2e.sh` | tools | crate tests + real Ubuntu apps with screenshots; last line `WAYLAND: ALLES OK` |
| `platform/ultar-wayland/examples/uwl-kern.rs` | example | static launcher for the kernel run (compositor, then bridge + demo; no shell needed) |
| `tools/kern-test/wayland-kern.sh`, `wayland-qemu.py` | tools | the static bridge on the Ultar kernel in QEMU, verdict from a screenshot |
| `Cargo.toml` (root) | – | workspace member `platform/ultar-wayland` |

## 2. How it runs

```
ultar-wayland [--compositor <socket>] [--runtime-dir <dir>] [--app-id <id>] -- <program> [args…]
```

* Started inside the app cell, **with the app's uid**: the compositor sees an ordinary client of
  that uid (rights, limits and the window budget of that uid apply; nothing is shared with other
  cells). Defaults: compositor `/run/ultar/ui/compositor.sock`, the system compositor's socket (or `ULTAR_COMPOSITOR_SOCKET`), app id
  from `ULTAR_APP_ID`, runtime dir `$XDG_RUNTIME_DIR` (else `/tmp/ultar-wayland-<uid>`, mode 0700).
* It listens on `<runtime-dir>/wayland-0`, then starts the program with `WAYLAND_DISPLAY=wayland-0`,
  `XDG_SESSION_TYPE=wayland`, `GDK_BACKEND`, `QT_QPA_PLATFORM`, `SDL_VIDEODRIVER`/`SDL_VIDEO_DRIVER`,
  `MOZ_ENABLE_WAYLAND`, … set to Wayland and `DISPLAY`/`WAYLAND_SOCKET` removed. The child is
  started by fork + exec (kernel gap K10: a vfork parent waits for the child on the Ultar kernel).
* It ends with the program and returns its exit code (signal → 128 + n). If the compositor goes
  away, the program is ended and the bridge exits with 70; a program that cannot start gives 127;
  wrong arguments 64. Messages in Arabic, English, German (`ULTAR_LANG`/`LANG`).
* The only sockets: the compositor connection and the own Wayland socket.

## 3. Protocols

| Global | Version | Notes |
|---|---|---|
| `wl_compositor`, `wl_region`, `wl_callback` | 6 | damage and buffer damage, buffer scale/transform-free, frame callbacks (≤ 64 per surface) |
| `wl_subcompositor` | 1 | subsurfaces are blended into their window (≤ 32 children) |
| `wl_shm` | 1 | ARGB8888 / XRGB8888; pixels read with `pread` (no mmap → no SIGBUS from a shrinking file), buffer released at commit |
| `wl_output` | 4 | from the compositor's outputs (scale rounded up to an integer, `preferred_buffer_scale` on v6 surfaces) |
| `wl_seat` | 7 | pointer (enter/leave/motion/button/axis/frame), keyboard with an XKB keymap of three groups (Arabic, English, German, generated from the compositor's own layout tables; the active group follows the compositor's layout switch), touch; key repeat stays in the compositor |
| `xdg_wm_base` | 5 | toplevel: title, min/max size, maximize, fullscreen, minimize, interactive move/resize, configure + ack, close; popups with positioner (anchor, gravity, offset; shown as native popups of the window, closed by an outside click when grabbed); window geometry crops the shadow; pings are passed on (a hung app shows as "not responding") |
| `zxdg_decoration_manager_v1` | 1 | server-side by default; an app may draw its own title bar only with the compositor right `CLIENT_HEADER` |
| `xdg_activation_v1` | 1 | tokens accepted (focus stays the compositor's decision) |
| `wl_data_device_manager` | 3 | text clipboard both ways (`text/plain;charset=utf-8`, `text/plain`, `UTF8_STRING`, `STRING`, ≤ 16 MiB); drag and drop is refused (cancelled at once) |

Not yet: `wp_viewporter`, fractional scale, `zwp_linux_dmabuf` (GPU buffers), text input / IME,
primary selection, drag and drop, Xwayland (X11 apps), layer shell (on purpose: cells get no
panels).

## 4. Security and limits

* A wrong or excessive request ends **that Wayland connection** with a protocol error
  (`wl_display.no_memory` for limits); the bridge and its other connections go on. Errors on new
  objects never leave an object without data (that would abort the server).
* Per bridge: 16 connections; per connection 256 surfaces, 128 pools (≤ 512 MiB each), 1024
  buffers, 256 positioners; 64 input objects and data objects of each kind; titles cut to the
  compositor's limit, control characters removed.
* Every native request stays inside the compositor's limits, because a refused request would end
  the native connection and with it every window of the app: at most 30 native surfaces and 8
  popups, buffers within 7/8 of the compositor's byte budget, sizes ≤ 8192. A frame that does not
  fit is dropped (the window keeps its last picture); a window too many ends the Wayland
  connection with `no_memory`.
* Clipboard: copying and pasting need the keyboard focus and a fresh user action (a press within
  8 s; the compositor allows 10 s). Without them the copy is cancelled and the paste returns
  nothing – the compositor is never asked for something it would refuse.
* Tests: a fixed-seed fuzz test (200 connections, well-formed random requests on objects of the
  right kind, plus random bytes) and limit tests.

## 5. Checks

* `cargo test -p ultar-wayland` – unit tests (args, keymap with real libxkbcommon semantics,
  pixels, popup placement) and end-to-end tests: the real compositor (headless, in-process), the
  bridge as its own process and a raw Wayland client: window with its pixels 1:1, click and key
  (incl. layout switch), maximize → bigger configure → bigger window, clipboard both ways with a
  native app, popup shown/closed by an outside click, Alt+F4 → close → the program's exit code,
  limits, fuzzing. `ULTAR_WAYLAND_SHOTS=<dir>` keeps screenshots.
* `tools/kern-test/wayland-e2e.sh` – the above plus `tests/real.rs` with installed Ubuntu apps
  (each with `WAYLAND_DEBUG=1`, so its own log proves which events arrived): window, click, key,
  typed shell command (terminals), clipboard via `wl-copy`/`wl-paste` (foot), Super+Up resize,
  Alt+F4 close. Last line `WAYLAND: ALLES OK`.
* `tools/kern-test/wayland-kern.sh` – static `ultar-compositor`, `ultar-wayland`, `uwl-demo` on
  the Ultar kernel in QEMU; verdict `WAYLAND-QEMU: OK|FAILED` from a screenshot.

## 6. Results (28.11.2026, server .8: Ubuntu 26.10, commit d011619)

* `cargo test -p ultar-wayland`: 9 unit tests, 5 bridge tests, clipboard, fuzzing (200
  connections, every one ended by a protocol error, the bridge never), 3 limit tests – all ok.
* `tools/kern-test/wayland-e2e.sh`: `WAYLAND-REAL: 6/6 apps OK`, last line `WAYLAND: ALLES OK`.

| App | Window | Click | Key | Typed in shell | Clipboard (wl-copy → wl-paste) | Super+Up resize | Alt+F4 close |
|---|---|---|---|---|---|---|---|
| weston-simple-shm | ok | – (no pointer) | ok | – | – | ok | ok |
| weston-terminal | ok | ok | ok | ok | ok | ok | ok |
| foot | ok | ok | ok | ok | ok | ok | ok |
| gtk4-demo (`GSK_RENDERER=cairo`) | ok | ok | ok | – | – | ok | ok |
| Qt 6 (PyQt6, `QT_QPA_PLATFORM=wayland`) | ok | ok | ok | – | – | ok | ok |
| SDL3 testsprite (`--resizable`) | ok | ok | ok | – | – | ok | ok |

  Screenshots: `out/uwl/e2e/shots/<app>-window.png` and `<app>-maximized.png`.
* Ultar kernel (QEMU, KVM): `tools/kern-test/wayland-kern.sh` → static `ultar-compositor`,
  `ultar-wayland`, `uwl-demo`, started by `uwl-kern`: `WAYLAND-QEMU: OK frame=1280x800
  demo_pixels=350128` (screenshot `out/uwl/qemu.png`). The child of the bridge must be started
  by fork + exec there (K10).

## 7. Open points

* GTK 4 draws its own header bar and does not negotiate xdg-decoration: without the compositor
  right `CLIENT_HEADER` for the app cell the window shows two title bars. Granting that right to
  foreign-app cells is a policy decision of the rights table (stage P2/P3).
* No GPU buffers (`zwp_linux_dmabuf`): apps render in software (GTK with `GSK_RENDERER=cairo`,
  Qt/SDL fall back to shm by themselves).
* No Xwayland (X11-only apps), no text input / IME protocol (the XKB keymap covers ar/en/de),
  no primary selection, no drag and drop, no `wp_viewporter` / fractional scale (integer scale,
  nearest scaling for odd scales).
* Popups: shown as native popups of their window, at most 8 per window, nested popups are placed
  relative to the window; positioner constraints (flip/slide) are not applied – the compositor
  clamps popups into the window.
* The bridge ends with its program; helper processes of the app that still hold a Wayland
  connection (e.g. `wl-copy` serving the clipboard) end with it.
