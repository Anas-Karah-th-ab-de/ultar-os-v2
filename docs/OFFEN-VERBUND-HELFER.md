# Geräteverbund und lokaler Helfer – was noch fehlt

KM Code Craft · Ultar OS v2 · Stand 25.11.2026 · gehört zu `docs/integration/ultar-mesh.md`,
`docs/integration/ultar-agentd.md`, `docs/ui/mesh.md`

Diese Liste ist die Merkliste zum Vollenden. Jede Zeile nennt, was fehlt, woran es hängt und wo es
hingehört. Erledigtes wird hier gestrichen, nicht umformuliert.

## A. Oberfläche und Abbild

`build/appbus`, `build/btscan`, `build/net`, `build/settings2` sind noch offen und ändern dieselben Dateien
(Einstellungen, Hülle). Das hier Gebaute liegt deshalb in eigenen Dateien mit wenigen Einhängepunkten.

| # | Was | Wo |
|---|---|---|
| ~~A1~~ | ~~Dienst `ultar-mesh` ins Systemabbild~~ – erledigt: Benutzer 833, `services/mesh.toml` (Handy, Desktop, Server; die Geräteart folgt der Ausgabe), Bus- und Protokoll-Zugang, Regeln, Port 47810 in `firewall`, VMware-Skripte. Offen bleibt: Dateien (siehe B6), `--jobs`, `--relay`, ein eigener Gerätename (heute: Art des Geräts) | – |
| ~~A2~~ | ~~Dienst `ultar-agentd` genauso~~ – erledigt: Benutzer 834, `services/agentd.toml`, Regeln | – |
| ~~A3~~ | ~~Einstellungen › Verbund~~ – erledigt: Schalter, dieses Gerät mit Kennung, gekoppelte Geräte, Geräte in der Nähe, Code zeigen, Code eintippen, Geräte-Seite, Kopplung aufheben. Offen: Zeile „Empfangsordner“ (dem Dienst fehlt der Aufruf), verbleibende Versuche im Dialog, Zustands-Plakette, Referenzbilder | `apps/ultar-settings` |
| ~~A4~~ | ~~Recht „Sprachmodell verleihen“~~ – erledigt: achter Schalter auf Tafel und Geräte-Seite | – |
| A5 | ~~Kachel, „Auf anderem Gerät weitermachen“ (App-Menü, Super+M), Auswahl, Meldungen~~ – gebaut, aber **abweichend von der Tafel** (Auswahl als Menü statt Dialog, Meldungen ohne Knöpfe, Pfeil der Kachel sendet): siehe `docs/ui/mesh.md` §4. Offen: Dialog nach Tafel 7, Knöpfe Abbrechen/Erneut/Öffnen, Liste der Geräte im Blatt | `platform/ultar-desktop` |
| A6 | Handy: Blatt „Weitergeben“ mit Geste, Blatt „Annehmen?“ (Tafeln 9, 10). Heute: Senden über den Pfeil der Kachel, Annehmen über die Desktop-Frage; auf dem Handy nicht geprüft | Handy-Hülle, Compositor (Geste) |

## B. Braucht eine Erweiterung in einem anderen Dienst

| # | Was | Hängt an |
|---|---|---|
| ~~B1~~ | ~~Modell am Bus~~ – erledigt als eigener kleiner Dienst `ultar-llm-bus` (Benutzer 835): bietet `llm.state` und `llm.complete` und reicht sie an die Buchse des Modell-Dienstes weiter. `ultar-llm` selbst ist unverändert. | – |
| ~~B2~~ | ~~Klasse des Modells~~ – erledigt nach Dateigröße (bis 1,7 GB klein, bis 5,8 GB mittel, darüber groß). Offen: das Gerät einrechnen (Speicher, Beschleuniger), sobald iGPU/NPU fertig sind | `ultar-llm-bus/src/backend.rs` |
| B3 | Zugang zum Konto-Server für Geräte in anderen Netzen: der Konto-Dienst gibt dem Verbund ein Ticket. **Nicht begonnen** – braucht eine Entscheidung: heute sieht außer dem Konto-Dienst niemand das Token; ein Ticket braucht auch den Server | `system/services/ultar-accountd`, Server |
| B4 | Apps melden, was offen ist. ~~Verbund merkt sich je App die Meldung; die Hülle nennt die App im Vordergrund; ohne Meldung wird die App dort einfach geöffnet; Dateien meldet seinen Ordner~~ – erledigt. Offen: Browser (hat keine Bus-Verbindung), Texteditor, Büro-Apps; Datei mit Stelle | `apps/ultar-web`, `apps/ultar-office` |
| B5 | Zwischenablage. **Nicht begonnen**: der Compositor gibt Kopiertes nur der App im Vordergrund; es braucht dort eine eigene Rolle für den Verbund | `platform/ultar-compositor` |
| B6 | **Dateien im Abbild gehen nicht.** Die Bereiche der Person (`/data/ultar/areas`) erreicht nur der Starter; der Verbund kann dort weder lesen (kein `--share`) noch Empfangenes ablegen (eigener Eingang, den die Dateien-App nicht zeigt). Sauber geht es nur mit Deskriptoren von der App über den Bus. **Nicht begonnen.** Weitergeben von Offenem (App, Ort) geht ohne Dateien | `ultar-bus`, Starter |
| B7 | Rechenaufgaben in einer Zelle ausführen. **Nicht begonnen** | Zellen am Bus |

