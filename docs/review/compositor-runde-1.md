# Ultar compositor – independent security and robustness review (w4/comprev)

Subject: `platform/ultar-compositor`, `platform/ultar-surface`, `system/hal/ultar-hal-gfx` at
`0f4846e`. Roles: Prüfer Sicherheit + Prüfer Bedienung. The reviewer did not write this code.

**Verdict: not ready for production. 3 critical, 8 major, 6 minor findings, each with a
failing regression test.** The protocol layer is solid: parsing, descriptor handling, the
copy-on-commit design, crash cleanup, per-client object limits and idle behaviour held up
against every attack tried. The weak parts are policy and trust: the lock screen fails
open, the on-screen keyboard bypasses the lock, any app can cover the shell and take its
clicks, any app can take the keyboard focus, and the resource limits are per connection
only.

Tests: `CHANGES.patch` (applies with `patch -p1` from the repo root; adds 6 test files and
changes no existing file; copies in `tests/`). Results on `0f4846e`, debug, `-j1`: the 93
existing tests pass. Of the 24 review tests, **17 FINDING tests fail as intended** and **7
SOUND tests pass**; one more test runs only in release builds (measurement). `rustfmt` and
`clippy --tests -D warnings` are clean, and no function is longer than 60 lines.

## Critical

### C1 – The lock screen fails open when its process dies
`state/layers.rs:52` derives `locked()` from a live `Lock` surface that has content.
`state/mod.rs:279` `remove_client` destroys that surface. When the lock-screen process
crashes, is OOM-killed or is killed by anyone with its uid, the desktop unlocks: windows are
shown again and keys go to the focused app.
*Repro:* `review_security::lock_screen_stays_locked_when_its_process_dies` gives
`locked=false app visible=true keys to app=2`.
*Fix:* make the lock a compositor state (`State::locked: bool`). The first `Lock` commit sets
it. Only an explicit unlock from a `ui.lock-screen` client clears it, never a disconnect.
While locked and no lock surface exists, draw an opaque ground colour, route no input and
let a newly started lock client attach (ext-session-lock semantics). Tell `ultar-init` to
restart the lock client.

### C2 – The on-screen keyboard types into the app behind the lock screen
`state/requests.rs:462` `ime_commit` sends `Text` to `self.focus`. `update_ime` (`:439`) also
reads the text field of `self.focus`. While locked, `key_target` (`keys.rs:48`) sends keys to
the lock surface, but the IME path does not. A PIN typed on the OSK at the lock screen lands
in the app behind it, and the lock never receives it (touch devices cannot unlock).
*Repro:* `on_screen_keyboard_types_into_the_lock_screen_while_locked` gives `app=true
lock=false`.
*Fix:* add one `text_target()` that returns the lock surface while locked, else the focus.
Use it in `ime_commit`, `update_ime` and `key`.

### C3 – Any app can cover the shell and take its clicks (no trusted path)
`state/layers.rs:87-101`: a `Popup` (no right needed, no serial or grab needed, creation at
`requests.rs:273`) may be any size up to 8192² and is clamped only to the whole layout.
`:112` ranks popups (3) above panels (2). `input.rs:158` hit-tests by rectangle, not alpha.
An ordinary app can therefore paint a fake bar, a fake lock screen or a fake password dialog
over everything while it keeps the keyboard focus. Made fully transparent, the popup
invisibly takes every click meant for other apps and for the shell.
*Repro:* `an_app_popup_cannot_cover_the_shell_panel` (the panel pixel shows the popup
colour) and `an_invisible_popup_cannot_take_other_apps_clicks` (`victim=0 popup owner=2`).
*Fix:*
- Stack popups with their parent's window: just above it, below panels.
- Clamp popups to the parent's output work area, with a size cap.
- Allow at most 8 popups per toplevel, mapped only while the parent is focused.
- Optionally require a recent serial for a grabbing popup.
- Hit-test popups and windows only where alpha > 0, or refuse fully transparent input
  regions.
- Longer term: a secure attention key (for example Ctrl+Alt+Del) that only the compositor
  handles and that always shows the real lock or auth UI.

## Major

### M1 – Focus stealing without user action
`state/windows.rs:244-246` focuses on every first map. A client can unmap
(`Attach 0`+`Commit`) and map again at any time. `windows.rs:358-359` focuses on every
restore from minimised, and `requests.rs:188` accepts `SetState` from the client
unconditionally. A background app can take the focus while the user types a password
elsewhere. It then receives the keys and gets a user serial, which it can use to read the
clipboard (`transfer.rs:159`).
*Repro:* `a_background_window_cannot_steal_focus_by_remapping` (2 key events to B) and
`…_by_restoring_itself`.
*Fix:* focus on map only for the first map, and only if the client had a user serial
≤ 10 s old or an activation token from the shell/launcher. Otherwise map below the focus and
mark it "demands attention". A client `SetState(Normal)` from `Minimized` must not focus.

