# Geteilte Zwischenablage zwischen gekoppelten Geräten (Verbund B5)

Kopieren auf einem Ultar-Gerät, Einfügen auf dem gekoppelten Gerät – Text und Bilder, privat
by design. Dieses Dokument beschreibt Compositor-Rolle, Verbund-Dienst, Einstellung, Hinweis
und die Tests. Dienst und Kopplung im Allgemeinen: `docs/integration/ultar-mesh.md`.

## 1. Überblick

```
App A (Fenster im Fokus)          Compositor A        ultar-mesh A  ~~ Noise ~~  ultar-mesh B       Compositor B        App B
 Strg+C  --SetSelection-->  merkt die Auswahl  --SelectionChange-->  liest Text/PNG      ...   Frame Clip / ClipImage   --BridgeSetSelection-->  bietet an  --SelectionOffer-->  Strg+V
                                              <--BridgeReadSelection--                                                                      <--ReadSelection--
```

* Der Compositor gibt die Auswahl sonst nur der App im Fokus (`state/transfer.rs`). Für den
  Verbund gibt es eine eigene, eng begrenzte Rolle: die **Zwischenablage-Brücke**.
* Der Verbund-Dienst (`ultar-mesh`, Benutzer 833) ist die einzige Brücke. Er verbindet sich nur,
  solange der Verbund eingeschaltet ist; ausgeschaltet liest nichts die Zwischenablage.
* Ohne Compositor-Socket (`--compositor`) oder ohne das Recht bleibt die Zwischenablage auf dem
  Gerät; der Dienst versucht es alle 2 s still erneut.

## 2. Compositor: Recht `ui.clipboard-bridge`

| Was | Wert |
| --- | --- |
| Recht | `Rights::CLIPBOARD_BRIDGE` (Bit 512), Manifestname `ui.clipboard-bridge` |
| Vergabe | nur über `rootfs/etc/ultar/compositor-rights.toml`: `833 = "ui.clipboard-bridge"` (wie `ui.capture` für 825). Apps können es nicht bekommen (`APP_GRANTABLE` enthält es nicht; Test `the_launchers_file_never_gives_the_system_ui`) |
| Anfrage 58 `BridgeSetSelection { mimes }` | bietet eine Auswahl an, deren Daten die Brücke selbst liefert (Kopie von einem anderen Gerät). Nur `text/plain;charset=utf-8` und `image/png`, höchstens 2 Typen, sonst `Malformed`. Die Auswahl ist als „anderes Gerät“ (`remote`) markiert und nie vertraulich |
| Anfrage 59 `BridgeReadSelection { mime }` | liest die aktuelle Auswahl. Nur die zwei Typen; Text höchstens 16 KiB, Bild höchstens 4 MiB (größere Übertragungen werden abgebrochen). Antwort wie bei einem Einfügen: `Data`-Stücke oder `TransferCancelled` |
| Ereignis 42 `SelectionChange { mimes, confidential }` | geht an jede Brücke, wenn der Mensch kopiert hat (`SetSelection` mit Fokus und frischer Nutzeraktion) |
| Gesperrte Sitzung | Brücken-Anfragen: Fehler `Permission`; `SelectionChange` wird nicht gesendet (auch nicht, wenn der Sperrbildschirm selbst kopiert) |
| Vertraulich | `confidential = true`, wenn das fokussierte Textfeld ein Passwortfeld ist (`TextInput.purpose == 2`, gesetzt von `ultar-ui`). Dann gibt der Compositor die Daten nie heraus (`TransferCancelled`) |
| Ohne Recht | beide Anfragen: `Permission` |
| Eigenes Angebot lesen | wird abgebrochen (keine Schleife) |

Der Client-Helfer steht in `ultar-surface` (`Client::bridge_set_selection`, `bridge_read_selection`);
Konstanten `CLIP_TEXT_MIME`, `CLIP_IMAGE_MIME`.

## 3. Verbund-Dienst

### 3.1 Dateien (alle in `system/services/ultar-mesh/`)

| Datei | Aufgabe |
| --- | --- |
| `src/clip_bridge.rs` | Thread mit der Compositor-Verbindung: liest Kopien (Text vor Bild, eine zweite Änderung wartet auf das Ende der ersten, 5 s Zeit), bietet fremde Einträge an und liefert deren Daten stückweise (`SendRequest` → ein `SendData`) |
| `src/clip_service.rs` | Bus-Seite: Brücke starten/beenden (mit `set_enabled`), Hinweis über `notify.post` |
| `src/clipboard.rs` | Knoten: Text senden/empfangen (bestehend, jetzt mit Recht beidseitig) |
| `src/clip_share.rs` | Knoten: Bilder, Lebensdauer 2 min, Hinweis höchstens alle 10 s |
| `src/clip_image.rs` | Bildstücke: Format, Zusammensetzen, Prüfungen |
| `src/wire.rs` | eine Zeile: `Frame::ClipImage(Piece)` (Tag `clip_image`) |

