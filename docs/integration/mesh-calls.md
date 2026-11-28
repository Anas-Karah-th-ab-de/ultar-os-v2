# Mesh: Anrufe, Ton und Hinweise zwischen Geräten (C2/C3)

Stand 28.11.2026 · Paket `calls` · Stufe: Mesh-Schicht fertig, Dienstanbindung offen.

Dieser Text beschreibt die neue Mesh-Schicht für „die Geräte der Person arbeiten als
eins“: ein Anruf, der auf dem Handy klingelt, klingelt auf dem Schreibtisch mit und kann
dort geführt werden (Ton durch Mikrofon und Lautsprecher des Schreibtischs); Hinweise des
Handys erscheinen auf dem Schreibtisch; Ton kann zu einem anderen Gerät geschickt werden.
Alles läuft Ende-zu-Ende-verschlüsselt über den bestehenden Mesh-Kanal (`secure.rs`,
Noise XX) – auf keinem Server liegt oder entsteht etwas.

## Was gebaut ist (und was beweist es)

### Neues Recht „Anrufe und Ton“ (`Right::Call`)

In `ultar_bus::messages::mesh` gibt es das neue Recht `Right::Call`. Es ist **nach dem
Koppeln aus** (`DEFAULT_RIGHTS` bleibt `Handoff, Files, Clipboard`). Es deckt drei Dinge
ab, die dasselbe Vertrauen brauchen:

* einen Anruf anbieten, annehmen, ablehnen und auflegen,
* die Sprache des Anrufs übertragen,
* Hinweise spiegeln.

Für ein Gespräch in beide Richtungen muss das Recht **beidseitig** gesetzt sein: das
Handy braucht es auf dem Schreibtisch (damit der Schreibtisch klingelt und die Stimme des
Handys nimmt), der Schreibtisch braucht es auf dem Handy (damit das Handy das Mikrofon
des Schreibtischs nimmt). Das ist symmetrisch und einfach: beide Geräte vertrauen sich
gegenseitig „Anrufe und Ton“.

### Rahmen (Frames) in `wire.rs`

Neue Frames im verschlüsselten Kanal, alle `serde_json` mit `t`-Tag wie bisher:

| Frame | Felder | Bedeutung |
|---|---|---|
| `Call` | `id`, `caller`, `video` | Anruf anbieten; `caller` ist nur, was das Handy schon zeigt |
| `CallAnswer` | `id`, `accept` | annehmen / ablehnen |
| `CallEnd` | `id` | auflegen (von beiden Seiten) |
| `CallAudio` | `id`, `seq`, `pcm` | eine 20-ms-Rahmen Sprache (PCM, base64) |
| `Note` | `seq`, `title`, `text`, `app`, `urgency`, `hide_on_locked` | gespiegelter Hinweis |

### Zustandsmaschine (`ultar-mesh/src/calls.rs`)

`Node` bekommt den Zustand `Calls` (Angebote raus/rein, Zähler der Hinweise) und Methoden:

* `call_offer(device, caller, video)` – bietet den Anruf einem erreichbaren, gekoppelten
  Gerät mit `AudioIn` **und** `AudioOut` an (Rückgabe: Angebots-`id`).
* `call_answer(id, accept)` – dieses Gerät nimmt an / lehnt ab.
* `call_end(id)` – dieses Gerät legt auf.
* `call_audio(device, id, seq, pcm)` – schickt einen Sprachrahmen.
* `note_mirror(…)` – spiegelt einen Hinweis.

Wie beim Klemmbrett gilt das Recht auf der **Empfängerseite**: ohne `Right::Call`
klingelt nichts und fließt kein Ton (`no right → nothing`, getestet). Ein Hinweis, der
als vertraulich markiert ist (Passwort, Einmalcode), verlässt das Gerät **nie**; der
Absender prüft `hide_on_locked` und der Ziel-Gerät bekommt das Flag mit (zeigt den
Hinweis nur auf entsperrtem Ziel).

### Sprache (Audio) über das Mesh (`ultar-mesh/src/audio_link.rs`)