### M2 – One uid can take every client slot
`state/mod.rs:215`: the 64-client limit is global. One app that opens 64 connections locks
out the shell, the lock screen and every other app.
*Repro:* `one_uid_cannot_take_every_client_slot`.
*Fix:* add a per-uid limit (for example 8). Reserve slots for uids with rights (shell, lock,
IME), or keep a separate small budget per right.

### M3 – No global memory bound; sparse buffers are charged to the compositor
The pixel limit is per connection (`config.rs:40`, `requests.rs:311`). 8 connections
committed 2 GiB (`total_client_pixel_memory_is_bounded`); 64 connections would commit
16 GiB plus 64 × 8 MiB of event queues. The compositor is OOM-killed, and the whole desktop,
lock screen included, goes with it.
**M3b:** `shm.rs:126` accepts sealed but *sparse* memfds. The compositor's copy faults the
pages in, so they are allocated and charged on the compositor's side (first touch). Reading
a 64 MiB sparse buffer added 64 MiB of `RssShmem` to the reader (`review_hal::
sparse_buffers_are_refused`). The design says buffers are charged to the client.
*Fix:*
- Add `Limits::total_bytes` (for example 1 GiB or a share of RAM) checked with the
  per-client limit, plus the per-uid limit from M2.
- In `SealedMapping::map`, require `st_blocks*512 ≥ min_len`. `SharedBuffer::create` then
  `fallocate`s, so the client pays for the pages.
- Optionally require `F_SEAL_WRITE`-free memfds only from known clients.

### M4 – Frame callbacks are not paced when there is no damage
`server.rs:336-338`: when an output has no damage, `render_now` answers `FrameDone` at once
and does not move `last_present`. A client that only sends `Frame` gets `FrameDone` as fast
as it can ask. The compositor spins, and the timing reveals when *other* clients damage the
screen (a keystroke-timing side channel).
*Repro:* `review_robustness::frame_callbacks_without_damage_are_paced` gives 50 `FrameDone` with a frozen
clock.
*Fix:* treat an empty-damage frame as a frame. Set `last_present` (or a separate
`last_callback`) and answer callbacks only on the refresh grid.

### M5 – No flow control on clipboard/drop transfers: pasting a big image kills the pasting app
`transfer.rs:217-251` forwards `SendData` straight into the reader's queue. The source is
read at 2 MiB per turn, the reader's queue is capped at 8 MiB, and a transfer may be
16 MiB. A 12 MiB paste into an app that is busy for a moment drops that app as too slow.
*Repro:* `a_large_paste_does_not_disconnect_the_reader` gives `reader connected=false`.
*Fix:* stop reading the source (leave it out of the read set) while the destination has more
than about 2 MiB queued. Or cap a transfer below `out_bytes`, with per-transfer credit.

### M6 – An unplugged input device makes the compositor spin at 100 % CPU
`evdev.rs:481` breaks on a read error but keeps the device. `backend/hal.rs:121` keeps
polling its descriptor, which reports `POLLERR|POLLHUP` forever. `server.rs:155` wakes on
every turn. Pulling a USB keyboard or mouse on Linux busy-loops the compositor. In addition,
devices are opened only at start (`hal.rs:56`), so plugging a keyboard later never works.
*Repro:* `review_hal::a_hung_up_input_device_is_dropped` (FIFO stand-in for ENODEV).
*Fix:* remove a device on `ENODEV`/HUP and on `POLLERR`, and re-run `poll_handles`. Rescan
`/dev/input` on SIGHUP or inotify. Emit `DeviceRemoved` so the modifiers are cleared.

### M7 – Composition cost is unbounded: translucent layers freeze the desktop
Any app may stack translucent screen-sized popups (or toplevels). Each one is blended in full
because `covers` only skips opaque surfaces. With 2 apps and 58 layers, a full 1080p frame
took **282 ms median and 404 ms max** of CPU in release (`review_bench.rs`, measured). With
more clients it takes seconds, and the single-threaded loop then also delays input and the
lock screen.
*Fix:* apply the popup caps from C3. Cap the translucent area per client (for example
2× the output). Keep a frame time budget that skips occluded translucent layers. Count
layers against `surfaces` per uid.

### M8 – Rights on the Ultar kernel (known, K9) – production blocker
Every process runs as uid 0, so a rights line for uid 0 grants that right to *every* app.
The rights table only becomes meaningful with per-app uids. Listed so that it is not shipped
by mistake: refuse to start with rights for uid 0, or fail closed.

## Minor

* **m1** `requests.rs:22` `clean_title` keeps U+061C ARABIC LETTER MARK (a bidi control) and
  invisible format characters (U+2060, U+FEFF, …). Test: `titles_lose_every_bidi_format_character`.
  Fix: drop every `Cf` character, at least 061C, 180E, 200B–200F, 202A–202E, 2060–2064,
  2066–206F and FEFF.