Hook-Zeilen in bestehenden Dateien: `node.rs` (Feld `clip`, drei Effekt-Varianten, zwei
Frame-Zeilen, `tick`), `service.rs` (Feld `clip`, Effekt-Verteilung, Start/Stopp der Brücke),
`trust.rs` (Standardrechte), `bin/ultar-mesh.rs` (Option `--compositor PATH`).

### 3.2 Regeln

* **Recht „Zwischenablage teilen“ (`Right::Clipboard`) je Gerät, Standard aus.** Es gilt in beide
  Richtungen: dieses Gerät sendet seine Kopien nur an Geräte, die es hat, und nimmt Einträge
  nur von ihnen. Ein neu gekoppeltes Gerät bekommt es nicht (`DEFAULT_RIGHTS` = Übergabe, Dateien).
  Bereits gekoppelte Geräte behalten, was in `peers` gespeichert ist. Beide Geräte müssen es
  einschalten.
* **Vertrauliche Einträge verlassen das Gerät nie** (Compositor sendet sie der Brücke nicht;
  der Knoten verwirft sie zusätzlich).
* **Kein Echo:** was von einem Gerät kam, wird nicht zurückgeschickt (Text: gleicher Text; Bild:
  gleicher SHA-256).
* **Lebensdauer 2 Minuten** (`clip_share::LIFE`): `Node::tick` meldet `ClipboardClear`; die Brücke
  nimmt das Angebot zurück, **nur wenn es noch das Angebot des Verbunds ist** (eine neuere
  Kopie des Menschen bleibt).
* **Hinweis** „Auf <Gerät> kopiert – einfügen mit Strg+V“ über den bestehenden
  Benachrichtigungsdienst (`notify.post`, App-Id `org.ultar.mesh`, Dringlichkeit niedrig,
  Kategorie System), höchstens einer je 10 s (`NOTICE_GAP`); der Eintrag selbst kommt trotzdem.
  Schlüssel `mesh.clip.title` (`{arg0}` = Gerätename, Daten) und `mesh.clip.body` in
  `platform/ultar-desktop/locales/{ar,en,de}.ftl`.
* **Grenzen:** Text 16 KiB (`MAX_CLIP`), Bild 4 MiB (`MAX_IMAGE`), nur PNG.

### 3.3 Bilder auf dem Draht (Frame `clip_image`, Version 1)

Ein Frame ist höchstens 60 000 Byte; ein Bild wird deshalb in Stücke zu je 32 KiB
(`wire::CHUNK`, base64) zerlegt:

```json
{"t":"clip_image","v":1,"seq":7,"total":300000,"sha256":"<64 hex>","index":0,"data":"<base64>"}
```

Der Empfänger nimmt die Stücke streng der Reihe nach (`index` = nächster erwarteter; ein
Stück 0 beginnt neu), nie mehr als `total` ≤ 4 MiB, prüft am Ende SHA-256 und PNG-Signatur und
verwirft Unvollständiges nach 30 s. Ein Stück mit unbekannter Version `v` wird ignoriert; ein
Gerät, das `clip_image` nicht kennt, liest `Unknown` und ignoriert den Frame (siehe `wire.rs`).
Es läuft höchstens ein Bild je Gerät gleichzeitig; Stücke von Geräten ohne Recht lösen nichts aus.

## 4. Einstellung

Einstellungen › Verbund › (Gerät) › „Zwischenablage teilen“ – der bestehende Schalter
(`Grant::Clipboard`, `apps/ultar-settings`), Untertitel: „Kopierter Text und Bilder folgen;
Passwörter nie“ (ar/en/de angepasst). Wirkt sofort; beide Geräte stellen ihn für das andere ein.

## 5. Dienstdatei und Regeln

* `rootfs/etc/ultar/services/mesh.toml`: `--compositor /run/ultar/ui/compositor.sock`.
* `rootfs/etc/ultar/compositor-rights.toml`: `833 = "ui.clipboard-bridge"`.
* `rootfs/etc/ultar/bus-policy.toml` (`system.mesh`): zusätzlich `notify.post`.
* Neue Abhängigkeit des Dienstes: `ultar-surface` (ohne Feature `ui`), bereits im Arbeitsbereich;
  keine neue externe Bibliothek.
