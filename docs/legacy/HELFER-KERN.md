# Ultar Mind – die Intelligenz des Helfers im Rust-Kern (ohne Ubuntu)

Ziel: Der Helfer soll **ohne den Ubuntu-Bereich** klug sein und die Vorteile nutzen, die nur ein eigenes
System hat. Dafür ist die Intelligenz in vier Schichten geteilt. Jede Schicht läuft für sich; fehlt eine
höhere, bleibt der Helfer trotzdem nützlich.

| Schicht | Was | Wo | Stand |
|---|---|---|---|
| 0 Reflexe | sieht, was auf der Seite steht; bewertet jede Adresse sofort; meldet sich von selbst | in der Oberfläche (`app_buddy.rs`, `ui.rs`) | **fertig** |
| 1 Verstehen ohne Modell | feste Anweisungen in Deutsch/Englisch/Arabisch → Werkzeuge | `mind.rs` | **fertig** |
| 2 Kleines Sprachmodell | freie Fragen, mehrstufige Aufgaben | eigener Dienst `ultar-mind` (Rust) | **geplant** |
| 3 Großes Modell im Netz | nur auf Wunsch, z. B. Claude | Rust-TLS statt `curl` im Ubuntu-Bereich | **geplant** |

## Was nur ein eigenes System kann

1. **Der Helfer liest die Seite, statt sie zu fotografieren.** Die Oberfläche zeichnet jeden Text selbst
   (`Ui::label`, `Ui::para`). Dabei merkt sie sich, was auf der Seite steht. Kein Bildschirmfoto, keine
   Texterkennung, keine Zugriffsrechte wie bei Android – und es kostet praktisch nichts.
   Heute: Web-Adressen. Nächster Schritt: der ganze sichtbare Text als Kontext für Schicht 2
   („fasse diese Nachricht zusammen", „was heißt das?").
2. **Werkzeuge sind Funktionen im selben Prozess.** „Taschenlampe an" ist ein Funktionsaufruf, kein
   Umweg über Dienste. Antwortzeit: unter einem Bild.
3. **Eine echte Box.** Gefährliches (Link verfolgen) läuft in einem eigenen Prozess ohne Rechte
   (`linkcheck.rs`). Später über `ultar-init` mit eigenem Namensraum.
4. **Der Kern verwaltet das Modell.** `ultar-init` kann den Modell-Dienst erst bei der ersten Frage starten,
   auf die schnellen Kerne legen, bei Speichermangel oder ausgeschaltetem Bildschirm beenden.
5. **Ein Schalter für alles.** Datenschutz-Einstellungen gelten für den Helfer wie für jede App.

## Schicht 0 – Reflexe (fertig)

- Jede neue Adresse auf der Seite bekommt sofort ein Schnellurteil (`linkcheck::analyze`, ohne Netz).
- Gefährlich → Figur wird rot, wackelt, zeigt „!" und eine Hinweisblase; Antippen startet die genaue Prüfung in der Box.
- Verhalten der Figur: Augen folgen dem Finger, blinzeln, schauen beim Nachdenken nach oben, sind nachts müde;
  der Mund zeigt die Lage (lächeln / gerade / besorgt).
- Vorschläge im Feld passen zur Seite (in der Kamera „Scan", sonst „Taschenlampe").

## Schicht 1 – Verstehen ohne Modell (fertig)

`mind::understand(text)` liefert einen Werkzeugaufruf oder eine Antwort – sofort, ohne Netz, immer gleich.
Beispiele: „Taschenlampe an", „turn off wifi", „السطوع ٧٠", „Timer 5 Minuten", „Wecker um 7:30",
„Notiz: Milch kaufen", „öffne die Kamera", „Dokument scannen", „wie spät ist es", „Akku?", „geh weg",
„ist bit.ly/abc sicher?". Es sind dieselben Werkzeuge wie beim Agenten (`App::exec`); riskante brauchen
weiter die Bestätigung. Was nicht verstanden wird, geht an Schicht 2 – oder der Helfer sagt ehrlich, dass er es nicht kann.

## Schicht 2 – Sprachmodell im Rust-Kern (geplant)

- **Dienst `ultar-mind`**: eigenes Rust-Programm, statisch gebaut wie alle Ultar-Programme, liegt in `/usr/bin`,
  Modell unter `/data/ultar/mind/model.gguf`. Kein Ubuntu, kein Python, kein `llama-server`.
- **Rechenkern**: `candle` (reines Rust, liest GGUF-Modelle mit 4-Bit-Gewichten). Ausweichweg, falls zu langsam:
  `llama.cpp` als statisch eingebundene Bibliothek hinter derselben Schnittstelle.
- **Schnittstelle**: Unix-Socket `/run/ultar/mind.sock`, gleiches Format wie heute (`/v1/chat/completions`),
  damit `agent.rs` unverändert bleibt – nur die Adresse ändert sich.
- **Antwort nur als Werkzeugaufruf oder Text**: die Ausgabe wird beim Erzeugen auf gültiges JSON mit unseren
  Werkzeugnamen begrenzt. Ein kleines Modell kann dann nichts Ungültiges liefern.
- **Start bei Bedarf** durch `ultar-init`; Ende nach einigen Minuten Ruhe.
- **Modellgröße für das A30** (3,7 GB Speicher, 2 schnelle + 6 langsame Kerne): 0,5–1,5 Milliarden Gewichte,
  4 Bit, 400 MB bis 1 GB. Erwartung: wenige Wörter je Sekunde – deshalb erledigen Schicht 0 und 1 alles Häufige,
  und Schicht 2 bekommt nur, was wirklich Sprache braucht. **Noch nicht gemessen.**
- **Kontext**: Schicht 2 bekommt den sichtbaren Text der Seite und den Gerätezustand mit.

## Schicht 3 – großes Modell im Netz (geplant)

Heute ruft `agent.rs` Claude über `curl` im Ubuntu-Bereich auf. Geplant: TLS direkt in Rust (`rustls`),
damit auch das ohne Ubuntu läuft. Bleibt abschaltbar; nichts geht ohne ausdrückliche Einstellung ins Netz.

## Reihenfolge der nächsten Schritte

1. Sichtbaren Seitentext als Kontext sammeln (klein, in `ui.rs`).
2. `ultar-mind` mit `candle` bauen und am A30 messen (Wörter je Sekunde, Speicher).
3. Ausgabe auf Werkzeug-JSON begrenzen; Start bei Bedarf in `ultar-init`.
4. Schicht 3 auf `rustls` umstellen.
