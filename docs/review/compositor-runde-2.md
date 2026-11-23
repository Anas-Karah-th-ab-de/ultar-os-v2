# Ultar compositor – re-review after the fixes (w4/comprev2, against b8849e3)

**Verdict: not ready for production yet.** All 17 round-1 tests now pass. C1, C2 and M4 and
the minor findings m1–m6 are fixed at the root. The other fixes only cover the case my test
exercised. This round found **12 new findings (2 critical, 7 major, 3 minor)**, each with a
failing test. `CHANGES.patch` adds three test files (copies in `tests/`). On HEAD, debug,
`-j1`: 130 tests pass (the 129 existing ones plus my one new SOUND test) and the 11 new
FINDING tests fail. N12 fails in a release build only.

## (1) Changes to my round-1 test files

Five of the six files are byte-identical. `review_bench.rs` was rewritten from a measurement
into an asserted budget. The new scene is harder than mine: 3 uids × 2 connections, popups up
to the cap plus 6 translucent toplevels each, `culled > 0` is asserted, and so is the refusal
of the popup above the cap. **This is legitimate, not weakened.** I measured it again: p99
9.7 ms, against a 16 ms budget.

The other changed tests are also legitimate, because new windows now open inactive:
- `damage.rs` and `processes.rs` add a click before the snapshot.
- `input_routing.rs` splits `two_windows` and asserts the new "inactive" rule.
- `evdev_tests.rs` asserts the release and the `DeviceRemoved` on EOF.
- `proto/tests.rs` moves the "unknown role" byte from 7 to 8, because role 7 now exists.

## (2) Is each round-1 finding fixed at the root?

| Finding | Status |
|---|---|
| C1 lock | Fixed (`Session::locked`). New issue: N1 |
| C2 IME | Fixed (`text_target`) |
| C3 trusted path | Partly: N3 (alpha 1), N4 (overlay over the dialog) |
| M1 focus | Partly: N2 (tokens) |
| M2 slots | Partly: N5 (reserve), N7 (shared uid today) |
| M3 memory | Partly: N6 (first come, first served), N8 (TOCTOU) |
| M4 pacing | Fixed (`idle_frame`) |
| M5 flow control | Partly: N9 (freeze), N10 (spin) |
| M6 hot-plug | Fixed for the dead device; device churn by reading (N13) |
| M7 frame cost | Translucent part fixed. N11 (culling hides the top window), N12 (opaque bypass) |
| M8 | Rights for root/`u32::MAX` refused. Consequence: N1 |
| m1–m6 | Fixed |

## Critical

**N1 – Super+L or Ctrl+Alt+Del bricks the shipped session.**
- The shipped rights file is empty, the shell runs as root, and root is refused. So no
  client can ever hold `ui.lock-screen`.
- `secure_attention` (`state/session.rs:220`) still locks.
- The result is a dark screen with no input anywhere and no way to unlock it.
- Test: `review2_trust::a_secure_attention_key_without_a_lock_screen_does_not_brick_the_session`.
- Fix: lock only if a lock provider is connected (or the rights table names one), otherwise
  ignore the key or forward it. Better: `ultar-init` starts the lock screen, and the lock
  waits a bounded time for it.

**N2 – One click buys unlimited focus steals.**
- `RequestActivation` does not consume the serial (`session.rs:187`): one click gave 16
  tokens.
- `Activate` on an unmapped window parks the token as `activate_on_map`, which never
  expires (`session.rs:213`, `windows.rs:282`).
- An app can therefore take the focus an hour later, or 16 times.
- Test: `one_click_buys_one_activation`.
- Fix:
  - One token per serial, and consume the serial.
  - Let `activate_on_map` expire with the token's own expiry, and clear it when the focus
    moves by user action.
  - Bind a token to the client that requested it, or to the uid it was handed to.

## Major

**N3 – The C3 fix only skips alpha 0.** A popup of alpha 1/255 (invisible) over the work area
still takes clicks on the windows below its top parent (`input.rs` `solid_at`). Test:
`a_nearly_invisible_popup_cannot_take_other_apps_clicks`. Fix:
- Clamp popups to their parent's content area (plus a menu margin), not the work area.
- Or use an alpha threshold (for example < 32 is not hit), or an input region checked
  against what the user actually sees.

**N4 – An overlay above the system dialog.** `Overlay` (rank 4) is stacked above
`SystemDialog` (3), is not dimmed and takes the dialog's clicks. Overlay size is set by its
client without a clamp (`layers.rs:90`). A notification service (`ui.overlay`, a lower
right) can cover the trusted dialog. Test: `an_overlay_cannot_cover_a_system_dialog`. Fix:
- Put the system dialog above overlays (only the OSK and the lock stay above it).
- Clamp the overlay size.

**N5 – The reserved slots can be exhausted.** Any uid with *any* right has no per-uid limit
and may use the reserve (`state/mod.rs` `add_client`). A `ui.capture`-only uid took 64/64
slots. Test: `a_low_right_cannot_take_the_lock_screens_slot`. Fix: keep the per-uid limit for
every uid, and reserve slots only for `lock-screen`, `shell` and `input-method`.

