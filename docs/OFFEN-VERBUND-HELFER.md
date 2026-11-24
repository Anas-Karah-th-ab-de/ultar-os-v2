# Geräteverbund und lokaler Helfer – was noch fehlt

KM Code Craft · Ultar OS v2 · Stand 24.11.2026 · gehört zu `docs/integration/ultar-mesh.md`,
`docs/integration/ultar-agentd.md`, `docs/ui/mesh.md`

Diese Liste ist die Merkliste zum Vollenden. Jede Zeile nennt, was fehlt, woran es hängt und wo es
hingehört. Erledigtes wird hier gestrichen, nicht umformuliert.

## A. Erst wenn die Branches `build/btscan`, `build/net`, `build/settings2`, `build/filesqs` in `main` sind

Sie ändern dieselben Zeilen. Vorher angefasst gäbe es sichere Konflikte.

| # | Was | Wo |
|---|---|---|
| ~~A1~~ | ~~Dienst `ultar-mesh` ins Systemabbild~~ – erledigt: Benutzer 833, `services/mesh.toml` (nur Ausgabe Desktop), Bus- und Protokoll-Zugang, Regeln, Port 47810 in `firewall`, VMware-Skripte. Offen bleibt: Eingangsordner je Person, `--share`, `--jobs`, `--relay`, Dienstdatei für das Handy | – |
| ~~A2~~ | ~~Dienst `ultar-agentd` genauso~~ – erledigt: Benutzer 834, `services/agentd.toml`, Regeln | – |
| A3 | Einstellungen › Verbund (Tafeln 1–5: Seite, Koppeln, Gerät mit Rechten) | `apps/ultar-settings` |
| A4 | Recht „Sprachmodell verleihen“: ~~Tafel 5 hat den achten Schalter~~ – der Schalter in den Einstellungen kommt mit A3 | `apps/ultar-settings` |
| A5 | Auswahl am Sender, Meldungen, Kachel in den Schnelleinstellungen, Fenstermenü-Eintrag (Tafeln 7, 8) | `platform/ultar-desktop` |
| A6 | Handy: Blatt „Weitergeben“ mit Geste, Blatt „Annehmen?“ (Tafeln 9, 10) | Handy-Hülle |

## B. Braucht eine Erweiterung in einem anderen Dienst

| # | Was | Hängt an |
|---|---|---|
| ~~B1~~ | ~~Modell am Bus~~ – erledigt als eigener kleiner Dienst `ultar-llm-bus` (Benutzer 835): bietet `llm.state` und `llm.complete` und reicht sie an die Buchse des Modell-Dienstes weiter. `ultar-llm` selbst ist unverändert. | – |
| ~~B2~~ | ~~Klasse des Modells~~ – erledigt nach Dateigröße (bis 1,7 GB klein, bis 5,8 GB mittel, darüber groß). Offen: das Gerät einrechnen (Speicher, Beschleuniger), sobald iGPU/NPU fertig sind | `ultar-llm-bus/src/backend.rs` |
| B3 | Zugang zum Konto-Server für Geräte in anderen Netzen: der Konto-Dienst gibt dem Verbund ein Ticket | `system/services/ultar-accountd` |
| B4 | Apps melden, was offen ist (`mesh.set_activity`), und öffnen mit Stelle: Dateien, Browser, Texteditor | `apps/*` |
| B5 | Zwischenablage: der Compositor meldet Kopiertes (`mesh.clipboard_set`) und übernimmt `mesh.clipboard` | `platform/ultar-compositor` |
| B6 | Dateien als Deskriptor von der App statt als Pfad (heute: nur aus freigegebenen Ordnern) | `ultar-bus` (Deskriptor-Übergabe) |
| B7 | Rechenaufgaben in einer Zelle ausführen (heute: nur Programme des Aufgaben-Ordners) | Zellen am Bus |

## C. Eigene Arbeit, noch nicht begonnen

| # | Was |
|---|---|
| C1 | Fernansicht: Fenster eines Geräts auf einem anderen zeigen und bedienen (Stufe `RemoteView` ist vorgesehen und abgeschaltet) |
| C2 | Ton, Anrufe und Benachrichtigungen zwischen Geräten |
| C3 | Fernseher und Lautsprecher: eigene Symbole (`kind-tv`, `kind-speaker`) und eine Stufe „dort weiterspielen“ |
| C4 | Verbund-Schlüssel an die Geräte-Identität binden (`docs/DEVICE-IDENTITY.md`) |
| C5 | Entscheidung offen: automatisches Vertrauen zwischen Geräten desselben Kontos (heute: immer Code) |
| C6 | Tafeln: mittlere Größe (Tablet), hoher Kontrast, hell/englisch für alle Flächen |
| C7 | Helfer: Oberfläche (Helfer-App, Eingabe in der Leiste, je Hülle eigene Fläche) – erst Tafeln, dann Bau |
| C8 | Helfer: Sprache (Weckwort „Hey KM“, Spracherkennung, Sprachausgabe, Arabisch zuerst) |
| C9 | Helfer: mehr Werkzeuge – Einstellungen lesen und setzen, Bluetooth, Helligkeit, Dateien suchen, Store, Treiber |
| C10 | Helfer: Wissen aus der Hilfe (Themen des alten Systems als Nachschlage-Werkzeug) |
| ~~C11~~ | ~~Helfer: auf einem anderen Gerät handeln~~ – erledigt: Werkzeug `ask_device`, Aufruf `mesh.act`; das andere Gerät nimmt es nur mit dem Recht „Dieses Gerät steuern“ an, sein eigener Helfer führt aus, Gefährliches bestätigt die Person dort. Keine Ketten, keine Kreise. Offen: Text der Antwort des anderen Helfers (heute nur seine Schritte) |
| C12 | Helfer: Antwort Stück für Stück anzeigen (heute kommt sie am Ende) |

## D. Noch nicht geprüft

- Kein Lauf auf dem Ultar-Kern, dem Beelink, in VMware oder auf dem A30; ARM64 nicht gebaut.
- Verbindung über den echten Konto-Server (bisher nur ein Ersatz-Server nach dessen Regeln).
- Rundruf im echten Netz (bisher Ankündigungen auf einer Maschine).
- `ultar-llm-bus` gegen den echten Modell-Dienst: bisher nur gegen einen Ersatz mit denselben drei
  Anfragen (`/health`, `/v1/models`, `/v1/chat/completions`). Der Modell-Dienst startet nur auf
  Anforderung (`manual = true`); wer ihn für den Helfer startet, ist offen (gehört zu C7).
- Das Abbild mit den drei neuen Diensten wurde nicht gebaut und nicht gestartet; geprüft sind die
  Dienstdateien durch die Tests von `ultar-busd`, `ultar-init` und `ultar-logd`.
- Helfer mit einem echten Modell: Die Tests nutzen ein Modell-Skript. Ob ein kleines Modell die
  Antwortform zuverlässig einhält, ist offen; schlägt es fehl, bleibt die kleinste Hilfe.
- Arabische Texte (Tafeln, Dialog, Helfer) hat kein Muttersprachler gegengelesen.