* **m2** `output.rs:239`: mirror mode overwrites `scale_milli` of the other outputs, and
  extend does not restore it. After mirror → extend, the scale is 533 (not a 25 % step and
  below 100 %). Test: `mirror_then_extend_restores_the_scale`. Fix: keep the user scale and
  the mirror scale apart.
* **m3** `evdev.rs:278` `SYN_DROPPED` cancels touches but not keys or buttons, so a
  Super/Alt/button can stay held forever. Test: `syn_dropped_releases_held_keys`. Fix:
  synthesise releases for held keys and buttons (or resync with `EVIOCGKEY`).
* **m4** `server.rs:470`: after unplugging the display under the pointer, the pointer stays
  outside every output (it was left at 2140,300). Test:
  `unplugging_the_display_under_window_and_pointer` (the window part of that test is sound).
  Fix: clamp the pointer in `outputs_changed`.
* **m5** `grabs.rs:66` / `input.rs:205`: the move offset is taken when the 4-point threshold
  is crossed, not at the press, so the first motion is lost. A (210,100) drag moved the window
  by (10,0). Test: `a_title_drag_keeps_the_grabbed_point_under_the_pointer`. Fix: compute the
  offset from `TitlePress.start`.
* **m6** (by reading, no test) Capture keeps streaming while locked
  (`server.rs:385`, `requests.rs:232`), and there is no visible "screen is being recorded"
  indicator. Key release and device repeat after a focus change go to the new focus
  (`keys.rs:27`). An implicit pointer grab survives locking (`input.rs:203`). The shadow cache
  is replaced per output (`render.rs:243`), so shadows are recomputed every frame with two
  outputs. `fbdev.rs:105` ignores `xoffset/yoffset` and assumes the 64-bit
  `fb_fix_screeninfo` layout. `RightsTable::parse` (`config.rs:146`) accepts rights for
  `u32::MAX` (the fail-closed uid) and silently overwrites duplicate lines.

## Verified sound (by test or measurement)

* **Wire parsing:** frames, bounded strings and blocks, damage count, MIME list, reserved
  header, trailing and truncated bodies all hold. The fuzzers pass, and every size and
  offset uses checked `u64` arithmetic (`requests.rs:309,338-343`, `surface.rs:61`,
  `shm.rs:171`).
* **SIGBUS:** the SHRINK seal is enforced. A hole punched after mapping reads as zeros, and
  shrinking is refused (`punched_holes_read_as_zero_without_sigbus`). Huge-page memfds map or
  fail at map time (`huge_page_memory_files_never_fault_later`, not exercisable here:
  `nr_hugepages=0`). Composition never reads shared memory because of copy-on-commit.
* **Descriptors:** no leak across 40 clients that sent buffers, unclaimed descriptors and
  floods (`no_descriptor_leaks`). There are at most 4 pending descriptors, and `CTRUNC` is
  treated as abuse.
* **Isolation:** object ids are per client. Capture needs its right
  (`capture_without_right_is_refused`). Drop is target-only, used once and time-limited.
  `Move`/`Resize`/`StartDrag` need a held implicit grab. Clipboard needs focus plus a serial
  (but see M1).
* **Slow clients:** a half-sent 1 MiB message stalls nobody (`a_half_sent_message_blocks_nobody`).
  A never-reading client is dropped (existing test). Captures respect the queue.
* **Damage:** incremental == full with translucent and hostile non-premultiplied panel,
  overlay and popups, a 150 % second output in Arabic, scales 100–275 %, mirror mode, and
  unplug with re-placement (`incremental_equals_full_*`, 3 tests).
* **Performance claims hold:**
  * 1080p, 4 windows, full damage: p99 CPU 3.29, 3.22 and 2.37 ms in three release runs, with
    wall time ≈ CPU time (load ≈ 1.9). The budget is 4 ms, but the margin is thin.
  * Caret damage: 4–13 µs.
  * Idle: the real binary, headless 1920×1080, had 0 CPU ticks and 0 context switches in
    10 s, both with no client and with an idle window.
* **Licence:** a line-level comparison against `platform/ultar-wm`,
  `/home/claude/ultar-os/crates` (incl. `ultar-wm`) and the `drm`/`rustix` sources found only
  boilerplate, plus one KMS line from the own old `ultar-shell`. There is no sign of Smithay or
  `ultar-wm` (MIT) code, so `Proprietary` is fine. Algorithm-level similarity was not checked.

## Not covered

No DRM/KMS hardware, no fbdev on Linux, no aarch64 and no Ultar-kernel run. The DRM and
fbdev backends were reviewed at unit level only (reading the code). There was no fuzzing of
the client library against a hostile compositor, and input-to-photon latency was not
measured.
