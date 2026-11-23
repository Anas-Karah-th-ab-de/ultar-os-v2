# Ultar compositor – design (P5 core)

Status: implemented as `platform/ultar-compositor` (service + library), `platform/ultar-surface`
(client library, protocol, `ultar-ui` bridge) and `system/hal/ultar-hal-gfx` (display, input,
shared memory). Plan references: PLAN §3.0 (areas), §3.2, §3.6 (budgets), §3.7 (cells), §8 P5/P6,
§9 decisions 1 and 3; DESKTOP-SPEC §5.

What P5 needs and this delivers: **windows of other processes on the screen**, each app a process of
its own, isolated from the others, with input routed only to it. Desktop mode details that belong to
P6 (overview, workspaces, dock integration, settings UI for displays) are listed in §13 as open.

## 1. Process model

```
ultar-init ──starts──> ultar-compositor   user "ui", right RAW_DEVICE only (ultar-service runtime:
                         │                 signals, readiness, hardening, umask, no root)
                         ├─ owns: display (DRM/KMS or fb0), input devices (evdev / Ultar event0/1)
                         └─ listens: /run/ultar/ui/compositor (mode 0666; identity = kernel peer creds)
app launcher ──starts──> app process (own uid in its cell, own memory group below `apps`)
                         └─ connects with ultar-surface, draws with ultar-ui into its own buffers
shell (bar, dock, quick settings, launcher, lock screen, on-screen keyboard)
                       = ordinary client processes with **rights** (see §8), no special code path
```

* The compositor never runs app code and never parses app content except pixels and the bounded
  protocol below. A crashing app is a closed socket (§10).
* One compositor per seat. It is a service of area 5 (platform) but started like a system service
  (`ultar-service::Runtime`): SIGTERM stops it within the stop deadline, SIGHUP probes the displays
  again (hot-plug on backends without events), readiness goes to `ultar-init` via `ULTAR_READY_FD`.
* Kernel rights: only `RAW_DEVICE` is kept (opening `fb0`/`event*` on the Ultar kernel); every
  other right is dropped at start by the runtime.

## 2. Client protocol (`ultar-surface::proto`)

Own small binary protocol over an `AF_UNIX` stream socket – no Wayland: a fixed, versioned set of 25
requests and 28 events that we can fuzz completely and that the Ultar kernel can carry today.

* Frame: `u32 body length | u16 opcode | u16 reserved (must be 0)` + body (little endian).
  Largest body 1 MiB + 64 (one inline pixel chunk); strings ≤ 4096 bytes (titles 512, app id 64,
  MIME 64), no control characters; ≤ 32 damage rectangles; ≤ 8 MIME types. Decoders refuse
  truncated bodies, trailing bytes, unknown opcodes/enum values (`wire.rs`, `proto/*`).
* First request `Hello{version, app_id, shared_memory}`; first event `Welcome{version, transport,
  rights, max_buffer_bytes}` followed by one `Output` per display. Anything before `Hello`, or a
  second `Hello`, ends the client.
* The `app_id` is a **hint** (dock grouping, logs). Identity and rights come from
  `SO_PEERCRED` (uid) and the rights table (§8), never from the client.
* Object ids (surfaces, buffers) are chosen by the client and live in the **client's own
  namespace**: client B saying "surface 1" means B's surface 1. There is no way to name an object of
  another client (test `hostile_clients_cannot_touch_others`).
* Errors: any violation sends `Error{code}` (Malformed, UnknownObject, Limit, Permission, BadBuffer,
  TooSlow) and closes that connection – never anything else.

### Requests (client → compositor)
`Hello · CreateSurface{id, role} · DestroySurface · SetTitle · SetSizeLimits ·
CreateShmBuffer{id,w,h,stride,format}+fd · CreateInlineBuffer · WritePixels{buffer,rect,bytes} ·
DestroyBuffer · Attach · Damage{rects} · Commit{configure serial} · Frame · SetState · Move{serial} ·
Resize{serial,edges} · Pong · SetSelection{serial,mimes} · ReadSelection{serial,mime} ·
SendData{transfer,chunk,last} · StartDrag{serial,mimes} · ReadDrop{mime} ·
TextInput{surface,enabled,purpose} · ImeCommit{text} · Capture{output}`

### Events (compositor → client)
`Welcome · Output · OutputRemoved · Configure{serial,w,h,scale,state,activated,rtl} ·
BufferReleased · FrameDone · PointerEnter/Leave/Motion/Button/Axis · KeyboardFocus · Key{code,
state,modifiers,character,serial} · TouchDown/Move/Up/Cancel · Text · CloseRequested ·
SelectionOffer · SendRequest · Data · TransferCancelled · DropOffer · Ping · ImeState · CaptureData ·
Error`

