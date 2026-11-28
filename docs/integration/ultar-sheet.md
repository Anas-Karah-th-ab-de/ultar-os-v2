# ultar-sheet – die Tabellenkalkulation

Crate `apps/ultar-sheet` (lib `ultar_sheet` + bin `ultar-sheet`), Manifest-ID `sheet`. Die Dateiformate
und die Rechenmaschine liegen in `apps/ultar-office` (`sheet.rs`, `calc.rs`, `sheet_edit.rs`,
`sheet_find.rs`, `sheet_refs.rs`, `numfmt.rs`, `xlsx.rs`); die App selbst enthält Oberfläche,
Bearbeitung, Dateien, Wiederherstellung und den Agenten-Zugang.

## Was sie kann
- **Dateien**: `.xlsx` und `.csv` (UTF-8 und Windows-1256; Trennzeichen und Zeilenende werden beim
  Öffnen erkannt und beim Speichern beibehalten), Neu, Speichern, Speichern unter, Frage bei
  ungespeicherten Änderungen, atomares Speichern (Temp-Datei + Umbenennen), Grenze 64 MiB
  („zu groß“), beschädigte Datei = klare Meldung statt Absturz. Beim CSV-Speichern wird gewarnt,
  wenn Formate/Formeln verloren gehen (nur Werte) oder die Kodierung wechselt.
- **Wiederherstellung**: alle 20 s eine Sicherungskopie, solange etwas ungespeichert ist
  (`recover.rs`, Kennung = Hash des Pfads); nach einem Absturz fragt die App beim nächsten Öffnen.
- **Gitter**: virtualisiert (nur sichtbare Zeilen/Spalten werden berechnet und gezeichnet; 100 000
  Zeilen), fixierte Kopfzeile/-spalte, Spaltenbreite und Zeilenhöhe per Ziehen, Auswahl mit Maus,
  Touch und Umschalt+Pfeil, Blätter-Leiste (hinzufügen, umbenennen, löschen mit Rückgängig).
- **Bearbeiten**: Formelleiste, Bearbeiten in der Zelle, Enter/Tab, Ausfüllen nach unten/rechts,
  Kopieren/Ausschneiden/Einfügen (TSV aus anderen Programmen), Rückgängig/Wiederholen (100 Schritte),
  Löschen, Zeilen/Spalten einfügen/löschen (Formelbezüge werden angepasst), Sortieren, Suchen/Ersetzen.
- **Formeln**: SUM, AVERAGE, MIN, MAX, COUNT, COUNTA, IF, AND, OR, NOT, ROUND, ABS, CONCAT und `&`,
  LEN, LEFT, RIGHT, MID, UPPER, LOWER, TODAY, NOW, VLOOKUP, IFERROR, SUMIF, COUNTIF; Bezüge und
  Bereiche auch über Blätter (`Blatt2!A1:B3`); Excel-Fehler (`#DIV/0!`, `#REF!`, `#NAME?`,
  `#VALUE!`, `#N/A`, `#NUM!`, Zirkelbezug); Neuberechnung nur der abhängigen Zellen.
- **Formate**: Zahl, Prozent, Währung, Datum, fett, kursiv, Textfarbe, Füllung, Ausrichtung,
  Umbruch; **RTL-Blatt** mit Spalte A rechts (Gitter, Köpfe, Auswahl und Bildlauf gespiegelt;
  Zahlen bleiben links-nach-rechts gesetzt).
- **Größenklassen**: kompakt, mittel, breit (Werkzeuge bei kompakt in einem unteren Blatt), dunkel
  und hell, Zustände leer, lädt, beschädigt, zu groß. Sprachen ar, en, de (`locales/`).

## Sicherheit (Zelle)
- Die App sieht nur ihre Bereiche unter `/areas/<name>` (`--areas` / `ULTAR_AREAS`); Dateinamen werden
  geprüft (kein `/`, `..`, NUL, Steuerzeichen), Aufruf-URIs (`files://bereich/pfad#sheet=…&cell=…`)
  werden als feindliche Eingabe behandelt (`place.rs`, Tests mit Umgehungsversuchen).
- xlsx wird mit festen Grenzen gelesen (Teil einer Zip-Datei höchstens 256 MiB, höchstens 4 Mio. Zellen); kaputte Dateien
  führen zu `DocError`, nie zu Panik. Kein Netz, kein Hintergrundbetrieb.

## Geräte-Mesh (B4)
Die App meldet über `mesh.set_activity` (`Activity{app, title, uri, file}`) die offene Arbeitsmappe,
das aktive Blatt und die aktive Zelle: `files://<bereich>/<pfad>#sheet=<NAME>&cell=<B7>`. Gemeldet
wird bei Dateiwechsel, Blattwechsel und Zellwechsel (nur wenn sich Datei, Blatt oder Zelle
gegenüber der letzten Meldung geändert haben). Die Gegenseite öffnet dieselbe URI und springt an die Stelle. Dateien außerhalb eines
Bereichs (nur lokal) werden nicht gemeldet.

