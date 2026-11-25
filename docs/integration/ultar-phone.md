# ultar-phone – integration notes (phone app, call contract, call screen)

Base f675fcc · 2026-11-24 · role "Bau Oberfläche". Boards PhoneDial, PhoneIncoming, PhoneInCall
(DESKTOP-SPEC §12.2), clicks `account.copy`, `win.*`, `phone.*` of docs/ui/CLICKS.md.

## 1. What exists

| Part | Where | Content |
|---|---|---|
| Bus contract `call.*` | `system/core/ultar-bus/src/messages/call.rs`, `call_requests.rs` | topic `call.state` (retained: both ways' availability, own number, ≤ 2 live calls, last ended call, audio), 16 calls `call.dial/answer/decline/hangup/hold/mute/output/dtmf/video/camera/share/text/log/forget/block/snapshot`, typed `CallError` as `ErrorCode::Service(code)`, limits (`MAX_CALLS` 2, number 32 B, text 160 B, log 200, blocked 256), `check_number`, `check_text`, `is_emergency` |
| Call service | `system/services/ultar-call/src/{line,session,session_cmd,calllog,service,fake,ultar_line}.rs` | `Line` (one per way) + `AudioRoute` + `Clock` traits; `Session` state machine (dialing → alerting → active ⇄ held, ringing / waiting, ended with `End::{Local, Remote, Declined, Missed, Busy, Blocked, Failed(Failure)}`), emergency numbers without SIM/service, hold-on-dial, swap, answer-and-hold / answer-and-end, third call refused (logged missed), blocked numbers declined unheard, mute reset after the last call; `Service` (bus loop: registers `call`, answers, publishes `call.state` on change, polls lines every tick); `fake::{FakeLine, FakeControl, FakeAudio, ManualClock}` test double; `UltarLine` adapts a `CallBackend` (`E2eCall` of §8, or `MockCall`) as the Ultar-number line. |
| Phone app | `apps/ultar-phone` (lib) | `model` (pure state), `view` (ultar-ui widgets + tokens only; wide/medium = boards, compact = one column with Call/History tabs), `app::PhoneApp` (events, clicks, rebuild with focus/hover kept, toasts, held backspace, timer), `port::{CallPort, BusPort, Host}`, `locales/{ar,en,de}.ftl` (121 keys, Arabic reference) |
| Call screen right | `platform/ultar-surface` (`Role::CallScreen` = 8, `Rights::CALL_SCREEN` = 64, `ALL` = 127), `platform/ultar-compositor` (`ui.call-screen` in `PERMISSION_NAMES`, rank 7 above the lock, full screen, shown while locked) | see §4 |
| E2E crate | `tools/phone-e2e` | phone app ↔ real broker ↔ call service + fake line (apps may not depend on services; tools may) |
| Conformance | `tools/design-conformance/phone_manifest.py`, `design/conformance/phone.tsv`, `apps/ultar-phone/tests/design_conformance.rs`, `run.sh` | 55 board elements: presence, size ±1, fill/border colour ±16 |

## 2. Wiring

1. **Bus policy** (the call service and the phone app):
   ```toml
   [[rule]]
   identity = "system.calld"
   register = ["call"]
   publish = ["call.state"]
   [[rule]]
   identity = "app.phone"
   call = ["call.*"]
   subscribe = ["call.state"]
   ```
   Only the phone (and later the lock screen's call banner) may call `call.*`: `call.log` and
   `call.state` carry numbers and names.
2. **Call service daemon:** `Service::new(Client::connect(socket, ClientOptions::named("calld")), Session::new(lines, audio, Box::new(SystemClock::default())), DEFAULT_TICK)?.run()`. Lines today: `UltarLine::new(ultar_call::default_backend(), own_number)`. There is **no binary yet** (see §6).
3. **App glue (platform):** apps may use only `ultar-ui`, the bus, i18n and brand (`tools/check-layers.toml`), so the window glue belongs in `platform/app-api` when it exists:
   `PhoneApp::new(Look{lang, theme, mode, size}, port.snapshot()?, BusPort::new(client, timeout), host)`;
   feed `app.handle(event)`, `app.set_state(state, now_ms)` for every `call.state` from
   `BusPort::states()`, `app.tick(now_ms, wall_s)` at least once a second and at `ui().next_wake()`,
   `app.set_utc_offset(s)`; paint `app.ui()`. `Host` = clipboard (`account.copy`), window
   operations (`win.*` – the app draws its own header like the board; the compositor must not
   decorate this window, see §6), open the phone settings, create a contact.
4. **Call screen:** while `PhoneApp::wants_call_screen()` the glue shows the app on a
   `Role::CallScreen` surface (right `ui.call-screen`; the installer writes the phone's uid line
   `<uid> = "ui.call-screen"` into the compositor rights file).

## 3. Behaviour (CLICKS.md → implementation)

`account.copy` → `Host::copy_text(own number)` + toast "Copied" (error toast if refused) ·
`win.min/max/close` → `Host::window` · `phone.way` → radio cards (mobile network unavailable
without modem, detail "SIM n" / "no SIM") · `phone.number` → LTR field, prefix `UOS-` fixed on the
Ultar way, Arabic-Indic digits converted, Ultar numbers grouped `XXXX-XXXX`, Enter calls ·
`phone.backspace` → one digit (with its dash); held `motion.long_press` → all ·
`phone.suggest` → newest named history entry containing the typed number · `phone.digit` →
appends; in a call with the keypad sheet open → `call.dtmf` · `phone.call` / `phone.call.video` →
`call.dial` (video only Ultar; disabled when the way cannot call, except emergency numbers on the
mobile network) · `phone.filter` → all / missed · `phone.settings` → `Host::open_settings` ·
`phone.callback` → dial back on the entry's way · `phone.entry.more` → menu Create contact
(`Host`), Block number (`call.block`), Delete entry (`call.forget`) · `phone.accept` →
`call.answer{others: Hold}` · `phone.decline` → `call.decline` · `phone.decline.msg` → decline,
then dialog with three ready messages → `call.text` · `phone.mute` → `call.mute` (label becomes
"Unmute") · `phone.output` → menu of the present outputs (current checked) → `call.output` ·
`phone.keypad` → sheet with tones · `phone.hold` → hold / resume · `phone.video` → own camera ·
`phone.camera` → next camera (≥ 2 cameras) · `phone.share` → confirmation dialog first, then
`call.share`; again = stop · `phone.hangup` → `call.hangup`.
States: no modem / no SIM / no network / airplane / emergency only (info bar, emergency numbers
still dial), signed out / server offline, dialling / ringing / on hold, ended / remote ended
(duration) / declined / missed / busy / failed with reason (info bar + "Call again"), muted,
speaker (output menu), hold, keypad + DTMF, second incoming call (answer & hold, answer & end,
decline), held call with "Swap", failed requests as danger toast with the reason.

## 4. Call screen over the lock screen (compositor)

`Role::CallScreen` needs `Rights::CALL_SCREEN` (`ui.call-screen`, default deny). It is laid out
full screen and stacked above the lock screen (rank 7); while locked it is the only surface
besides the lock screen and the on-screen keyboard. Pointer and touch reach it (it can be
answered/declined); keys, IME commits and `Unlock` never do – the text target stays the lock
screen and `Unlock` needs a lock surface of the same client. Tests:
`platform/ultar-compositor/tests/call_screen.rs` (2), `ultar-surface` proto tests.

## 5. Tests (x86_64, `-j2`, debug off, `CARGO_INCREMENTAL=0`; three identical runs)

`cargo test -p ultar-bus -p ultar-call -p ultar-phone -p phone-e2e -p ultar-surface -p ultar-compositor`:
**339 passed, 0 failed, 5 ignored** in each of 3 runs (bus 152 / call 21 / phone 41 / e2e 1 /
surface 12 / compositor 112). Phone: unit 10 (model, clicks, port, when, app), `tests/clicks.rs`
18 (every phone/call CLICKS entry by real pointer/keyboard input), `tests/states.rs` 11,
`tests/render.rs` 1 (3 surfaces × ar/en × dark/light × 1280×800 / 360×720: every text translated,
page ground = theme token, start side mirrored), `tests/design_conformance.rs` 1. Call service:
19 unit tests (session, log, fake, lines) + `tests/service.rs` 2 over a real broker (hostile payloads,
typed errors). E2E: incoming → answer → hold → end over the bus. `cargo fmt --check`, `cargo
clippy --all-targets -D warnings` (the six crates) clean; `ultar-check all` passes (5596
baselined as before, no new baseline entry); `ultar-i18n-check apps/ultar-phone/locales` complete;
`tools/design-conformance/run.sh` OK; `cargo check -p ultar-shell` OK.

Design conformance: **41 of 55 elements equal** (presence, size, colours); **0 missing**; 16
known differences in `known-differences.txt` (`phone/…`), all toolkit gaps:
way cards 65 high (RadioGroup CardRow padding 12, board 54), number field 312 wide (no end
adornment in `TextField`, the backspace sits beside it), video-call button 172 vs 170.5 (CSS flex
shares by border width), filter tabs (underlined `Tabs` vs filled pill buttons), eight in-call
controls (Button has only a leading icon – board: icon above label, 104 × 67 – and no pressed /
toggle state for "video on"). coverage.toml: 11 actions `ok`, 12 `different`, 0 `missing`.

## 6. Not done / open (round 1 – points 1–4 are closed in §7)

1. **No real modem line.** `ultar-ril` (A30 Shannon) has dial/answer/release/list but no
   hold, DTMF, mute or availability events, and talks through files in `/run`, not the bus; a
   `CellularLine` over it is not written. The mobile-network way works only with the fake line.
2. **No `ultar-call` daemon binary** and no service file; no persistence of log and blocked
   numbers (in memory, bounded); `call.text` is unsupported by both real lines.
3. **App window glue** needs `platform/app-api` (apps may not use `ultar-surface`), and the
   compositor needs a "client draws its own header" toplevel flag: the app draws the board's
   header (own number, title, window buttons) itself.
4. **Toolkit gaps** behind the 16 differences (§5) – `ultar-ui` area, not changed here.
5. Contacts (names, "create contact"), the system permission prompt for screen sharing
   (here an in-app confirmation), phone settings page, ringtone/vibration, a lock-screen banner
   when another app is full screen: other services/surfaces.
6. Arabic texts written by the agent, not reviewed by a native speaker. Not run on a device.

## 7. Round 2 (2026-11-24) – open points closed

| Point | Done |
|---|---|
| Toolkit gaps | `RadioLayout::Choices`, `TextField::end_action` + `fill_height`, `Tabs::pills`, `Button::tile` + `toggled` (ultar-ui wave 8, `tests/w8_phone_bricks.rs`, snapshots `tests/snapshots/phone-bricks/`). Phone: **55 of 55 board elements equal, 0 MISSING, 0 DIFFERENT**; no `phone/…` line in known-differences.txt; all 23 actions `ok` in coverage.toml. In-call controls are toggles (mute, output = loudspeaker, hold, video, share) with the board labels; `pressed` is announced. |
| Window | `tools/check-layers.toml`: apps → `platform/ultar-surface` ("compositor client API for app windows"). Protocol request `ClientHeader { surface, on }` (opcode 42 since the wave-8 integration), right `ui.client-header` (`Rights::CLIENT_HEADER` = 256, `ALL` = 511): no compositor title bar; refused without the right and for system dialogs; move, resize, snap, Alt+F4 and "not responding" stay with the compositor (tests `ultar-compositor/tests/call_screen.rs`). `ultar_phone::window::PhoneWindow`: toplevel with own header, input, damage-free full repaint, clipboard (`SetSelection` + `SendData`), min/max/close through `SetState`/close, call screen while ringing; `QueuedHost`. Test: `tools/phone-e2e/tests/window.rs` in a headless compositor. |
| Real line | ultar-ril: `tel::{build_manage_call, build_dtmf, build_mute}` (command ids `// VERIFY` on the A30 like the other NET/SS ids), `Engine::{hold_swap, dtmf, mute, release}`, `bus::ModemService` (`modem.*`, contract `ultar_bus::messages::modem`), daemon `--daemon --bus SOCKET` (ril.toml). ultar-call: `modem_line::{ModemLine, ModemAudio}`, `fake_modem::{FakeModem, FakeModemControl}` (test double of `modem.*`), binary `ultar-calld` (`--bus --log --modem --fake-modem --ultar --lang`), `rootfs/etc/ultar/services/call.toml` (user 829 `calld`, no kernel rights, `/data/ultar/calls`), `ids.toml`, `bus.toml` (sockets `calld.sock` group 829, `ril.sock` root), `bus-policy.toml` (`system.calld`, `system.ril`, `app.phone`). Persistent history: `logstore` (mode 0600, atomic temp+fsync+rename+dir fsync, ≤ 128 KiB, corrupt lines skipped and counted, fixed-seed fuzz), blocked numbers kept too, `call.clear` + menu item "Delete whole history". |
| QEMU | `tools/phone-e2e/examples/telefontest.rs` (broker, test-double modem, the real `ultar-calld --modem` process, the phone app) and `tools/kern-test/telefon-e2e.sh`, called from `ci.sh` (verdict `TELEFON: ALLES OK`). |

Tests round 2 (x86_64, `-j2`, debug off, three identical runs): `cargo test -p ultar-ui -p ultar-bus
-p ultar-call -p ultar-ril -p ultar-phone -p phone-e2e -p ultar-surface -p ultar-compositor`:
**739 passed, 0 failed, 18 ignored** each run. fmt clean; clippy `-D warnings` clean for all of
them except `ultar-ril`, whose legacy files have 5 lints on f675fcc (none in the new code);
`ultar-check all` passes (no new baseline entry); `ultar-init --test shipped_services` 7 passed;
`design-conformance/run.sh` OK.

Round 3: **`tools/kern-test/telefon-e2e.sh` on the Ultar kernel (QEMU, 2 CPUs, OVMF): `TELEFON:
ALLES OK` in 4 of 4 runs** (kernel 0.4.0 from this tree, static `ultar-calld` + `telefontest`);
`ultar-ril` is clippy `-D warnings` clean (the 5 + 3 old lints fixed; the data-SIM choice was
rewritten as one equivalent condition). "Phone settings" opens Settings through the app launcher:
`launcher.start` version 2 `{app: org.ultar.settings, role: Ui, activation_token, uri:
settings://apps/org.ultar.phone, new_window: false}` (`ultar_phone::launch`, with the launcher's
own `ultar_bus::messages::launcher::Start`/`StartReply` since the wave-8 integration); the token comes from `RequestActivation` with the click's serial; a refused
or missing launcher shows the toast `phone.error.launch`. Test: `tools/phone-e2e/tests/window.rs`
(fake launcher). Gate: 741 passed, 0 failed, 18 ignored, three identical runs.

Still open (honest):
1. Bus policy for the phone's `launcher.start` call is not added (the launcher delivery owns its
   rules; `app.phone` needs `call = ["launcher.start"]` there).
2. Samsung-IPC ids of SS_MANAGE_CALL (0x0C07), CONT_DTMF states and SND_MIC_MUTE (0x0901) are from
   libsamsung-ipc naming and must be checked on the A30; IPC has one release for all calls
   (`Engine::release` checks the index, then releases). Speaker routing of the modem path is not in
   the modem contract (output = earpiece on the mobile network).
3. "Create contact" (`HostOp::Contact`) is still handed to the caller: no Contacts app or scheme
   exists. No `ultar-phone` binary / app manifest yet.
4. `tests/modem.rs` polls the fake modem's state with short sleeps (bounded); the other tests wait
   on bus events.

## 8. Ultar-number calls end to end (2026-11-25)

The server only connects two numbers; it never learns keys, voice, pictures or who talked how long, and
keeps nothing (signalling: no log line, `ULTRAOS_LOG=warn`; TURN: random user tag, coturn without log file).

| Part | Where | What |
|---|---|---|
| Protocol | `ultar-call/src/e2e/{crypto,engine,pins}.rs` | hello (call id, ephemeral X25519, device Ed25519) → reply (own keys, signature over the transcript T = SHA-256 of both) → sealed confirm (caller signature, video flag, candidates); the callee rings only after both signatures check. HKDF-SHA256 keys per direction for signalling and media, ChaCha20-Poly1305 with counter nonces and a replay window. Keys pinned on first call (TOFU); a changed key shows a warning; both phones show the same 60-digit safety number (`call.state` `safety`, `key_changed`). Every server message is an opaque `{"to","type":"e2e","data"}`. |
| Network | `e2e/{stun,net,media}.rs` | STUN binding, TURN allocate/ChannelBind/refresh (RFC 8489/8656), direct and relayed candidates, probes pick the first working way; `relay_only` hides the own address (the other side then must not be relay-only too: coturn refuses relay-to-relay on its own address). |
| Voice / picture | `e2e/{audio,video}.rs` | 16 kHz mono 20 ms IMA ADPCM with jitter buffer; JPEG 320×240 in 1100-byte pieces, 12 fps. |
| Device key | `ultar-accountd` `link.rs` (`account.device_*`) | The device key never leaves accountd: calld asks for the signalling hello/answer, the public key, a signature over T (label `ultar-call-e2e-v1`, hash only) and TURN credentials. Policy: `system.calld` only. |
| Sound | kernel `gen_hda` + `svcdev/pcm.rs`, `ultar-call/src/pcm.rs` | `/dev/ultar/pcm` (path from `ultar-hal-system::seat_nodes::pcm_node`): 48 kHz 16 bit stereo running output and input rings, one opener, right `sound` (bit 12, `call.toml`). |
| Daemon | `ultar-calld --account [--pcm NODE]` | `BusDevice` + `E2eCall` as the Ultar line next to the modem line. |

**Tests:** `ultar-call` unit tests (crypto, replay, STUN, ADPCM, engine, media over loopback);
`server/cloud-e2e/tests/calls.rs` (two devices against the real server: video call, both hear the other's
tone, see ≥ 10 pictures, same safety number, server database unchanged); `tests/live_call.rs` (ignored; through
the release server's TURN relay); `tools/kern-test/pcm-e2e.sh` (in `ci.sh`: `PCM-E2E: ALLES OK`, the tone
reaches QEMU's speaker, input recorded at the card's pace).

### Camera and the other side's picture (2026-11-25)

| Part | Where | What |
|---|---|---|
| Kernel | `drivers/gen_xhci/uvc.rs`, `svcdev/kamera.rs` | USB Video Class 1.1/1.5, MJPEG: at attach only the descriptors (frame 320×240, else the smallest larger one); opening `/dev/ultar/kamera` (right `camera`, bit 13, one opener) negotiates VS_PROBE/VS_COMMIT at 15 pictures a second, selects the largest alternate setting that carries the payload (falls back when the bus refuses the bandwidth), queues 192 isochronous TDs (xHCI isoch TRBs, start as soon as possible) and puts frames together (frame id, end of frame, error bit); the reader gets the newest whole JPEG per `read`. The event ring has 1024 entries. |
| Call service | `ultar-call/src/camera.rs` | `Camera` reads the node without waiting, puts in the standard Huffman tables (cameras leave them out), decodes and scales to 320×240; `CallVideo` = camera (optional) + `PictureSink` (the camera's own JPEG goes on for the own picture, nothing is encoded twice). Without a camera a video call still shows the other side. `--camera NODE` (default from `ultar-hal-system::seat_nodes::camera_node`). |
| Bus | `call.picture` (`CallPicture{own, jpeg}`, ≤ 256 KiB, not retained) | published by `system.calld`, followed only by `app.phone` (bus policy). |
| Phone app | `src/picture.rs`, `view/call.rs`, `bin/ultar-phone.rs` | the other side's picture (`PixelArea`, 320×240) and the own camera's (in the small own card, made smaller to fit) appear with their first picture and go when no call is active; a burst of pictures shows only the newest of each. |

**Tests:** `ultar-call` camera tests (tables put in, scaling, a file standing in for the node);
`apps/ultar-phone/tests/states.rs` (picture shown, replaced in place, gone with the call);
`tools/kern-test/kamera-e2e.sh` (in `ci.sh` without a camera: the node is absent; with
`KAMERA_USB=046d:08e5` a real camera passed into QEMU). Real run 2026-11-25: Logitech C920 through
usbipd → WSL → QEMU `usb-host` (`KAMERA_HOSTOPTS=isobufs=32,isobsize=64`): 12 whole pictures in 8 s,
none broken, no transfer errors; the low rate is the usbip path (whole frames lost on the way), not
the driver.

**Open:** a camera that streams only over bulk endpoints and uncompressed (YUY2) cameras are not
offered (every current USB webcam offers MJPEG over isochronous endpoints).