Kein reines-Rust-Opus ist in `Cargo.lock` (geprüft, `grep -i opus` leer) – also fahren
wir **PCM 16-bit, 48 kHz mono, 20 ms**: 960 Samples = **1920 Byte** je Rahmen, base64 im
Frame. Das ist auf dem LAN ehrlich und billig; beide Seiten prüfen die Größe (1920 Byte,
sonst wird der Rahmen verworfen, getestet). Ein `Jitter` puffert 3 Rahmen (60 ms),
ordnet um und füllt einen verlorenen Rahmen mit einer leisen Kopie des letzten
(„repeat and fade“: jeder weitere fehlende halbiert nochmal). Echo: **offen** – dazu
siehe unten; die bestehende Jitter-/PLC-Logik von `ultar-call/src/e2e/audio.rs` (dort
IMA-ADPCM, 16 kHz) bleibt für die SIM-/Id-Strecke, die Mesh-Strecke nutzt den neuen
48-kHz-Puffer.

### Bus-Vertrag (`ultar-bus/src/messages/mesh_calls.rs`)

Damit `calld`, die Oberfläche und `notifyd` mit dem Mesh reden, ohne das Mesh zu kennen:

* Methoden: `mesh.call_offer`, `mesh.call_answer`, `mesh.call_end`, `mesh.call_audio`
  (Sprachrahmen **hin**, base64), `mesh.note_mirror`.
* Topics: `mesh.call_incoming`, `mesh.call_progress` (Phasen `offered/accepted/
  declined/ended/failed`), `mesh.call_audio` (Sprachrahmen **her**), `mesh.note`.

`ultar-mesh/src/service.rs` bedient die Methoden und veröffentlicht die Topics (zwei
kleine Haken im `request`/`host`). Die Namesprüfung ist im `mod.rs`-Test ergänzt.

## Tests

* `wire.rs`: `Call`/`CallAnswer`/`CallEnd`/`CallAudio`/`Note` überleben den Draht.
* `audio_link.rs`: feste Größe + Round-Trip, Reihenfolge + Fade bei Lücken, verspäteter
  Rahmen wird verworfen.
* `tests/calls.rs` (zwei Knoten, echt verdrahtet wie zwei Geräte):
  * ohne Recht klingelt nichts,
  * klingeln → annehmen auf dem Schreibtisch → Ton in **beiden** Richtungen in
    Reihenfolge (mit Markierungs-Samples) → auflegen von **jeder** Seite,
  * vertraulicher Hinweis geht nie raus; normaler kommt mit Feldern und Flag an,
  * Rahmen falscher Größe wird verworfen.
* `cargo test -p ultar-bus -p ultar-mesh` grün; `cargo fmt` und `clippy -D warnings`
  sauber.

## Offen (nächste Schritte)

1. **`calld` anbinden** (`system/services/ultar-call`): bei eingehendem Anruf
   `mesh.call_offer` an erreichbare Geräte; „das erste Gerät, das annimmt, bekommt das
   Gespräch“ liegt in `calld` (es beendet die übrigen Angebote). Sprachrahmen
   Handy ↔ Schreibtisch über `mesh.call_audio`/`mesh.call_audio`; das Handy behält die
   SIM-/Id-Strecke. Jitter/PLC liefert `audio_link::Jitter`.
2. **Schreibtisch-Oberfläche** (`platform/ultar-desktop`): Banner für eingehenden Anruf
   + Bedienelemente (stumm, laut, auflegen) nach `design/boards/project/PhoneInCall`,
   an den Schreibtisch angepasst; gespiegelte Hinweise in das lokale `notifyd` mit
   Quell-Gerät.
3. **„Dort weiterspielen“** (`ultar-mediad`): gekoppelte Geräte mit `AudioOut` als
   Ausgang anbieten; Media-Audio über das Mesh (`Right::Audio` für reines Abspielen).
4. **`bus-policy.toml`**: die neuen `publish`- und `call`-Einträge für `system.mesh`,
   `system.calld`, `system.notifyd` und `system.shell` eintragen (Datei liegt außerhalb
   der erlaubten Dateien dieses Pakets).
5. **Einstellungen** (`apps/ultar-settings`): das neue Recht auf der Mesh-Seite als
   Schalter (arabisch zuerst).
6. **Echo** gegen einen echten Raum testen; audiod bietet keinen Echo-Canceller – für
   den Schreibtisch-Freisprech-Fall dokumentieren/ergänzen.

Die Meshs-Schicht ist der gesicherte Unterbau dafür; die Dienste binden sich über den
Bus-Vertrag an, ohne das Protokoll zu sehen.