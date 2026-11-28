# Mesh: Anrufe, Ton und Hinweise zwischen Geräten (C2/C3)

Stand 28.11.2026 · Paket `calls` · Branch `q/calls`.

Die Geräte der Person arbeiten als eins: Ein Anruf, der auf dem Handy klingelt, klingelt
auf dem Schreibtisch mit und kann dort geführt werden (Mikrofon und Lautsprecher des
Schreibtischs). Hinweise des Handys erscheinen auf dem Schreibtisch. Ton lässt sich zu einem
anderen Gerät schicken („dort abspielen“). Alles läuft Ende-zu-Ende-verschlüsselt über den
bestehenden Mesh-Kanal (Noise); auf keinem Server liegt oder entsteht etwas.

## Aufbau

```
Handy (Gastgeber)                                  Schreibtisch (Gast)
 Modem/ID-Leitung <-> calld (MeshRelay) <== mesh ==> calld (MeshRelay) <-> Klang (Tap)
                        mesh.call_*                    mesh.call_*       Banner (desktop)
```

* **Recht `Right::Call` („Anrufe und Ton“)**: nach dem Koppeln **aus**. Es wird
  empfängerseitig geprüft (ohne Recht klingelt nichts und fließt kein Ton). Für ein
  Gespräch muss es beidseitig gesetzt sein. In den Einstellungen als Schalter (Mesh-Seite,
  `apps/ultar-settings`).
* **Frames** (`ultar-mesh/src/wire.rs`): `Call`, `CallAnswer`, `CallEnd`, `CallAudio`
  (`id`, `seq`, `pcm`), `Note`, dazu die Medien-Frames für „dort abspielen“ (`media_link.rs`).
* **Bus-Vertrag** (`ultar-bus/src/messages/`): `mesh_calls.rs` (`mesh.call_offer`,
  `call_answer`, `call_end`, `call_audio`; Topics `call_incoming`, `call_progress`,
  `call_audio`; `CallMeshMute`, `MeshCallState`), `mesh_media.rs`, `notify_mirror.rs`.

## Anruf (calld)

`ultar-call` bekommt `MeshRelay` (`mesh_relay.rs`) mit zwei Rollen:

* **Gastgeber** (Handy, `--mesh-calls`): bietet jeden eingehenden Anruf allen gekoppelten
  Geräten mit Recht an. **Wer zuerst annimmt, bekommt das Gespräch**; die übrigen Angebote
  werden beendet, späte Annahmen werden abgewiesen (`turned_away`). Beim Annehmen
  antwortet die Leitung des Handys; die Sprache läuft Handy-Leitung <-> Mesh.
* **Gast** (Schreibtisch, `--mesh-guest`): zeigt das Angebot, nimmt Mikrofon und
  Lautsprecher, schickt und spielt Sprachrahmen.
* **Auflegen** von jeder Seite beendet beide Seiten. Geht die Mesh-Verbindung mitten im
  Gespräch verloren, bleibt der Anruf auf dem Handy (kein Abbruch).
* SIM-Anrufe werden nur angeboten, wenn ein Modem-Sprachpfad gesetzt ist
  (`--mesh-modem-pcm`). ID-Anrufe (Ultar-Nummer) immer.
* Was das Angebot zeigt, ist nur, was das Handy schon anzeigt (Anrufername/-nummer).

### Sprache und Qualität

* **PCM 16 bit, 48 kHz mono, 20 ms** = 960 Samples = 1920 Byte je Rahmen, base64
  (ohne Padding). Kein Opus: Es liegt kein reines Rust-Opus im Workspace. Auf dem LAN ist
  PCM billig; falsche Größen werden verworfen.
* **Jitter** (`mesh_jitter.rs`): Tiefe 3 Rahmen (60 ms), ordnet um, verwirft Verspätete.
* **Verlust**: „wiederholen und halbieren“ (jeder weitere fehlende Rahmen halbiert noch
  einmal), danach Stille.
* Die **ID-Anruf-Stimme** ist intern 16 kHz und wird für das Mesh auf 48 kHz verdreifacht
  (`mesh_voice.rs`): Die Qualität bleibt die der 16-kHz-Strecke.
* Die Rahmen werden in einem eigenen Pacer-Thread im 20-ms-Takt abgespielt, damit der
  Dienstzyklus den Takt nicht stört.

## Hinweise (notifyd)

`ultar-notifyd/src/mirror.rs`:

* **Hinausspiegeln** nur von **Klartext**: genau der Titel und der Text, wie gepostet
  (Titel auf 128, Text auf 1024 Zeichen gekürzt). Hinweise von System und alles
  Vertrauliche (`Private`) verlassen das Gerät **nie**. Positionsargumente mit Übersetzung
  werden nicht gespiegelt.
