# ultar-doc: die Textverarbeitung

Stand: Paket `word`, Branch `q/word`. Dieses Dokument beschreibt, was die App kann, wie sie
gebaut ist, was beim Speichern verloren gehen kann und was noch offen ist. Alles, was hier
steht, ist durch Tests im Repository belegt, außer es steht ausdrücklich als offen oder
ungeprüft da.

## 1. Überblick

`apps/ultar-doc` ist die Textverarbeitung von Ultar OS v2. Sie öffnet und speichert `.docx`,
öffnet `.txt` (und `.md`), zeigt das Dokument als Seiten (A4 oder Letter mit Rändern) oder
als fließende Spalte (auf dem Handy), bearbeitet gemischten arabischen und lateinischen Text
mit der Schreibrichtung je Absatz und exportiert nach PDF.

Aufbau (Crate `ultar-doc`, Bibliothek plus Binärdatei `ultar-doc`):

| Modul | Aufgabe |
| --- | --- |
| `state` | `Doc`: Dokument, Cursor, Auswahl, Dateipfad, Verlauf, Laden und Speichern |
| `edit`, `format` | Tippen, Löschen, Bewegen; Zeichen- und Absatzformat, Listen, Bilder, eigene Zwischenablage |
| `history` | Rückgängig/Wiederholen auf Basis von Unterschieden (eine Eingabe = ein Schritt) |
| `tables` | Tabellen: Zeilen und Spalten einfügen und entfernen |
| `layout` | Zeilenumbruch, Bidi je Absatz, Seitenumbruch, Tabellen, Bilder; Absatz-Cache nach Inhalt |
| `paint` | Zeichnen: weißes Papier, Text, Auswahl, Fundstellen, Cursor, Tabellenraster, Bilder |
| `commands`, `keys`, `pointer` | Werkzeugleiste (Layoutklassen), Tastatur, Maus und Touch |
| `dialogs`, `files`, `place` | Fragen (Speichern, Wiederherstellen, Warnung, Suchen, Farbe), Dateinamen |
| `search` | Suchen und Ersetzen mit arabischer Faltung |
| `recovery` | Kopie für den Absturz (Autospeichern) |
| `export` | PDF-Export über `ultar_office::pdfout` |
| `action` | Aktionen für den Assistenten (siehe 7) |
| `service` | Meldung des offenen Dokuments an das Mesh (siehe 8) |

Das Dokumentmodell, der docx-Leser und -Schreiber und der PDF-Schreiber liegen in
`apps/ultar-office/src` (`doc.rs`, `doc/edit.rs`, `docx.rs`, `docx/write.rs`, `docx/lossy.rs`,
`pdfout.rs`).

## 2. Bedienung

### Tastatur

| Taste | Wirkung |
| --- | --- |
| Strg+S / Strg+N | Speichern / neues Dokument |
| Strg+Z / Strg+Y | Rückgängig / Wiederholen |
| Strg+X / C / V | Ausschneiden / Kopieren / Einfügen |
| Strg+A | Alles auswählen |
| Strg+B / I / U | Fett / Kursiv / Unterstrichen |
| Strg+F | Suchen und Ersetzen |
| Strg+E / Strg+J | Zentrieren / Blocksatz |
| Strg++ / Strg+- | Zoom |
| Strg+Enter | Seitenumbruch |
| Pfeile | physisch: in einer RTL-Zeile bewegt der linke Pfeil den Cursor nach links |
| Strg+Pfeil | Wortweise |
| Pos1 / Ende | Zum Anfang / Ende der sichtbaren Zeile |
| Bild auf/ab | Seitenweise |
| Tab in einer Liste | Einrücken |

### Maus und Touch

- Klick setzt den Cursor, Ziehen wählt aus, Doppelklick wählt ein Wort, Dreifachklick den Absatz.
- Mausrad scrollt, Umschalt+Rad scrollt seitwärts, Strg+Rad zoomt.
- Touch: Tippen setzt den Cursor, Doppeltippen wählt ein Wort, Ziehen scrollt, langes Halten
  (450 ms) wählt ein Wort und beginnt eine Auswahl.

### Layoutklassen

`compact` (unter 480 px, Handy), `medium` (unter 800 px), `wide`. Auf `compact` zeigt die
Leiste nur die wichtigsten Befehle und einen Knopf "Mehr"; die Ansicht wird automatisch zur
fließenden Spalte, solange der Nutzer nichts anderes gewählt hat. Die Leiste spiegelt sich im
Arabischen (der erste Knopf sitzt rechts). Das Papier bleibt in beiden Themen weiß (es ist
eine Dokumentkonstante, keine Themenfarbe).

## 3. Formate und Verluste

