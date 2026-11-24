# ultar-mesh – integration notes

KM Code Craft · Ultar OS v2 · 2026-11-24 · replaces `system/services/ultar-verbund` (v1, kept until this is wired in)

The device mesh: the person's own devices find each other, are paired once with a six-digit code and hand
work over – an activity ("continue on the other device"), files, the clipboard, computing jobs.
Mining is not part of it.

## The two rules the owner set

1. **Nothing arrives unasked.** The receiving device always asks its person (`mesh.handoff_incoming`,
   answered with `mesh.handoff_answer`). Before the answer nothing is written and nothing is opened.
2. **The sender chooses when several devices can take it.** `mesh.handoff_send` without a target answers
   `Choose(targets)`; the shell shows the list and sends again with the chosen device. With exactly one
   possible device it goes there directly (and is still asked there).

## Devices are described by what they can do

`Capability`: screen, input, audio-out, audio-in, files, apps, clipboard, compute. `DeviceKind` (phone,
tablet, desktop, server, board, tv, speaker, other) is for the icon only – no decision reads it.
On the wire kinds and capabilities are words; unknown words and unknown frames are ignored, not refused.
A TV (screen + sound) or a speaker (sound) therefore joins with the same protocol and is offered only what
it can take. What is missing for them is a *stage*, not a new protocol (see "Not done").

## What exists

| Part | File | Tests |
|---|---|---|
| Bus contract `mesh.*` | `system/core/ultar-bus/src/messages/mesh.rs` | – |
| Device id (from the key), kinds, capabilities | `src/device.rs` | 4 |
| Wire frames (JSON, one frame per Noise message, max 60 000 bytes) | `src/wire.rs` | 3 |
| Encrypted channel: Noise XX 25519 / ChaCha20-Poly1305 / BLAKE2s, both sides prove their key | `src/secure.rs` | 3 |
| Trust store, rights per device, pairing (SPAKE2 over the code, bound to both static keys, 5 attempts, 5 min) | `src/trust.rs`, `src/pairing.rs` | 4 + 6 |
| Where an activity can go and how it arrives (`State` · `File` · `RemoteView`) | `src/handoff.rs` | 5 |
| Offers sent and received, timeouts, flood limit (4 open offers per device) | `src/offers.rs`, `src/received.rs` | 9 |
| File transfer: 32 KiB pieces, acknowledged, part file, size + SHA-256 before the name is given, names cannot leave the inbox | `src/transfer.rs` | 3 |
| Clipboard: text up to 16 KiB, confidential entries never leave, no echo | `src/node.rs` | in handoff tests |
| Computing jobs: exactly one result per job, silence 8 s → requeue, 4 strikes → out, stragglers twice, own device as reserve, lending side answers a repeated id from memory | `src/compute.rs`, `src/runtime.rs` | 6 + 1 |
| Network: announcements (UDP 47810 broadcast), channels (TCP 47810), `--peer` for networks without broadcast | `src/net.rs` | via network tests |
| Service on the bus (`ultar-svckit`), binary `ultar-mesh` | `src/service.rs`, `src/bin/ultar-mesh.rs` | 1 (two real services) |

47 tests. `tests/network.rs` runs two runtimes over real sockets (pairing, a 1.5 MB document, restart
without a new code, jobs with and without permission). `tests/process.rs` starts two real `ultar-mesh`
binaries on two brokers with a stand-in launcher: switched on by the shell, paired, a document handed
over, accepted, opened through `launcher.start` at the same place (`file://…#line=3`).

## Handoff, step by step

1. An app (or the shell for it) reports what is in front: `mesh.set_activity { app, title, uri, file }`.
   An app may only speak for itself.
2. The shell asks `mesh.handoff_targets` or sends at once with `mesh.handoff_send`.
3. Stage per target: same app installed → `State` (the file travels if there is one, the place in it –
   `#…`/`?…` of the URI – is kept); else a device that keeps files → `File`; else, when this device can
   show its windows, a device with a screen → `RemoteView`.
4. Receiver: right `Handoff` (plain file: `Files`) is checked first; without it the offer is declined
   without bothering the person. Then `mesh.handoff_incoming`.
5. Accepted → pieces → verified file in the inbox → `launcher.start { app, uri }` (`File` stage: the
   opener app, default `files`). `mesh.handoff_progress` on both devices: Asked → Accepted → Opened, or
   Declined / Expired (60 s) / Failed.

## Rights

A freshly paired device may: offer activities, offer files, share the clipboard. Seeing the screen,
controlling, sound and computing are off until granted (`mesh.set_rights`). Unpaired devices can pair and
nothing else – every other frame is dropped.

## Wiring it in (not done here, on purpose)

Two other branches change the same shared files, so this branch adds new files only (plus one line each
in `Cargo.toml` and `messages/mod.rs`). To ship the service:

- `rootfs/etc/ultar/services/mesh.toml`: own user, **no kernel right**, `--state`, `--inbox`, `--name`, `--kind`.
- `rootfs/etc/ultar/ids.toml`: user `mesh`.
- `rootfs/etc/ultar/bus-policy.toml`:
  `system.mesh` registers `mesh`, publishes `mesh.*`, calls `launcher.list`, `launcher.start`;
  the shell and `app.settings` call `mesh.*` and subscribe `mesh.*`; `app.*` call `mesh.set_activity` only.
- Settings page "Mesh", quick-settings tile, the gesture on the phone shell and the window-menu entry on
  the desktop shell (each shell its own surface).
- Apps: Files, Browser, Text editor report their activity and accept `ULTAR_OPEN_URI` with a place.

## Not done / not tested – honest list

- **Account trust and relay.** Devices outside the local network (through the server) and automatic trust
  between devices of one account are not ported from v1. Pairing is by code only, in the same network.
- **Remote view, sound, calls and notifications across devices.** `RemoteView` is planned for in the
  stage logic and switched off (`Config::remote_view = false`): the compositor has no window sharing yet.
  A speaker or TV is found and paired but has nothing it can be handed (no "continue playing there" stage).
- **Device identity.** The mesh has its own X25519 key; it is not yet tied to the Ed25519 device identity
  of `docs/DEVICE-IDENTITY.md`.
- **Files are read by the service.** `Activity::file` and `mesh.send_file` name a path the service opens
  with its own user. Before shipping, the file must come from the app as a descriptor (or through a
  portal), otherwise the service user decides what can be read.
- **Jobs run a program by name** for a device that was granted `Compute` – that is remote execution by
  design; it needs a cell before the right is offered in the settings.
- **Broadcast discovery** was not run (the test machine has no broadcast network); connections were made
  with `--peer`. Large files are hashed on the service loop (a 4 GiB file blocks it while hashing).
- **No real hardware**: not run on the Ultar kernel, the Beelink, a VM or the A30; ARM64 not built.
- The clipboard owner (compositor) does not call `mesh.clipboard_set` yet; computing jobs have no bus call.