**N6 – One app takes the whole 1 GiB.** Four connections of one uid hold 1024 MiB. The lock
screen's first 1080p buffer is then refused with `Limit`, which ends its connection: the
locked session can never be unlocked. Test: `one_app_cannot_take_the_lock_screens_memory`.
Fix:
- Add a per-uid byte limit (for example 256 MiB).
- Reserve memory for rights holders (for example 128 MiB that apps cannot use).

**N8 – TOCTOU in the sparse-buffer refusal.** `st_blocks` is checked once at create time. A
hole punched afterwards is allowed, and the compositor's next copy refilled it (8 → 2096
blocks). Test: `a_buffer_cannot_be_hollowed_after_the_check`. Fix: the client maps its
buffer RW and then adds `F_SEAL_FUTURE_WRITE`, which blocks punch-hole and `MADV_REMOVE`
while the existing RW mapping keeps working. The compositor requires that seal. (Impact is
bounded by the global byte count: the cost is misattributed memory, not an unbounded amount.)

**N9 – A slow reader freezes the clipboard owner.** Flow control stops reading the whole
source (`server.rs` `throttled`). A focused app that pastes and stops reading keeps the
owner's commits, pongs and titles unhandled for up to 10 s per paste, and one keystroke buys
the next paste. Test: `a_slow_reader_does_not_freeze_the_clipboard_owner`. Fix: use credits
per transfer instead. Send `SendRequest` per chunk (pull) and keep reading the source's other
requests. Or end the transfer early once the reader is over its queue limit.

**N10 – A dead throttled source makes the loop spin.** A throttled source is left out of the
read set, but `poll` still reports its hang-up, and `read_client` returns at once. Ten waits
of 50 ms took 52 µs in total, and the source is not removed until the transfer expires.
Test: `a_dead_throttled_source_is_removed_without_spinning`. Fix: on `POLLHUP/POLLERR` drop
the client whether or not it is throttled.

**N11 – The overdraw budget hides the window on top.**
- The budget is spent bottom to top (`layers.rs:198` `cull`), so hostile translucent windows
  at the bottom use it up.
- The user's translucent app on top is culled: it is invisible and is not hit-tested.
- The click aimed at it lands in the hostile window below, while keys still go to the
  invisible focused window.
- Test: `review2_overdraw::the_overdraw_budget_cannot_hide_the_window_on_top`.
- Fix: spend the budget from the top (focused/topmost first), or charge only the unoccluded
  area. Never cull the focused window, and never hit-test through a culled window.

**N12 – Opaque windows bypass the budget.**
- One uid with 8 × 63 thin (1800×2) opaque floating windows uses a few KiB of memory.
- Each window costs its shadow band, corner masks and title bar.
- A full 1080p frame then costs **72 ms CPU** (release, two runs); 7 uids would cost about
  0.5 s. The shell's frames miss too.
- Test: `thin_opaque_windows_stay_within_the_frame_budget` (release).
- Fix: count the painted area (frame + shadow) of every surface of a uid without rights
  against the budget, and cap visible toplevels per uid (for example 16).

## Minor

**N7 – Eight app processes per session today.** Until K9 every app shares one uid without
rights, so the per-uid limit caps the whole desktop at 8 app connections. Test:
`nine_apps_of_one_user_can_run`. Fix: make `clients_per_uid` configurable and raise it
(for example 32) until apps have their own uids.

**N13 – Hot-plug churn (found by reading, no test).** `Evdev::rescan` matches devices by path
only. Unplug and replug between two rescans reuses `eventN`: the stale device is kept, and the
new one is opened only at the next uevent. `input_changed` ignores `ENOBUFS` (lost uevents),
which should force a rescan. Only `event0..31` are scanned.

**N14 – Input injection paths (found by reading).** Every new `/dev/input/event*` is now
opened on hot-plug. Any process that may create uinput devices can inject keys, including the
SAK. This is outside the compositor and depends on `/dev/uinput` permissions on Linux;
document it and keep uinput root-only. Clients cannot synthesize the SAK through the
protocol: `ImeCommit` produces only `Text`, and there is no key injection request.

## Sound (verified)

- **SAK:** handled before shortcuts and key routing, never forwarded, release swallowed (the
  author's test, and reading). The IME and the protocol cannot produce it.
- **Activation tokens:** a 64-bit keyed hash (`RandomState`), single use, 30 s, at most 16.
  They are not forgeable; transferability is by design (launcher → app). The problems are
  N2.
- **System dialog:** `ui.system-dialog` is only in the rights file, and root and
  `u32::MAX` are refused. Its title bar and the lock mark are compositor-drawn. Panels are
  dimmed, which an app cannot do. It is modal below itself (except overlays, N4).
- **Recording indicator:** painted last, above everything. The first `CaptureData` comes from
  a presented frame that already shows it (`capture_data_never_precedes_the_indicator`).
  Capture pauses while locked.
- **Frame pacing:** an idle frame answers callbacks once per interval.
- **Uevents:** only the kernel can send to an unprivileged `KOBJECT_UEVENT` socket, and
  non-root senders get `EPERM`. At most 256 messages are read per turn.
- **Translucent hostile scene (author's bench):** 9.7 ms p99 at load 5.5.

Not covered: real input hot-plug hardware, QEMU and the Ultar kernel (uevents, `fallocate`
there), and a real lock-screen or dialog service (none exists yet).