- `.docx` lesen und schreiben (Eigener Round Trip und ein Fixture in
  `apps/ultar-office/tests/fixtures`). Geschrieben wird atomar über eine `.part`-Datei.
- `.txt`, `.md`: öffnen. Speichern geschieht immer als `.docx` unter neuem Namen.
- `.odt`: nicht unterstützt. Die App sagt das beim Öffnen ("wird nicht unterstützt"), statt
  zu raten.

Das Modell kennt Absätze mit Läufen (fett, kursiv, unterstrichen, durchgestrichen, Größe,
Farbe, Hervorhebung), Überschriften 1 bis 3, Ausrichtung, Richtung je Absatz, Zeilenabstand,
Einzug, Listen, Seitenumbruch, einfache Tabellen (Text je Zelle) und Bilder (PNG, JPEG).

Alles andere geht beim Speichern verloren. Beim Öffnen prüft `lossy_features` die Datei und
nennt die Merkmale in einer Warnung (Kopf- und Fußzeilen, Fußnoten, Kommentare, verfolgte
Änderungen, Textfelder, Felder, Links, verbundene Zellen, verschachtelte Tabellen,
Tabellenformat, schwebende Objekte, Diagramme, eingebettete Objekte, mehrere Abschnitte,
Spalten, Listenebenen). Vor dem ersten Überschreiben legt die App eine Kopie
`<name>.original.docx` neben die Datei.

Das Fixture `word-style.docx` ist von Hand zusammengesetzt
(`apps/ultar-office/tests/fixtures/make_word_style.py`) und enthält Word-typische Elemente.
Es wurde NICHT von Word exportiert, weil auf dem Entwicklungsrechner weder Word noch
LibreOffice vorhanden ist. Ein echter Austausch mit Word ist deshalb nicht belegt.

## 4. Autospeichern und Wiederherstellung

Alle 20 Sekunden schreibt die App bei Änderungen eine versteckte Kopie
`.<name>.autosave.docx` neben die Datei (bei einem unbenannten Dokument nach
`documents/.untitled.autosave.docx`). Beim nächsten Start bietet sie die Kopie an, wenn sie
neuer ist als die Datei. Beim Schließen mit ungespeicherten Änderungen fragt die App (Speichern,
Verwerfen, Abbrechen). Nach dem Speichern verschwindet die Kopie.

## 5. PDF-Export

`ultar_office::pdfout` ist ein kleiner PDF-Schreiber ohne fremde PDF-Bibliothek (nur
`miniz_oxide` für Flate): ganze TrueType-Schriften als CID Type0/Identity-H mit ToUnicode,
Glyphen einzeln positioniert (so bleibt die Bidi-Reihenfolge des Layouts erhalten), PNG als
RGB-Bild, JPEG über DCTDecode, klassische xref-Tabelle. Kursiv wird als Scherung der Glyphen
geschrieben, weil es keine Kursivschrift gibt.

Belegt durch Tests: `ultar-office` (`pdfout::tests`, sechs Tests) und `ultar-doc`
(`export::tests`): Der eigene Leser (hayro) öffnet die Datei, zählt die Seiten, rendert sie
und extrahiert Text, auch arabische Buchstaben; ein Bild wird gerendert.

Grenzen:
- Schriften werden ganz eingebettet (keine Teilmenge), die Datei ist größer als nötig.
- Die Reihenfolge des extrahierten Textes folgt bei RTL der Anzeigereihenfolge.
- Die Schriftbytes liegen doppelt im Programm (`include_bytes!` in `export.rs`), weil
  `FontId::bytes()` nicht öffentlich ist.
- Keine Kopf- und Fußzeilen, keine Links, keine Lesezeichen.

## 6. Eigener Renderer

Die App benutzt `platform/ultar-textview` nicht. Textview zeichnet nur einfache Zeilen; für
Absätze mit gemischter Richtung, Läufe, Auswahl über Seiten und Tabellen reicht das nicht.
`layout` und `paint` setzen deshalb direkt auf `ultar-ui::text` (`TextEngine`) und
`unicode-bidi` auf. Es gab keine Änderung an `platform/ultar-textview`.

Weitere Abhängigkeiten über den Arbeitsbereich hinaus: `unicode-bidi` (Reihenfolge),
`png` (Bilder lesen), `miniz_oxide` (Flate für PDF und Bilder).

## 7. Aktionen für den Assistenten

Das Manifest `ultar-app.toml` nennt `open` und `read_text` (frei) sowie `insert_text` und
`replace` (mit Bestätigung). Die Aktionen sind in `action.rs` umgesetzt:

- `open`: öffnet eine `files://`-Adresse; wird abgelehnt, solange es ungespeicherte
  Änderungen gibt.