Positions are logical points in fixed point (1/256), relative to the receiving surface's content.

## 3. Buffers and the kernel

Two transports, chosen by the compositor in `Welcome`:

| Transport | How | Used where |
|---|---|---|
| **SharedMemory** | client creates a `memfd`, sizes it, adds seals `SHRINK|GROW|SEAL`, maps it read-write, sends the descriptor with `CreateShmBuffer` (`SCM_RIGHTS`). The compositor checks `F_GET_SEALS` ⊇ `SHRINK` (else `BadBuffer`), checks size ≥ `stride·(h−1)+4w` and ≤ 256 MiB, maps it **read-only** | Linux (development) |
| **Inline** | `CreateInlineBuffer` allocates the buffer in the compositor (charged to the client); `WritePixels` sends only the damaged rows, ≤ 1 MiB per message | Ultar kernel today; `--inline` |

On `Commit` the compositor **copies the damaged rectangles** out of the buffer into its own surface
image and sends `BufferReleased` at once. Consequences, deliberately chosen:
the client may draw into the buffer again immediately; a client cannot change the screen behind the
compositor's back or tear a frame being composed; composition never touches shared memory (no SIGBUS
path, no stall); a client killed mid-frame leaves its last complete image. Cost: one copy of the
damage (a caret: bytes; a full 1080p frame: 8 MB ≈ 1–2 ms memcpy, outside the composition budget).

Mapping and copying live in `ultar-hal-gfx::shm` – the only `unsafe` of the whole stack apart from
three `ioctl`s, each block with a `SAFETY:` comment. Platform crates are `#![forbid(unsafe_code)]`.

### Kernel gaps (Ultar kernel, state 23.11.2026, read from `kernel/ultar-kern/src`)