## C. Eigene Arbeit

Nicht begonnen sind C1, C2, C4, C6, C8, C12; C5 ist eine Entscheidung des Eigentümers.

| # | Was |
|---|---|
| C1 | Fernansicht: Fenster eines Geräts auf einem anderen zeigen und bedienen (Stufe `RemoteView` ist vorgesehen und abgeschaltet) |
| C2 | Ton, Anrufe und Benachrichtigungen zwischen Geräten |
| C3 | Fernseher und Lautsprecher: ~~Wort und Symbol je Geräteart in den Einstellungen~~ (Symbole `monitor`, `speaker` wiederverwendet). Offen: eigene Symbole, Stufe „dort weiterspielen“ |
| C4 | Verbund-Schlüssel an die Geräte-Identität binden (`docs/DEVICE-IDENTITY.md`) |
| C5 | Entscheidung offen: automatisches Vertrauen zwischen Geräten desselben Kontos (heute: immer Code) |
| C6 | Tafeln: mittlere Größe (Tablet), hoher Kontrast, hell/englisch für alle Flächen |
| C7 | Helfer: Oberfläche. ~~Erreichbar über die Suche der Übersicht: was getippt ist, lässt sich „den Helfer fragen“; die Antwort kommt als Meldung und bleibt in der Liste~~ – gebaut ohne Tafel. Offen: eigene Fläche nach Tafeln (Helfer-App, Verlauf, Schritte sichtbar), Handy |
| C8 | Helfer: Sprache (Weckwort „Hey KM“, Spracherkennung, Sprachausgabe, Arabisch zuerst) |
| C9 | Helfer: mehr Werkzeuge. ~~Bluetooth, Flugmodus, Medientasten, Bildschirmfoto, Sperren, Schlafen (mit Bestätigung)~~ – erledigt. Offen: Einstellungen lesen und setzen, Helligkeit, Dateien suchen, Store, Treiber |
| C10 | Helfer: Wissen. ~~Acht Themen (Koppeln, Weitermachen, Dateien, Modell, WLAN, Bluetooth, Aktualisierungen, Helfer) in drei Sprachen, ohne Modell beantwortet, als Werkzeug `help` für das Modell~~ – erledigt. Offen: die Themen des alten Hilfe-Systems übernehmen |
| ~~C11~~ | ~~Helfer: auf einem anderen Gerät handeln~~ – erledigt: Werkzeug `ask_device`, Aufruf `mesh.act`; das andere Gerät nimmt es nur mit dem Recht „Dieses Gerät steuern“ an, sein eigener Helfer führt aus, Gefährliches bestätigt die Person dort. Keine Ketten, keine Kreise. Offen: Text der Antwort des anderen Helfers (heute nur seine Schritte) |
| C12 | Helfer: Antwort Stück für Stück anzeigen (heute kommt sie am Ende) |

## D. Geprüft und noch nicht geprüft

Geprüft (25.11.2026): Verbund und Helfer zusammen wie auf zwei Geräten, mit den echten Diensten `ultar-mesh`,
`ultar-agentd`, `ultar-llm-bus` und den **ausgelieferten** Bus-Regeln (`system/services/ultar-agentd/tests/together.rs`):
koppeln, Modell verleihen, das Handy denkt auf dem Rechner und handelt bei sich, der Helfer des Handys lässt den
Helfer des Rechners dort eine App öffnen. Dabei gefunden und behoben: Oberflächen nennen Apps `org.ultar.files`,
Starter und Bus sagen `files` – die Meldung der Dateien-App wurde abgewiesen, der Helfer fragte den Starter
nach Kennungen, die es nie gibt.

Noch nicht geprüft:


- Die neue Oberfläche (Einstellungen › Verbund, Kachel, Weitermachen) lief nur in den Tests mit Ersatz-Diensten,
  nicht gegen den echten Dienst `ultar-mesh` und auf keinem Bildschirm.
- Kein Lauf auf dem Ultar-Kern, dem Beelink, in VMware oder auf dem A30; ARM64 nicht gebaut.
- Verbindung über den echten Konto-Server (bisher nur ein Ersatz-Server nach dessen Regeln).
- Rundruf im echten Netz (bisher Ankündigungen auf einer Maschine).
- `ultar-llm-bus` gegen den echten Modell-Dienst: bisher nur gegen einen Ersatz mit denselben drei
  Anfragen (`/health`, `/v1/models`, `/v1/chat/completions`). Der Modell-Dienst startet nur auf
  Anforderung (`manual = true`, Kernparameter `ultar.start=llm`). Von selbst startet ihn niemand: Der Aufruf
  dafür (`init.control`) dürfte jeden Dienst starten und stoppen – das Recht bekommt die Brücke nicht.
- Das Abbild mit den drei neuen Diensten wurde nicht gebaut und nicht gestartet; geprüft sind die
  Dienstdateien durch die Tests von `ultar-busd`, `ultar-init` und `ultar-logd`.
- Helfer mit einem echten Modell: Die Tests nutzen ein Modell-Skript. Ob ein kleines Modell die
  Antwortform zuverlässig einhält, ist offen; schlägt es fehl, bleibt die kleinste Hilfe.
- Arabische Texte (Tafeln, Dialog, Helfer) hat kein Muttersprachler gegengelesen.
