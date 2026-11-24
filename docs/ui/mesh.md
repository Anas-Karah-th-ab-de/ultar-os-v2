# Mesh – the surface (`settings.mesh`, `shell.mesh`)

KM Code Craft · Ultar OS v2 · 2026-11-24 · status: **spec – boards await approval** (step 3 of DESKTOP-SPEC §2.2)
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

## 4. Open before approval

- Device kinds reuse the icons `phone`, `device`, `monitor`, `server` (as the Account board does). A TV and
  a speaker need icons of their own (`kind-tv`, `kind-speaker`) – one icon, one meaning.
- The medium size class (600–1023) and the high-contrast theme are not drawn; light and English exist for
  the settings page only.
- The system dialog needs a call in `sysdialog` for this question (today: permission and authentication).
- Wording was not read by a native speaker.
