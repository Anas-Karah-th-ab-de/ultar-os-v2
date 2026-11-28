# Mesh – the surface (`settings.mesh`, `shell.mesh`)

KM Code Craft · Ultar OS v2 · 2026-11-24 · status: **boards approved by the owner on 2026-11-24** (step 3 of DESKTOP-SPEC §2.2); built so far: the question on the receiving device (desktop)
Service and contract: `docs/integration/ultar-mesh.md`, `system/core/ultar-bus/src/messages/mesh.rs`
Boards: `design/boards/b9.py` → `design/boards/project/{SettingsMesh,SettingsMeshLight,MeshPairShow,MeshPairJoin,MeshDevice,MeshIncoming,MeshSend,MeshStates,CompactMeshSend,CompactMeshIncoming}.dc.html`
Clicks: `docs/ui/CLICKS.md` (20 actions `mesh.*`, all with a defined effect)

## 1. Purpose

The person's own devices work as one: what is open on one device continues on another, files and copied
text follow. Ubuntu 26.04 has no counterpart (`ultar-only`). Two rules of the owner shape every surface:

1. **Nothing arrives unasked** – the receiving device asks its person every time.
2. **The sender chooses** as soon as more than one device can take the activity.

Phone and desktop have their own surfaces (no phone surface in a window): a dialog and a settings window
with sidebar on the desktop shell, sheets from the bottom edge and a list → page settings flow on the phone
shell. Both use the same bricks, tokens, texts and bus calls.

## 2. Surfaces

| Id | Kind | Shell | Board | Needs |
|---|---|---|---|---|
| `settings.mesh` | panel | both | SettingsMesh, SettingsMeshLight | component Mesh installed |
| `settings.mesh.pair_show` | dialog | both | MeshPairShow | Mesh on |
| `settings.mesh.pair_join` | dialog | both | MeshPairJoin | Mesh on, a device in reach |
| `settings.mesh.device` | panel | both | MeshDevice | a paired device |
| `shell.mesh.incoming` | system dialog (desktop) · bottom sheet (phone) | each its own | MeshIncoming, CompactMeshIncoming | an offer |
| `shell.mesh.send` | dialog (desktop) · bottom sheet (phone) | each its own | MeshSend, CompactMeshSend | an activity in front, ≥ 2 possible devices |
| `shell.mesh.toasts` | toasts | both | MeshStates | – |

### 2.1 Settings › Mesh

- **Mesh** switch (`mesh.set_enabled`): off = no port open, nobody sees this device; paired devices stay.
  The quick-settings tile (`qs.mesh`, already on the boards) shows the same state and the number of
  connected devices.
- **This device**: name and kind, device id (monospace, left-to-right island) with Copy.
- **Folder for received files**: Change opens the system folder dialog.
- **My paired devices**: icon by kind, name, "kind · connected or last seen · local network or over the
  internet", state badge (colour + icon + word), arrow to the device page. Primary action of the page:
  **Pair a device** (shows the code).
- **Nearby devices, not paired**: seen because Mesh is on there; one action each: Pair (type its code).

### 2.2 Pairing

- *Show*: six digits in two groups, time left, "five attempts". Cancel ends the code. The dialog closes by
  itself when the other device typed the right code, and the device appears in the list.
- *Join*: one field (digits, left-to-right), error text below the field with the attempts left, primary
  action Pair. Enter = Pair, Esc = Cancel.

### 2.3 One paired device

Seven switches, effective at once: offer activities · send files · share the clipboard (on after pairing) ·
see this device's screen · control this device · sound · use computing power (off until granted).
Each row says what it means in one line. **Unpair** is the one danger action and asks first.

### 2.4 The question on the receiving device

Title "Continue from ‹device›?" (plain file: "Receive a file from ‹device›?"), one line what it is, a box
with the file or page (icon by kind, name, app, size, "opens at the same place"), badges *paired device*
and *end-to-end encrypted*, the line "question of the system · declined after 60 seconds · nothing is
stored or opened before you accept". Actions: **Accept** (primary) · Decline. Esc = Decline.
Desktop: the compositor's trusted system dialog, everything else dimmed. Phone: sheet from the bottom edge
with touch-size buttons, Accept above Decline. On the lock screen only "‹device› offers something" is
shown; the content appears after unlocking.

