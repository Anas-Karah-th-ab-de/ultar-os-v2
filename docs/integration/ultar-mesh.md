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
| Lending the language model: a device announces the class of its model, a paired device with the right `Model` lets it think (`mesh.think`, `mesh.models`); prompt and answer in the encrypted channel | `src/model.rs`, `src/lending.rs` | 3 + 1 |
| Network: announcements (UDP 47810 broadcast), channels (TCP 47810), `--peer` for networks without broadcast | `src/net.rs` | 1 (two devices find each other) |
| Relay for devices in other networks: the account server forwards ciphertext, the Noise channel runs end to end inside it (`--relay`, `--relay-token-file`) | `src/relay.rs` | 1 + 1 (stand-in relay) |
| What may be sent: only regular files inside the shared folders, links followed (`--share`) | `src/transfer.rs` | 1 |
| What a job may run: only a program by bare name inside the jobs folder (`--jobs`); without the folder no job runs | `src/runtime.rs` | 1 |
| Service on the bus (`ultar-svckit`), binary `ultar-mesh` | `src/service.rs`, `src/bin/ultar-mesh.rs` | 1 (two real services) |

58 tests. `tests/network.rs` runs two runtimes over real sockets (pairing, a 1.5 MB document, restart
without a new code, jobs with and without permission, finding each other by announcements, meeting through
a stand-in relay that follows the server's frame rules). `tests/process.rs` starts two real `ultar-mesh`
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
controlling, sound, computing and the language model are off until granted (`mesh.set_rights`). Unpaired devices can pair and
nothing else – every other frame is dropped.

## Wiring it in (not done here, on purpose)

Two other branches change the same shared files, so this branch adds new files only (plus one line each
in `Cargo.toml` and `messages/mod.rs`). To ship the service:

- `rootfs/etc/ultar/services/mesh.toml`: own user, **no kernel right**, `--state`, `--inbox`, `--name`, `--kind`.
- `rootfs/etc/ultar/ids.toml`: user `mesh`.
- `rootfs/etc/ultar/bus-policy.toml`:
  `system.mesh` registers `mesh`, publishes `mesh.*`, calls `launcher.list`, `launcher.start`,
  `sysdialog.handoff`;
  the shell and `app.settings` call `mesh.*` and subscribe `mesh.*`; `app.*` call `mesh.set_activity` only.
- Settings page "Mesh", quick-settings tile, the gesture on the phone shell and the window-menu entry on
  the desktop shell (each shell its own surface).
- Apps: Files, Browser, Text editor report their activity and accept `ULTAR_OPEN_URI` with a place.

## Decisions taken while building (please confirm)

1. **A device met through the relay is not trusted because of the account.** v1 trusted every device of
   the same account automatically; here the server only brings devices together and the six-digit code
   is still needed once. Reason: the server could otherwise add a device to the person's mesh.
2. Ports 47810 (UDP and TCP) instead of v1's 47800/47801, so both can run during the change-over.
3. Without `--share` nothing can be sent, without `--jobs` no job runs.

## Not done / not tested – honest list

- **Not wired into the system image.** Service file, user id, bus listener, log listener, bus policy and
  the image scripts are untouched: `build/btscan`, `build/net`, `build/settings2` and `build/appbus` change
  the same lines of `ids.toml`, `services/bus.toml`, `bus-policy.toml`, `service_files_tests.rs` and
  `make-vmware.sh`. The list above ("Wiring it in") is what has to be added once they are merged.
- **Hardly any surface.** Built: the question on the receiving device (desktop system dialog,
  `sysdialog.handoff`; the mesh asks through it and takes its answer). Not built: settings page,
  quick-settings tile, gesture, window-menu entry, the phone sheets, a command-line tool; no app reports
  its activity. Boards and description: `docs/ui/mesh.md` (approved 2026-11-24).
- **Relay token (done 2026-11-25).** `ultar-accountd` writes the device token after the person signs in to
  `/data/ultar/account-relay/relay.token` (group `accountrelay`, only it and the mesh service), the mesh reads
  it before every connection (`Login::token_file`), so the relay follows sign-in and sign-out without a
  restart. The relay protocol as the mesh uses it (role `dienst`, open frame with the sender's key) is
  checked against the release server by `server/cloud-e2e/tests/live_server.rs`.
- **Remote view, sound, calls and notifications across devices.** `RemoteView` is planned for in the
  stage logic and switched off (`Config::remote_view = false`): the compositor has no window sharing yet.
  A speaker or TV is found and paired but has nothing it can be handed (no "continue playing there" stage).
- **Device identity.** The mesh has its own X25519 key; it is not yet tied to the Ed25519 device identity
  of `docs/DEVICE-IDENTITY.md`.
- **Files are still opened by the service** (inside the shared folders only). A descriptor from the app
  would be better; the bus cannot pass descriptors yet.
- **Jobs are not run in a cell** (only the programs of the jobs folder run, with the service's user).
- Large files are hashed on the service loop (a 4 GiB file blocks it while hashing).
- **No real hardware**: not run on the Ultar kernel, the Beelink, a VM or the A30; ARM64 not built.
- The clipboard owner (compositor) does not call `mesh.clipboard_set` yet; computing jobs have no bus call.

Everything that is still open, in the order it should be finished: `docs/OFFEN-VERBUND-HELFER.md`.

## Nachtrag 24.11.2026: auf einem anderen Gerät handeln, Abbild

- `mesh.act { device, text }` → `{ ok, steps, by }` (`ultar_bus::messages::mesh_remote`): der Helfer
  hier bittet den Helfer eines gekoppelten Geräts. Das andere Gerät nimmt nur an, wenn es diesem Gerät
  das Recht „Dieses Gerät steuern“ gab; es ruft dann seinen eigenen Helfer (`agent.ask`). Während es
  eine solche Bitte ausführt, nimmt es keine zweite an und gibt keine weiter. Wartezeit 150 s.
- Im Abbild: Benutzer 833, `services/mesh.toml` (Ausgabe Desktop), Port 47810 (TCP, UDP) in
  `rootfs/etc/ultar/firewall`. Name des Geräts: `--name`, sonst `HOSTNAME`, sonst „device“.
- Tests: 60 (neu: `tests/remote.rs`, 2).

## Nachtrag 25.11.2026: was offen ist, je App

- Eine App meldet mit `mesh.set_activity`, was in ihr offen ist; der Verbund merkt es sich je App (höchstens 16).
- Die Hülle nennt mit demselben Aufruf die App im Vordergrund (App und Titel). Hat diese App selbst etwas
  gemeldet, gilt ihre Meldung (Ort oder Datei); sonst wird die App auf dem anderen Gerät einfach geöffnet,
  wenn sie dort installiert ist.
- Neu im Vertrag (`messages/mesh.rs`): `Grant` (anderer Name für `Right`, für Oberflächen-Code),
  `Peer::new`, `Peer::grants`, `Peer::set_grants`, `SetRights::of`.
- Tests: 61.

## Nachtrag 25.11.2026 (2): Kennungen, Ausgaben

- Eine Aktivität wird unter der Kennung geführt, die der Starter kennt (`launcher_id`: `org.ultar.files` → `files`).
  Der Bus-Name einer App ist dieselbe Kennung; nur unter ihr darf sie melden.
- Geräteart ohne `--kind`: aus `ULTAR_EDITION` (phone, desktop, server). Name ohne `--name`: `HOSTNAME`, sonst die Art.
- Die Dienstdatei gilt für Handy, Desktop und Server.