* **Datenschutz** (`notify_mirror.rs`): `Normal` zeigt auf gesperrtem Ziel nur, *dass*
  etwas ankam; `HideOnLocked` zeigt dort gar nichts.
* **Herein**: Dringlichkeit wird auf `Normal` begrenzt, Absender `mesh`, der Hinweis nennt
  das Quellgerät (`desk.note.mirror.title`, `.locked`).
* **Gesperrt** heißt hier: kein angemeldeter Benutzer (`session.state.user == None`) oder
  unbekannt. Es gibt keinen Bus-Zustand für „Bildschirm gesperrt, Benutzer angemeldet“;
  dieser Fall wird deshalb **nicht** erkannt (offen).

## Schreibtisch-Oberfläche

`platform/ultar-desktop` (`model/calls.rs`, `ports/bus_calls.rs`): Banner für eingehenden
Anruf (annehmen/ablehnen), laufender Anruf mit **Stumm** und **Auflegen**, gespiegelte
Hinweise mit Quellgerät. Rechts-nach-links zuerst; Schlüssel `desk.*` in ar/en/de.
Bildschirmfotos: `call-incoming`, `call-active`, `note-mirrored` (ar/en, dunkel/hell),
im Test `tests/calls.rs`. Nicht gebaut: Lautsprecher-Umschalter (nur Stumm und Auflegen);
Videoangebote klingeln als Sprachanruf.

## Dort abspielen (mediad)

`ultar-mediad` (`there.rs`, `there_service.rs`): gekoppelte Geräte mit Recht erscheinen als
Ausgang; der Ton geht über das Mesh, Pause/Fortsetzen/Lautstärke wirken auf beiden Seiten.
Die Senke öffnet `/dev/ultar/pcm` je Sitzung, daher kann sie mit einem laufenden Anruf
kollidieren; **es gibt kein Mischen**. Start mit `--there-sink`.

## Richtlinie und Dienste (SCOPE-EXTRA)

* `rootfs/etc/bus-policy.toml`: Einträge für `system.mesh`, `system.calld`,
  `system.notifyd`, `system.mediad`, `system.shell` (publish/subscribe/call für die neuen
  Themen). `media.system`/`notify.system` bleiben für Systemzugriffe maßgeblich.
* `rootfs/.../services/call.toml`, `mediad.toml`: neue Argumente. `--mesh-modem-pcm` ist
  **nicht** gesetzt.
* Bestehende Tests von `notifyd` und `mediad` haben neue Richtlinien-Fixtures.

## Tests (Beweise)

* `ultar-bus`: Namen/Rundlauf der neuen Nachrichten.
* `ultar-mesh`: `wire`, `audio_link`, `tests/media.rs`, `tests/calls.rs` (zwei Knoten:
  ohne Recht nichts; klingeln -> annehmen -> Ton beidseitig in Reihenfolge -> auflegen von
  jeder Seite; vertraulicher Hinweis geht nie raus).
* `ultar-mesh/tests/calls_process.rs`: zwei **echte** Mesh-Dienste mit Bus.
* **`tools/calls-e2e/tests/calls_e2e.rs`** (Tools-Bereich, weil Dienste einander nicht abhängen dürfen): zwei echte Mesh-Dienste (echte Sockets, Noise) und
  auf jedem Gerät der echte calld-`Service` mit Fake-Leitung und Fake-Klang. Klingeln,
  Annehmen auf dem Schreibtisch, sechs Markierungs-Samples in **beide** Richtungen in
  Reihenfolge, Auflegen vom Schreibtisch, zweiter Anruf, Auflegen vom Handy.
* `ultar-call/tests/mesh_relay.rs` (7), `ultar-notifyd/tests/mirror.rs`,
  `ultar-mediad/tests/there.rs` (zwei echte mediad-Prozesse), `ultar-desktop/tests/calls.rs`,
  `ultar-settings/tests/mesh.rs`.

## Offen und ehrlich

1. **Echo**: audiod ist nur Steuerung, es gibt keinen Echo-Canceller. Freisprechen am
   Schreibtisch kann Echo erzeugen; mit Kopfhörer kein Problem. Nicht gelöst.
2. **SIM-Sprachpfad**: kein Modem-PCM-Pfad im System; der SIM-Tap ist nur mit Fakes
   getestet, nicht auf echter Hardware.
3. **Kein Opus**: PCM kostet etwa 768 kbit/s je Richtung; für WLAN/LAN ok, für schwache
   Funkstrecken nicht.
4. **Hardware**: nichts davon lief auf echten Geräten/Mikrofonen; alles mit Fakes und
   echten lokalen Sockets.
5. **Sperrzustand** siehe oben; **Lautsprecher-Taste** und **Video** fehlen.
6. **Kollision** „dort abspielen“ gegen laufenden Anruf auf demselben Klanggerät.