### 2.5 The choice on the sending device

Opens only when two or more devices can take the activity; with exactly one the offer goes there directly.
A radio group of devices: name, what will happen there ("opens in ‹app› at the same place" · "the file
only" · "not connected" = cannot be chosen). Actions: **Send** (primary) · Cancel.
How it is reached – desktop: window menu "Continue on another device…" and Super+Shift+M; phone: long press,
then swipe up (the gesture of v1), or the share sheet.

### 2.6 Toasts and states

Waiting for the answer (with Cancel) · opened there · declined · nobody answered within a minute (Retry) ·
connection lost, nothing stored (Retry) · a file arrived (Open). Page states: Mesh off · on without
devices (empty state with Pair a device) · nothing in front that can be continued · no device can take it
now · the port could not be opened (Retry).

## 3. Input

Keyboard: Tab order follows the reading order; radio groups move with the arrow keys; Enter triggers the
primary action, Esc the safe one. Screen reader: every device row is read as "name, kind, state"; the
question is an alert dialog; toasts are status (warnings: alert). Pointer targets 32, touch targets 44.

## 4. Built

| Surface | State | Where | Proof |
|---|---|---|---|
| `shell.mesh.incoming` (desktop) | built | `platform/ultar-desktop/src/dialog/handoff.rs`, question `sysdialog.handoff` (`messages/mesh.rs`), asked by `ultar-mesh` | reference pictures `tests/snapshots/dialog-handoff-{ar,en}-{dark,light}.png` (pixel compare), model test (accept, decline, unanswered = declined), two real mesh services with a dialog stand-in (accepted: opened; declined: nothing stored, nothing opened) |
| `settings.mesh`, `settings.mesh.device`, both pairing dialogs | built (2026-11-25) | `apps/ultar-settings/src/{service,state,update,view}/mesh.rs`, dialogs in `view/dialog.rs` | `apps/ultar-settings/tests/mesh.rs` (5, driven by clicks) |
| quick-settings tile `qs.mesh` | built | `platform/ultar-desktop/src/model/quick.rs`, `shell/sheets.rs` | `platform/ultar-desktop/tests/mesh.rs` |
| `shell.mesh.send` | built (2026-11-28) | `platform/ultar-desktop/src/shell/handoff.rs`, `view/mesh_send.rs`, `model/mesh.rs`, `ports/bus_mesh.rs` | `tests/mesh.rs`, `tests/mesh_conformance.rs` (board `MeshSend`: names, x, width, state; the card height is the named deviation below), unit tests in `model/mesh.rs` |
| `shell.mesh.toasts` | built (2026-11-28) | `view/mesh_toasts.rs`, `app/desktop/mesh_toasts.rs`, `model/mesh.rs`; toolkit toasts with Cancel (`mesh.offer.cancel`), Retry (`mesh.retry`) and Open (`mesh.received.open`, from the bus topic `mesh.received`) | `tests/mesh_conformance.rs` (board `MeshStates`: all ten toast buttons, 0 missing, 0 different), `model/mesh.rs` unit tests |
| quick-settings arrow `qs.mesh.list` | built (2026-11-28) | `view/mesh_list.rs`, `shell/sheets.rs`, `shell/handoff.rs` (`list_device`): the list of the paired devices inside the sheet; a connected device gets what is in front, one that is not connected opens its place in Settings | `tests/mesh.rs` |
| phone sheet `CompactMeshIncoming` | built (2026-11-25) | `platform/ultar-mobile/src/dialog/handoff.rs` (+ `sheet.rs`), program `ultar-mobile-dialog` (`src/program/dialog.rs`, service `mobile-dialog.toml`, edition phone), shared state `platform/ultar-dialog-core` | `tests/dialog_render.rs` (ar/en, dark/light, 360 × 760), `tests/dialog_clicks.rs` (accept, decline, unanswered = declined), `tests/dialog_e2e.rs` (real compositor in phone mode + bus: a stand-in mesh asks `sysdialog.handoff`, a tap on Accept / Decline answers `accept: true` / `false`) |
| phone `qs.mesh` tile + `CompactMeshSend` + notices | built (2026-11-28) | `platform/ultar-mobile/src/model/mesh.rs`, `view/{quick,recents,mesh}.rs` (sheet: back arrow, radio group, Send, Cancel; notice card with Cancel / Retry / Open / Close), `program/{shell,bus}.rs` (calls `mesh.handoff_send`, `mesh.offer_cancel`; topics `mesh.state`, `mesh.handoff_progress`, `mesh.received`) | `tests/clicks.rs` (tile, recents button, hold + swipe up, choice, back, every notice button), `tests/mesh_conformance.rs` (boards `CompactMeshSend`, `CompactMeshIncoming`; two named deviations below), `tests/mesh_e2e.rs` (real compositor + bus), `tests/render.rs` |
| flow phone -> desktop | built (2026-11-28) | `platform/ultar-mobile/tests/handoff_flow.rs` | two real `ultar-mesh` services, each on its own bus: the gesture on the phone's recents card sends, the desktop's dialog view asks and its Accept is pressed, the launcher opens the same app at `#line=3` from the received file, the phone's notice turns to "opened" |

Deviations from the board, on purpose: the wait is said as "a minute" (the dialog service answers after
55 s, the mesh gives up after 60 s); "nothing is stored or opened before you accept" moved from the system
line into the text so the line fits one row in every language. The boards were regenerated with this
wording and with the word the system already uses for pairing (`إقران`).

Named deviations (2026-11-28), to be settled with the owner:

- **Device cards (`MeshSend`, `CompactMeshSend`).** `ultar-ui` has no device-card layout of the radio group: its
  cards have padding 12 and a radio dot, the boards' have padding 8, the device icon and a check mark at the
  end. The cards are therefore 69 / 67 / 67 px high (board 61.6 / 59.6 / 59.6) and the dialog, which is
  centred, sits higher by the difference. The conformance tests name exactly these boxes (five on the desktop,
  four on the phone) and fail when anything else differs; the count must go to 0 when the toolkit gets the
  layout. Fix: a small additive layout `RadioLayout::Devices` in `platform/ultar-ui/src/widgets/` (owner
  decision pending, the toolkit is outside this package).
- **Touch targets (phone).** The phone's buttons are at least 44 px high; the board draws 32 px buttons inside
  44 px rows. Same top edge and width, 12 px taller (`CompactMeshSend`, `CompactMeshIncoming`).
- **Settings page rows of `MeshStates`** (`mesh.toggle`, `mesh.pair`, the Retry for "port could not be
  opened") belong to `apps/ultar-settings`; the conformance test of this package leaves them out.
- **Skeleton lines** behind the phone's sheet (the board's grey bars standing for the page) are not drawn;
  the page behind the scrim is the title and the back arrow only.
- **Reached by**: the entry "Continue on another device" in the active app's menu and **Super+Shift+M**
  (plain Super+M does nothing; the key handler sees Shift now). On the phone: the recents card held and
  swiped up, the mesh button of the card, and the arrow of the tile (the app in front). The phone has no
  app menu on the home screen yet (CLICKS `mobile.app`: long press).
- **Phone notices** show above the navigation bar as a card with a button, not in the status bar.
- Settings: the row "folder for received files" (`mesh.inbox`) is not there (the service has no call for
  it); the pairing dialog does not show the attempts left; the device page has no state badge.
- Reference pictures of the settings page do not exist yet.

## 5. Open

- Device kinds reuse the icons `phone`, `device`, `monitor`, `server` (as the Account board does). A TV and
  a speaker need icons of their own (`kind-tv`, `kind-speaker`) – one icon, one meaning.
- The medium size class (600–1023) and the high-contrast theme are not drawn; light and English exist for
  the settings page only.
- Shipping the question needs the rule `system.mesh` may call `sysdialog.handoff` in `bus-policy.toml` (with the rest of the wiring).
- Wording was not read by a native speaker.