- `read_text`: liefert den Text (mit Obergrenze).
- `insert_text`, `replace`: öffnen zuerst eine Frage "Der Assistent möchte ..."; erst nach
  "Ja" ändert sich das Dokument, als genau ein Rückgängig-Schritt. Bei "Nein" bleibt alles
  unverändert und der Assistent bekommt "declined".

Offen: Es gibt auf der Plattform noch keinen Bus-Kanal für App-Aktionen. Die Aktionen laufen
deshalb nur im Prozess (`App::agent`, `App::take_agent_results`) und sind durch Tests belegt
(`tests/flow.rs`); ein Test prüft, dass die Namen im Manifest stehen. Die Verdrahtung mit dem
Assistenten fehlt, bis der Kanal existiert.

## 8. Mesh

`Mesh::report(APP_ID, Titel, Adresse, None)` meldet das offene Dokument. Die Adresse hat die
Form `files://<bereich>/<pfad>#b<block>`; `b<block>` ist die Nummer des Blocks am Cursor
("B4": vierter Block). Gemeldet wird nur bei Änderung der Adresse.

## 9. Tests

| Datei | Inhalt |
| --- | --- |
| `ultar-office` Bibliothek | Modell, docx (Round Trip), Markdown, PDF-Schreiber |
| `ultar-office/tests/docx_fixture.rs` | Fixture im Word-Dialekt (von Hand), Seitenumbruch, Listen, Tabelle |
| `ultar-doc` Bibliothek | Bearbeiten, Verlauf, Layout (gemischt arabisch/lateinisch), Suchen, Wiederherstellung, Befehle, PDF |
| `tests/flow.rs` | Tippen, Formatieren, Speichern unter, Schließen mit Frage, Zwischenablage, Suchen/Ersetzen, Autospeichern, Assistent, txt/odt/pdf |
| `tests/input.rs` | Tastatur (RTL-Pfeile, Wörter, Auf/Ab), Maus, Touch, Tabelle und Bild |
| `tests/window.rs` | Fenster ohne Bildschirm: ar/en, dunkel/hell, compact/medium/wide, alle Zustände |
| `tests/latency.rs` | Tippen in 112 Seiten gegen das Budget |

Mit `ULTAR_DOC_SHOTS=<ordner>` schreibt `tests/window.rs` die Bilder als PNG.

Laufzeit (WSL, `CARGO_BUILD_JOBS=4`):

    cargo test -p ultar-doc -p ultar-office
    cargo test --release -p ultar-doc --test latency -- --nocapture

### Antwortzeit

Das Budget (`docs/APP-RULES.md` Abschnitt 8) liegt bei 50 ms (Handy) und 30 ms (PC) pro
Tastendruck. Gemessen mit einem Dokument von 112 Seiten (gemischt, mit Überschriften), im
Release-Build, Taste für Taste (Bearbeiten, Layout des geänderten Absatzes, Neuzeichnen):

- Tippen am Anfang, in der Mitte, am Ende: Median 3,3 bis 7,5 ms, schlechtester Wert 6,0 bis
  10,8 ms (zwei Läufe; der zweite lief nach Umbauten und etwas langsamer).
- Erstes Layout und Zeichnen von 112 Seiten: etwa 6 ms; Enter/Rückschritt: 3,7 bis 8,2 ms;
  Bild auf/ab: 3,2 bis 5,2 ms.

Im Debug-Build ist es rund 20-mal langsamer (Median etwa 125 ms); dort prüft der Test nur eine
lose Obergrenze von 3 s. Die echte Prüfung des Budgets läuft nur im Release-Build. Die
Messung stammt vom Entwicklungsrechner (WSL), nicht vom Handy.

## 10. Offene Punkte

- Kein Test auf echtem Gerät (Handy, Compositor). Alle Prüfungen laufen ohne Bildschirm; die
  Bilder von `tests/window.rs` wurden geschrieben, aber noch von niemandem angesehen; die
  Tests prüfen nur Inhalt, weißes Papier, Spiegelung und Zustände.
- Mit Word oder LibreOffice wurde nichts ausgetauscht (siehe 3).
- JPEG wird in die PDF eingebettet, auf dem Bildschirm aber nur als Rahmen gezeigt (es gibt
  keinen JPEG-Decoder in der Abhängigkeitsliste).
- Die Pfeiltasten folgen der Richtung des Absatzes, nicht jedem Lauf einer gemischten Zeile
  (kein "visuelles" Wandern über Richtungswechsel innerhalb einer Zeile).
- Der Bus-Kanal für Assistent-Aktionen fehlt (siehe 7).
- ODT wird nicht gelesen.
- PDF: ganze Schriften, keine Links (siehe 5).