| # | Gap | Where | Effect today | Needed |
|---|---|---|---|---|
| K1 | `memfd_create` (319) missing → `ENOSYS` (kernel logs "noch nicht umgesetzt: 319") | `syscall.rs` dispatch | `shm::probe()` = false → inline transport | anonymous shared memory object (RAM-layer file without path), charged to the creator's memory group |
| K2 | `F_ADD_SEALS`/`F_GET_SEALS` (fcntl 1033/1034) missing | `sys_fcntl` | as K1 | seals on that object (at least `SHRINK`, `GROW`, `WRITE`, `SEAL`) |
| K3 | `mmap` of a file **copies** the bytes at map time (`sys_mmap` fills fresh pages); `MAP_SHARED` is not shared | `sys_mmap` | no zero-copy between processes | shared page mapping of one object into two address spaces (+ `PROT_READ` only for the reader) |
| K4 | `SCM_RIGHTS` not carried: `sendmsg` drops ancillary data, `recvmsg` returns `controllen = 0` | `sys_sendmsg`/`sys_recvmsg` | descriptors cannot be passed | descriptor passing on Unix stream sockets (bounded count, close-on-exec flag) |
| K5 | `MSG_DONTWAIT` ignored on Unix `recvmsg` (blocks unless `O_NONBLOCK`) | `sys_recvmsg` | harmless: the compositor sets `O_NONBLOCK` and uses `read` with inline transport | honour the flag |
| K6 | `fb0`: no vertical blank/page flip (`FBIO_WAITFORVSYNC`), no `mmap`; `pwrite` copies straight into the visible buffer | `eingabe.rs` | frames paced by a timer at 60 Hz, possible tearing | page flip or vblank event; ideally the plan's "direct buffer swap compositor↔kernel" (§3.6) |
| K7 | one display only (GOP), no modes, no hot-plug | `grafik.rs`, `console.rs` | multi-display, mirror/extend tested on headless/KMS only | display driver interface (virtio-gpu, Intel/AMD) – driver workshop |
| K8 | input: one absolute mouse (`event0`, PS/2) + one keyboard (`event1`); no `EVIOCGBIT`; no touch device; USB keys arrive as press+release pairs (no hold/repeat) | `eingabe.rs` | classification falls back to "absolute axis = pointer"; touch only tested with synthetic events | evdev capability ioctls, xHCI HID reports with real key-up, touch devices |
| K9 | no per-app uids yet (every process is uid 0 unless a launcher sets ids) | launcher | rights table keyed by uid grants per uid only; on the Ultar kernel everything runs as 0 today | app launcher assigns a uid per app cell (§3.7) |
| K10 | a `vfork` parent (`CLONE_VM|CLONE_VFORK`, used by glibc `posix_spawn` and thus by Rust's `Command::spawn`) is released only when the child **exits**, not at `execve` (`Wait::Vfork` = child not alive) | `proc.rs`, `sys_fork` | any launcher that starts a long-running child blocks forever (found in QEMU: the test launcher hung until the compositor exited); `kerncheck` takes the fork path (`CommandExt::uid`) as a workaround | wake the vfork parent at a successful `execve` of the child (and at its exit) – affects `ultar-init` and the app launcher too |
| K11 | a process waiting in `poll` for `POLLOUT` on a Unix socket is not woken when the peer drains the buffer (observed: output queued by the compositor stayed unsent until another event woke it) | `sys_poll` wake-up, `UnixConn` | the compositor retries queued output after 10 ms (`FLUSH_RETRY_MS`); the in-kernel capture over the protocol still did not complete in QEMU (see integration notes) | wake pollers when a Unix/pipe buffer gains room |
| K12 | `poll`/`ppoll` timeouts use whole seconds only (nanoseconds ignored); a 16 ms timeout becomes 0 | `syscall.rs` 7/271 | frame pacing busy-waits until the frame is due (bounded by one refresh interval, only while damage is pending) | honour nanoseconds (millisecond resolution suffices) |
| K13 | `mremap` (25) missing → glibc falls back to copying on `realloc` growth | dispatch | slower growth of large buffers in apps | implement `mremap` (MAYMOVE) |

With K1–K4 the protocol switches to shared memory without any change in apps (the transport is
negotiated). What already works on the Ultar kernel (QEMU, `tools/kern-test/compositor-qemu.py`):
`fb0` output, `event0`/`event1` input, Unix sockets with paths and `SO_PEERCRED`, the service
runtime (threads, signals, rights drop keeping `RAW_DEVICE`), inline transport, two `ultar-ui` app
processes composed with Arabic decorations – see the screenshot in the integration notes. Until then the inline path costs one extra copy through the socket (256 KiB socket
buffer → a full 8 MB frame is 32 round trips; damage-only frames are small).

## 4. Surfaces, roles, windows

Roles (`Role`): `Toplevel` (window with compositor decorations), `Popup{parent toplevel, x, y}`
(menus; parent must be one of the client's own toplevels – no cycles), `Panel{edge, size}` (bar/dock,
reserves its strip of the primary output's work area; edges are logical: `Start` = right in Arabic),
`InputPanel` (on-screen keyboard, bottom), `Lock`, `Background`, `Overlay` (notifications, at the
top end corner). Stacking: background < windows < panels < popups < overlays < input panel < lock.
While a lock surface has content, **only** lock, background and input panel are drawn and get input.

Configure/commit: the compositor sends `Configure{serial, size, scale, state, activated, rtl}`; the
client draws that size and commits with the serial. Geometry changes (maximise, snap, resize) take
effect **when the client commits the acknowledged serial** (atomic, like Wayland); the visual style
(shadow, corners) changes at once. Size limits are clamped to the work area.

Window states: `Normal`, `Maximized` (work area), `Fullscreen` (output, no decorations),
`Minimized` (hidden; Alt+Tab or dock restores). Snap layouts (Windows 11 / DESKTOP-SPEC §5.2):
drag to a side → half, to a corner → quarter, to the top → maximise, with an accent preview
(`state.pressed_overlay` opacity); Super+arrows walk half → quarter → half → restore → minimise;
dragging a maximised/snapped window restores it under the pointer. Double click on the title
toggles maximise. Physical sides are used for snapping (dragging to the left screen edge snaps left
in every language).

Decorations are drawn by the compositor with `ultar-ui` (Painter, tokens, icons): title bar height
`size.titlebar_height` (40), title centred (Noto Kufi/Sans 14, ellipsis, bidi controls removed from
titles), buttons **at the end** – minimise, maximise/restore, close outermost – **left in Arabic**,
hover `hover`, close hover `danger`; radius `radius.card` (12) at the top of the title bar and the
bottom of the content (anti-aliased coverage masks), shadow from `[shadow]` (Gaussian box shadow,
separable erf profile), no shadow/corners when maximised or snapped. Unfocused title: `surface` +
`text_muted`; an app that misses pings for 5 s gets a muted title (marked "not responding").

## 5. Damage and composition

* Damage is tracked per output in device pixels (`Region`, ≤ 32 rectangles, merged, else the
  bounding box). Sources: commits (surface pixels → logical → each output), window move/resize/state
  (painted area incl. shadow), focus/hover (title bar only), cursor (old + new rectangle), layout
  changes (all).
* Each damaged rectangle is split into tiles of 512×64 pixels. Per tile: find the topmost surface
  that covers it opaquely; paint from there up
  (nothing hidden is drawn); background fill only if nothing covers. Opaque parts are row
  `copy_from_slice`; translucent parts use a branch-free premultiplied `over` on `u32` rows that the
  compiler vectorises; corners and shadows touch only their pixels (shadow profiles are cached per
  window frame; only the band outside the frame is darkened). Surfaces drawn for another scale
  (window spanning outputs, mirror) use nearest-neighbour scaling.
* **Differential test**: after every random operation the incremental frame must equal a from-scratch
  composition, on one and on two outputs (`tests/damage.rs`). It found one real bug during
  development (state change restyling without damage).

## 6. Frame pacing

A frame is composed when an output has damage (or a visible surface waits for `FrameDone`) **and**
one refresh interval has passed since the last present of that output. After presenting, every
visible surface on that output that asked with `Frame` gets `FrameDone`; clients draw only after it
(`Window::can_draw`), so a hidden window draws nothing. KMS dumb buffers and `fb0` have no vblank
event here, so the interval is a timer at the mode's refresh rate (K6). **Zero work when idle**: no
damage, no waiting callbacks, no ping or transfer deadline → `poll` without timeout (test
`idle_means_no_work`).

## 7. Input routing

* Pointer: hit test top → bottom over the scene (decorations first: title, buttons, an 8-point resize
  band outside the frame). Enter/Leave/Motion go only to the surface under the pointer, in its
  coordinates. A press starts an **implicit grab**: until all buttons are up, events go to that
  surface even outside it. Click focuses and raises. Super+drag moves from anywhere.
* Keyboard: modifiers tracked by the compositor; compositor shortcuts first (Super+arrows, Super+Space
  layout switch, Alt+F4, Alt+Tab), else the key goes **only to the focused surface** (lock screen
  while locked) with the character of the active layout: **Arabic (PC 101) first**, English second,
  digits 0–9 in both, lam-alef keys as `Text`. Ctrl/Alt/Super chords report the Latin letter
  (Ctrl+C stays Ctrl+C in Arabic).
* Touch: each contact belongs to the surface it went down on; decorations by touch behave like the
  primary button.
* IME hook (`platform/ultar-ime`): a focused text field sends `TextInput{enabled, purpose}`; the
  client with the `InputPanel` role (needs `ui.input-method`) gets `ImeState{active, purpose}` and
  sends `ImeCommit{text}`, which arrives as `Text` at the focused surface only.
* Serials: every press/key/touch-down delivered gets a serial; the client keeps the last 8 with
  time. Requests acting for the user (`Move`, `Resize`, clipboard, drag) must quote one ≤ 10 s old
  (and `Move`/`Resize` only while that press is still held on that surface).
* Ping: the first input to a client sends `Ping`; the client library answers in its event loop
  (an app that reads events is alive). No `Pong` within 5 s → title muted, `unresponsive` set.

## 8. Security model

Default deny. Rights come from `/etc/ultar/compositor-rights.toml`-style lines `uid = "right,…"`
written by the app installer from the **app manifest** (`ui.shell`, `ui.input-method`,
`ui.lock-screen`, `ui.capture`, `ui.overlay`); an unknown uid has none. Peer identity is
`SO_PEERCRED`; if the kernel cannot tell, the client gets uid `u32::MAX` (no rights – fail closed).

| Right | Allows |
|---|---|
| none (every app) | toplevels and popups of its own; its own input; clipboard/drag on user action |
| `ui.shell` | `Panel`, `Background` |
| `ui.input-method` | `InputPanel`, `ImeCommit` |
| `ui.lock-screen` | `Lock` |
| `ui.capture` | `Capture` (screenshots, screen sharing) – reads the composed output |
| `ui.overlay` | `Overlay` |

Guarantees and how they are tested:
* **No client reads another client's pixels**: there is no request that returns pixels except
  `Capture` (right needed; test `capture_*`); object ids are per client.
* **No client sees another client's input**: routing goes to one surface key; tests
  `input_routing.rs` check the other client receives nothing.
* **Clipboard/drag only on user action**: serials + focus (§7); a drop can be read only by its
  target, once, within 10 s; data flows through the compositor in ≤ 1 MiB chunks, ≤ 16 MiB per
  transfer, one transfer per reader.
* **Bounded resources per client**: 64 surfaces, 64 buffers, 256 MiB pixel memory (buffers +
  committed copies), 8 MiB unsent events (a client that does not read is dropped – test
  `a_client_that_never_reads_is_dropped`), 2 MiB read per loop turn (fairness under flood – test
  `hostile_clients_cannot_touch_others`), ≤ 4 pending descriptors (more = Malformed), 64 clients.
* Fuzzing (`tests/protocol_fuzz.rs`, fixed seeds): random byte streams, random requests with
  hostile field values, huge sizes, unsealed / too small / foreign descriptors, descriptor floods,
  lying strides – the compositor never panics, limits hold, the good client keeps working.

## 9. Performance budgets (asserted in `tests/bench.rs`, release build)

| Measure | Budget | Measured (shared 2-core build machine, see integration notes) |
|---|---|---|
| Full composition 1920×1080, 4 windows (1 maximised, 3 floating with shadows), software | ≤ 4 ms p99 (thread CPU time) | 2.1–3.0 ms CPU p99; wall time on the shared build machine 9–22 ms (other agents take the cores) |
| Caret-sized damage | ≤ 0.3 ms p99 | 3–17 µs |
| Idle | no frame composed, no timer | asserted |
| Input → photon | ≤ 1 frame interval + app draw time (PC budget 30 ms, §3.6) | not measured on hardware (open) |

## 10. Crash isolation and restart

* A client dying (kill -9, crash mid-frame with half a message written) is a closed socket: its
  surfaces, buffers, grabs, touches, transfers and clipboard ownership are removed, damage repaints
  what was below, focus goes to the topmost window (tests `crashes_never_take_the_compositor_down`).
  Nothing a client sends can panic the compositor (no `unwrap`, typed errors, fuzzed).
* Compositor restart: clients see `Disconnected`, reconnect (`Client::connect_retry`) and create
  their windows again from their own state (test `clients_reconnect_after_a_compositor_restart`
  restarts the real binary). The compositor keeps no state that would need recovering; window
  positions are re-placed.

## 11. Outputs

`Layout`: outputs with id, connector name, mode, refresh, scale (100–300 % in 25 % steps, suggested
from DPI), logical position, primary. Modes **extend** (side by side, primary first), **mirror**
(every output shows the primary's logical area at its own fitting scale) and **single**. Hot-plug:
`Compositor::rescan_outputs` (SIGHUP; KMS probes connectors) adds/removes outputs, sends
`Output`/`OutputRemoved`, moves windows of a removed output to the primary, re-sizes maximised and
snapped windows and layers. Windows are configured with the scale of the output holding most of
them, so they redraw sharp after moving ("kein Hochrechnen").

## 12. Shell, ultar-ui and apps

* `ultar-surface::ui::UiWindow` runs an `ultar-ui` tree in a window: configure → `Ui::set_size/
  set_scale/set_direction`; events → toolkit events (named keys, characters, pointer, scroll,
  touch); `Ui::paint` → canvas damage → only damaged rows go to the buffer; frame pacing via
  `FrameDone`; `Ui::next_wake` respected. Test `ultar_ui_app_in_its_own_process` runs an ultar-ui
  app as a separate process, snapshots it and clicks its button.
* The shell (bar, launcher, quick settings, dock, lock screen, keyboard) is **just clients with
  rights** (`Panel`, `Overlay`, `Lock`, `InputPanel`); work area reservation and stacking are the
  compositor's part.

## 13. Not done (open, for P5/P6)

* Kernel gaps K1–K9 above (shared memory, fd passing, vblank, more displays/input on the Ultar
  kernel).
* GPU renderer (HAL `Renderer` trait), hardware cursor/planes, direct scan-out of a fullscreen
  client buffer, page flipping on KMS (dumb buffer is written in place).
* Overview, workspaces, Alt+Tab switcher UI, dock integration, "not responding" dialog,
  window menu, snap suggestion for the other half, resize of two snapped windows together.
* Keyboard repeat in the compositor (repeats only come from devices that send them), more layouts,
  dead keys; pointer acceleration; touchpad gestures; tablet tools.
* Screenshots with region/window selection UI (only output capture), screen recording.
* Settings integration over `ultar-bus` (display arrangement, scale per output from Settings,
  "Keep?" timeout) – the layout API exists, the bus contract does not.
* Accessibility (focus announcements, magnifier) and XWayland/foreign Wayland clients (plan §3.2:
  would need a Wayland front end; this protocol is the native one).
