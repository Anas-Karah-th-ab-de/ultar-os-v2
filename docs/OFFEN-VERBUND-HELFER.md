# Geräteverbund und lokaler Helfer – was noch fehlt

KM Code Craft · Ultar OS v2 · Stand 24.11.2026 · gehört zu `docs/integration/ultar-mesh.md`,
`docs/integration/ultar-agentd.md`, `docs/ui/mesh.md`

Diese Liste ist die Merkliste zum Vollenden. Jede Zeile nennt, was fehlt, woran es hängt und wo es
hingehört. Erledigtes wird hier gestrichen, nicht umformuliert.

## A. Erst wenn die Branches `build/btscan`, `build/net`, `build/settings2`, `build/filesqs` in `main` sind

Sie ändern dieselben Zeilen. Vorher angefasst gäbe es sichere Konflikte.

| # | Was | Wo |
|---|---|---|
| A1 | Dienst `ultar-mesh` ins Systemabbild: Dienstdatei, Benutzer, Bus- und Protokoll-Zugang, Regeln | `rootfs/etc/ultar/{services/mesh.toml,ids.toml,services/bus.toml,services/logd.toml,bus-policy.toml}`, `service_files_tests.rs`, `tools/vm/make-vmware.sh` |
| A2 | Dienst `ultar-agentd` genauso | dieselben Dateien, `services/agentd.toml` |
| A3 | Einstellungen › Verbund (Tafeln 1–5: Seite, Koppeln, Gerät mit Rechten) | `apps/ultar-settings` |
| A4 | Recht „Sprachmodell verleihen“ auf der Geräte-Seite (achter Schalter; Tafel 5 nachziehen) | `design/boards/b9.py`, `apps/ultar-settings` |
| A5 | Auswahl am Sender, Meldungen, Kachel in den Schnelleinstellungen, Fenstermenü-Eintrag (Tafeln 7, 8) | `platform/ultar-desktop` |
| A6 | Handy: Blatt „Weitergeben“ mit Geste, Blatt „Annehmen?“ (Tafeln 9, 10) | Handy-Hülle |

## B. Braucht eine Erweiterung in einem anderen Dienst

| # | Was | Hängt an |
|---|---|---|
| B1 | Modell-Dienst am Bus: `llm.state` und `llm.complete` anbieten. Ohne das gibt es weder ein eigenes noch ein verliehenes Modell, der Helfer bleibt bei der kleinsten Hilfe. | `system/services/ultar-llm` – **nicht angefasst**, weil dort die Arbeit an iGPU, NPU und 3D-Beschleunigung läuft |
| B2 | Klasse des Modells (klein, mittel, groß) aus Modellgröße und Gerät bestimmen | wie B1 |
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
| C11 | Helfer: Werkzeuge auf einem **anderen** Gerät ausführen („auf dem Server: …“) – braucht ein eigenes Recht je Gerät und die Rückfrage dort |
| C12 | Helfer: Antwort Stück für Stück anzeigen (heute kommt sie am Ende) |

## D. Noch nicht geprüft

- Kein Lauf auf dem Ultar-Kern, dem Beelink, in VMware oder auf dem A30; ARM64 nicht gebaut.
- Verbindung über den echten Konto-Server (bisher nur ein Ersatz-Server nach dessen Regeln).
- Rundruf im echten Netz (bisher Ankündigungen auf einer Maschine).
- Helfer mit einem echten Modell: Die Tests nutzen ein Modell-Skript. Ob ein kleines Modell die
  Antwortform zuverlässig einhält, ist offen; schlägt es fehl, bleibt die kleinste Hilfe.
- Arabische Texte (Tafeln, Dialog, Helfer) hat kein Muttersprachler gegengelesen.