## Manifest und „Öffnen mit“
`ultar-app.toml`: `[open] schemes = ["files"]`, `multi_window = true`; Rechte `home/documents/
downloads/desktop/removable` lesen+schreiben, `bus = ["mesh.set_activity"]`, kein Netz.
Start: `ultar-sheet [URI]`; Optionen `--compositor --bus --areas --recover --lang --light --touch`.

## Agenten-Aktionen
Im Manifest `[agent]`: `open`, `read_range` ohne Rückfrage; `write_range` und `add_sheet` nur nach
Bestätigung des Nutzers (Dialog mit Bereich/Anzahl bzw. Blattname), jede Änderung ist **ein**
Rückgängig-Schritt. In der Bibliothek als typisierte Schnittstelle (`agent.rs`: `Request`/`Reply`).

## Offen / ehrliche Grenzen
1. **Agenten-Weg**: es gibt noch keinen Bus-Vertrag für App-Aktionen; `agent.rs` ist die typisierte
   Schnittstelle plus Manifest. Der Anschluss an den Bus fehlt (Besitzer: Agenten-Dienst).
2. **Launcher**: Dateibereiche binden (`/areas/<name>`), Standard-App „Tabelle“ für `.xlsx/.csv` in
   Files (`FileKind::default_app`), Bus-Richtlinie für `mesh.set_activity` freischalten.
3. **Kursiv** wird gespeichert und in xlsx geschrieben, aber nicht gezeichnet (keine kursive Schrift).
4. **Rückgängig** deckt Zellen, Formate, Zeilen/Spalten, Sortieren, Blätter sowie Spaltenbreite,
   Zeilenhöhe, Fixierung und RTL-Umschaltung ab; ein ganzes Ziehen an einer Spalten- oder
   Zeilenkante ist ein Schritt.
5. Ausschneiden = Kopieren + sofortiges Leeren (nicht „beim Einfügen verschieben“).
6. Touch: Bereichsauswahl über den umschaltbaren Knopf „Erweitern“ (Tippen wählt, Ziehen scrollt,
   zweites Tippen bearbeitet).
7. Textfarbe und Füllung sind feste Farbfolgen, keine Farbwähler. Blattnamen dürfen kein `'` enthalten.
8. **Dateiwähler**: im System gibt es keinen (kein Wähler/Picker in `apps/*`, `platform/ultar-ui`
   oder `ultar-launcher`; gesucht nach chooser/picker/open-with). „Öffnen“ nimmt daher einen
   Dateinamen aus Dokumente; die App startet sonst mit einer URI (Files über `launcher.resolve`).
   Die App ist über `[open] schemes = ["files"]` Kandidat für die Art `documents`; der Launcher
   unterscheidet dort aber nicht nach Endung (`kind_of_uri`: xlsx/csv, docx, pdf sind alle
   `Documents`, Standard-App `documents`). „Standard für .xlsx/.csv“ braucht eine eigene Art im
   Bus-Vertrag (`ultar-bus` `launcher::Kind`) – nicht Teil dieses Pakets.
9. Die xlsx-Prüfdateien sind `openpyxl-sample.xlsx` (Fremdschreiber) und `excel-like.xlsx` (von Hand
   nach Excel-Aufbau gebaut, `gen_fixtures.py`); keine mit echtem Excel/LibreOffice gespeicherte Datei.
10. Leistung nur im Debug-Build gemessen (Kopfzeile des Tests), kein Lauf auf echtem Gerät.

## Tests
- `cargo test -p ultar-office`: Formeln (`tests/sheet_formulas.rs`), Bearbeiten (`tests/sheet_edit.rs`),
  xlsx hin und zurück mit eigener Datei und beiden Prüfdateien (`tests/xlsx_fixtures.rs`).
- `cargo test -p ultar-sheet`: Modultests (Auswahl, Gitter, Datei, Wiederherstellung, Orte),
  `tests/flow.rs` (17 Abläufe: Öffnen/Speichern, Bearbeiten, Rückgängig, Blätter, Agent),
  `tests/window.rs` + `tests/window_input.rs` (16: Fenster ar/en × dunkel/hell × kompakt/breit,
  Maus, Touch, Wiederherstellung, Zustände, 100 000 Zeilen Bildlauf unter 100 ms je Bild im
  Debug-Build, bestes von drei Durchläufen),
  `tests/locales.rs` (Kataloge ar/en/de vollständig und gleich).
- Bilder: `ULTAR_SHEET_SHOTS=<ordner>` schreibt die Fensterbilder des Tests.
- `cargo test -p ultar-launcher --test app_manifests` prüft das Manifest.

## Änderungen außerhalb der App
Arbeitsbereich-Mitglied `apps/ultar-sheet`; `tools/release/apps.list`, `tools/vm/make-vmware.sh`,
`tools/release/build-a30.sh` (Eintrag `ultar-sheet`); `system/core/ultar-launcher/tests/app_manifests.rs`
(Liste der Manifeste); in `apps/ultar-office`: neue Dateien `calc.rs`, `sheet_refs.rs`, `sheet_edit.rs`,
`sheet_find.rs`, `numfmt.rs`, Änderungen an `sheet.rs`, `xlsx.rs`, `lib.rs`.