* `tools/check-layers.toml`: Ausnahme Dienste → `platform/ultar-surface` und
  `platform/ultar-compositor` (Brücke ist Client des Compositors; der Ende-zu-Ende-Test startet
  den echten kopflosen Compositor).
* Neue Protokoll-Nummern in `ultar-surface`: Anfragen 58 und 59, Ereignis 42. Sie wurden als
  nächste freie gewählt; fügt ein anderes Paket gleichzeitig Nummern hinzu, muss beim Zusammenführen
  neu nummeriert werden.
* Vorhandene, schon gekoppelte Geräte behalten ihre gespeicherten Rechte; die neue Voreinstellung
  „Zwischenablage aus“ gilt nur für neue Kopplungen.

## 6. Tests

| Test | Was bewiesen wird |
| --- | --- |
| `platform/ultar-compositor/tests/clipboard_bridge.rs` (10) | ohne Recht abgelehnt; Brücke liest Text/Bild; Passwort nie; gesperrt nichts (Lesen, Anbieten, Kopie auf dem Sperrbildschirm); fremde Typen nie; kein Rücklesen des eigenen Angebots; Text > 16 KiB und Bild > 4 MiB abgebrochen (4 MiB genau geht) |
| `platform/ultar-surface` `proto::tests` | Anfragen 58/59, Ereignis 42 rund (Kodierung/Dekodierung), Recht in `ALL` |
| `ultar-mesh` `clip_image` (6) | Stücke, 4-MiB-Grenze, falscher Hash/Lücke/Wiederholung/anderer Absender, Größenlüge, Nicht-PNG, neue Version, Langsamkeit, Stück passt in einen Frame |
| `ultar-mesh` `tests/clip.rs` (9) | zwei Knoten: Text und Bild kommen an; nichts ohne Recht (Standard aus, Empfänger ohne Recht, Bild); vertraulich nie, kein Echo; Ablauf nach 2 min (Text, Bild); ein Hinweis je 10 s; beschädigtes Bild; ungekoppelt nichts |
| `ultar-mesh` `tests/clip_e2e/` (1) | Ende zu Ende mit den ausgelieferten Bus-Regeln: je Gerät ein echter Compositor (kopflos) mit dem Recht, ein echtes `ultar-mesh` mit eigenem Bus, Stellvertreter für den Benachrichtigungsdienst, ein App-Fenster. Kopieren im Fenster des Telefons → Einfügen im Fenster des Rechners: Standard aus (nichts kommt), nach Einschalten Text (Deutsch/Arabisch) + Hinweis mit Gerätename, 300-KB-Bild bytegleich, Passwort kommt nie an (auch nicht im Bus-Thema `mesh.clipboard`), Recht zurückgenommen → verworfen, Gegenrichtung |
| `ultar-mesh` `tests/handoff.rs` | bestehender Zwischenablage-Test angepasst: Standard aus, dann freigegeben |

## 7. Offen und Grenzen

* **Quelle „anderes Gerät“:** der Compositor merkt die Auswahl der Brücke als `remote`
  (`Transfers::selection_remote`, getestet über das Verhalten „kein Rücklesen“), aber kein
  Protokollereignis und keine Oberfläche zeigt es an. Eine Anzeige braucht eine Erweiterung der
  Oberfläche (Einfügen-Menü der Shell).
* **Ablauf nach 2 Minuten** ist mit eingespeister Zeit im Knoten geprüft, nicht mit echter Uhr
  im Ende-zu-Ende-Test. Die Brücke nimmt das Angebot im Compositor zurück (Code in
  `clip_bridge.rs`); dieser Schritt hat keinen eigenen Test gegen den echten Compositor.
* **Gesperrte Sitzung auf der Verbund-Seite** ist nur im Compositor getestet, nicht im Ende-zu-Ende-Test.
  Ein Eintrag, der während der Sperre ankommt, geht verloren (das Anbieten wird abgelehnt).
* **Vertraulichkeit** hängt daran, dass die App das Feld als Passwort kennzeichnet. Kopiert eine
  App ein Passwort aus einem normalen Feld, erkennt der Compositor das nicht.
* Nur `text/plain;charset=utf-8` und `image/png`; sind beide angeboten, wird der Text gelesen.
* Rechte-Datei: der Test vergibt das Recht dem Benutzer des Tests; die Zuordnung `833` mit echter
  Kernel-Anmeldung (`SO_PEERCRED` unter dem Dienstmanager) ist auf einem Gerät nicht gelaufen.
* Das Bus-Thema `mesh.clipboard` (Text eines anderen Geräts) bleibt wie vorher bestehen; nur die
  Shell und die Einstellungen dürfen `mesh.*` abonnieren.
