# Ultar OS – Desktop, Installation, Komponenten: Entwurf und Arbeitsablauf

KM Code Craft · Stand 22.11.2026 · gehört zu [PLAN.md](PLAN.md) (Phasen P5, P6, P8, P9) · Maßstab: Ubuntu 26.04 LTS Desktop

> Diese Unterlage legt fest, **was** gebaut wird – jede Fläche, jedes Symbol, jede Einstellung – und **wie** gearbeitet
> wird, damit nichts Halbes entsteht. Gebaut wird erst, was hier (oder in einer Folge-Unterlage unter `docs/ui/`)
> vollständig beschrieben und von dir freigegeben ist.

---

## 1. Was aus dem alten System NICHT übernommen wird

| Alter Fehler | Neue Regel |
|---|---|
| Handy-Oberfläche in Fenster gesteckt (`app_desktop.rs`): Apps sahen am PC aus wie ein Handy im Rahmen | Desktop ist ein eigener Entwurf; jede App hat drei Layout-Klassen (kompakt · mittel · weit) und nutzt am PC Seitenleiste, Menüs, Tastatur, Rechtsklick |
| Zwei Fenstersysteme, zwei Verhalten | ein Compositor, ein Fensterverhalten |
| Funktionen entstanden einzeln „nach Zuruf“ → Dopplungen (`app_more`, `app_extra`, drei Rechner, zwei Agenten-Oberflächen) | jede Funktion hat genau einen Ort (Abschnitt 9: Verzeichnis); vor dem Bauen wird geprüft, ob es sie schon gibt |
| Symbole mehrfach belegt („Ton“ und „Anzeige“ dasselbe Symbol) oder ohne klare Bedeutung | ein Symbol = eine Bedeutung (Abschnitt 4) |
| Einstellungen als lange Liste, auch ohne passende Hardware | Bereiche erscheinen nur, wenn die Fähigkeit da ist; jede Einstellung hat einen Zweck und eine Wirkung, die man sofort sieht |
| Knöpfe ohne Wirkung („Fehler bis nachgezogen“), Attrappen im Produkt | kein sichtbares Element ohne fertige Funktion; Unfertiges ist unsichtbar |
| Alles in einem Abbild (Telegram, Spotify, Büro, Verbund …) | kleines Grundsystem + wählbare Komponenten (Abschnitt 7) |
| Texte deutsch gedacht, dann übersetzt; abgeschnittene arabische Texte | Arabisch zuerst entworfen; Layout in Anfang/Ende |
| Farben/Abstände je Bildschirm frei gewählt | feste Gestaltungswerte (Abschnitt 3), keine Zahl im App-Code |
| Zustände vergessen (leer, lädt, Fehler, keine Berechtigung, offline) | jede Fläche hat alle Zustände beschrieben, bevor sie gebaut wird |
| Tests erst am Gerät | jede Fläche hat Bildtests (3 Sprachen × 3 Größen × hell/dunkel) und Bedientests |

---

## 2. Arbeitsablauf – so behalten wir den Überblick

### 2.1 Das Verzeichnis (eine Quelle der Wahrheit)
`docs/ui/inventory.toml` führt **jede** Fläche, jedes Bedienelement mit Funktion und jede Einstellung mit einer festen
Kennung. Nichts wird gebaut, was dort nicht steht; nichts steht dort ohne Zweck.

```toml
[[surface]]
id = "settings.display"            # Kennung = i18n-Präfix = Testname = Ordner der Unterlage
kind = "panel"                     # shell | panel | app | dialog | installer-step
needs = ["display"]                # Fähigkeiten, ohne die es nicht erscheint
ubuntu = "Settings > Displays"     # Gegenstück bei Ubuntu 26.04 (oder "ultar-only")
status = "spec"                    # idea → spec → approved → built → verified
[[surface.item]]
id = "settings.display.scale"
type = "choice"                    # toggle | choice | slider | action | text | list | info
purpose = "Größe von Text und Elementen an Bildschirm und Sehkraft anpassen"
default = "auto (aus DPI)"         # woher der Startwert kommt
effect = "sofort, mit Rücknahme nach 15 s ohne Bestätigung"
store = "settingsd: display.<id>.scale"
```

`ultar-check` bekommt dafür zwei neue Regeln: **(a)** jeder i18n-Schlüssel und jede `settingsd`-Einstellung im Code
muss im Verzeichnis stehen, **(b)** jede Zeile mit `status = "verified"` braucht vorhandene Tests. So kann keine
Funktion „nebenbei“ entstehen und keine vergessen werden.

### 2.2 Sieben Schritte je Fläche (keiner wird übersprungen)

| # | Schritt | Ergebnis | Tor |
|---|---|---|---|
| 1 | **Zweck** | Wer braucht das wofür? Gegenstück bei Ubuntu? Schon vorhanden? | Eintrag im Verzeichnis (`idea`) |
| 2 | **Beschreibung** | `docs/ui/<id>.md`: Aufbau, jedes Element mit Funktion, alle Zustände (normal, leer, lädt, Fehler, gesperrt, offline, keine Hardware), Tastatur/Maus/Touch/Sprachausgabe, Texte arabisch zuerst | `spec` |
| 3 | **Entwurf** | Bild der Fläche in kompakt/mittel/weit, hell/dunkel, arabisch + englisch | **deine Freigabe** → `approved` |
| 4 | **Schnittstelle** | Bus-Nachrichten und Einstellungs-Schlüssel festgelegt (Dienst zuerst, Oberfläche danach) | Vertragstest |
| 5 | **Bau** | Dienst-Logik mit Tests, dann Oberfläche aus den Bausteinen von `ultar-ui` | `built` |
| 6 | **Prüfung** | Bildtests, Bedientests (jeder Weg durch die Fläche), Barrierefreiheit, Leistungsbudget, Lauf auf dem Ultar-Kern | CI grün |
| 7 | **Abnahme** | Durchlauf nur in Arabisch am Testrechner; Vergleich mit dem Ubuntu-Gegenstück: fehlt etwas Nötiges? | `verified` |

### 2.2b Rollen – jede Fläche geht durch mehrere Hände

Gearbeitet wird immer mit mehreren Agenten gleichzeitig, jeder mit **einer** Rolle und eigener Aufgabe. Keine Rolle
prüft ihre eigene Arbeit. Eine Fläche gilt erst als fertig, wenn alle Rollen unterschrieben haben.

| Rolle | Aufgabe | Liefert |
|---|---|---|
| **Produkt** | Zweck, Vollständigkeit gegen Ubuntu und gegen das alte System, keine Dopplung | Eintrag im Verzeichnis, Liste der Muss-Funktionen |
| **Gestaltung** | Entwurfsbild in allen Größen/Sprachen/Farbwelten; nur Gestaltungswerte und Bausteine | freigegebenes Bild = **Referenz** |
| **Arabisch** | Wortwahl, Richtung, Begriffe einheitlich (Glossar `i18n/glossary.toml`) | geprüfte Texte |
| **Bau: Dienst** | Logik, Bus-Schnittstelle, Tests | Dienst mit Vertragstests |
| **Bau: Oberfläche** | setzt das Bild 1:1 um – ohne eigene Gestaltungsentscheidungen | Fläche aus `ultar-ui` |
| **Prüfer Bedienung** | jeder Klick, jede Taste, jede Geste, jeder Zustand; versucht es kaputt zu machen | Klickliste 100 % grün |
| **Prüfer Bild** | vergleicht Umsetzung mit der Referenz (Bild für Bild) | Abweichungsliste = leer |
| **Prüfer Zugang** | Tastatur allein, Bildschirmleser, Kontrast, große Schrift | Bericht |
| **Abnahme** | unabhängiger Durchlauf am Testrechner in Arabisch; hat die Fläche vorher nie gesehen | „verified“ |

### 2.2c Umsetzung darf nicht anders aussehen als der Entwurf – dreifache Sicherung

1. **Eine Quelle für Werte:** Farben, Abstände, Radien, Schriften stehen einmal in `design/tokens.toml`. Entwurfsbilder
   **und** `ultar-ui` werden daraus erzeugt – es gibt keine zweite Stelle, an der ein Wert stehen kann.
2. **Bildvergleich in der CI:** jede freigegebene Fläche liegt als Referenzbild im Repo (`docs/ui/ref/<id>/<größe>-<sprache>-<farbwelt>.png`).
   Die echte Oberfläche wird im Test in derselben Größe gezeichnet (`--shot`) und Bildpunkt für Bildpunkt verglichen
   (Toleranz nur für Kantenglättung der Schrift). Abweichung = Bau scheitert. Ändern darf ein Referenzbild nur, wer
   ein neu freigegebenes Entwurfsbild vorlegt.
3. **Maßprüfung:** zusätzlich prüft ein Test die Lage und Größe jedes Elements gegen die Beschreibung (Abstände im
   Raster, Mindestgrößen, Reihenfolge in Arabisch gespiegelt).
4. **Zwei unabhängige Prüfer** („Prüfer Bild“ und „Abnahme“) sehen die Fläche am echten Bildschirm – nicht derselbe
   Agent, der gebaut hat.

### 2.2d Jeder Klick funktioniert – Klickliste

Für jede Fläche erzeugt die CI aus dem Verzeichnis die **Klickliste**: jedes bedienbare Element × jede Bedienart
(Klick, Rechtsklick, Doppelklick, Ziehen, Tastatur, Touch) × jeder Zustand. Jede Zeile braucht einen Test, der die
**Wirkung** prüft (nicht nur „stürzt nicht ab“): Zustand im Dienst geändert, richtige Fläche geöffnet, Rückmeldung
sichtbar, nach Neustart noch da. Ein Element ohne Zeile oder eine Zeile ohne Test lässt den Bau scheitern.
Dazu ein Zufallstest je Fläche (10 000 zufällige Eingaben): kein Absturz, kein Hänger, kein Element ohne Reaktion.

### 2.3 Reihenfolge der Arbeit
1. Gestaltungswerte + Bausteine (`ultar-ui`) – Abschnitt 3/4.
2. Compositor + Desktop-Hülle – Abschnitt 5.
3. Einstellungen (weil sie jede Dienst-Schnittstelle erzwingen) – Abschnitt 8.
4. Installer + Ersteinrichtung – Abschnitt 6.
5. Grund-Apps – Abschnitt 9, dann Ultar Store und Komponenten – Abschnitt 7.

Fortschritt steht in `docs/ui/README.md` als Tabelle (von der CI aus dem Verzeichnis erzeugt): je Fläche der Status.

---

## 3. Gestaltung – ein System, überall gleich

**Leitidee:** ruhig, klar, schnell. Inhalt vor Schmuck. Gold der Marke nur für *die eine* Hauptaktion und für „an“.

| Bereich | Festlegung |
|---|---|
| Raster | Grundmaß 4; Abstände 4 · 8 · 12 · 16 · 24 · 32 · 48. Kein anderer Wert im Code. |
| Größenklassen | kompakt < 600 · mittel 600–1023 · weit ≥ 1024 (logische Punkte). Jede Fläche beschreibt alle drei. |
| Schrift | Arabisch: Noto Naskh Arabic (Lesetext), Noto Kufi Arabic (Titel); Latein: Noto Sans; Code: eine feste Breite mit arabischer Unterstützung. Stufen: 12 · 14 · 16 · 20 · 24 · 32. Zeilenhöhe für Arabisch 1,6. |
| Farben | Rollen statt Werte: Fläche, Fläche-erhöht, Text, Text-leise, Linie, Akzent (Gold), Info (Blau), Erfolg (Grün), Warnung (Bernstein), Gefahr (Rot). Hell + Dunkel + hoher Kontrast. Kontrast mindestens 4,5:1. Farbe trägt nie allein die Bedeutung (immer Symbol oder Text dazu). |
| Form | Ecken 8 (Elemente), 12 (Karten/Fenster), rund (Schalter, Profilbild). Eine Schattenstufe für Schwebendes. |
| Bewegung | 120 ms (Rückmeldung), 200 ms (Ein-/Ausblenden), 300 ms (Übersicht/Arbeitsflächen). „Bewegung verringern“ schaltet alles auf Überblenden. Nie Bewegung ohne Bedeutung. |
| Richtung | Anfang/Ende statt links/rechts; Symbole mit Richtung (Pfeile, Zurück, Fortschritt) werden gespiegelt, Symbole ohne Richtung (Uhr, Haken, Medien-Wiedergabe) nicht. |
| Ziffern | immer 0–9. |
| Ziele | Maus: Elemente mind. 32 hoch · Touch: mind. 44 · Fokusrahmen immer sichtbar bei Tastatur. |
| Fokusrahmen | 2 Punkte breit, 2 Punkte Abstand, Farbe = Rolle **Info (Blau)** (`state.focus_ring = "info"`); Werkzeugkasten und Entwurfsbilder lesen dieselbe Marke. **Begründung (Runde 3):** Gegen den Hintergrund erreicht Info 4,3–9,0 : 1 (verlangt 3 : 1). Gegen das Gold, das er oft umschließt, erreicht *keine* Rolle 3 : 1 (Info 1,03 dunkel / 2,52 hell; Akzent-Text 1,24 / 2,58), Akzent-Text hat also dort keinen Vorteil. Ein goldener Rahmen um einen goldenen Hauptknopf liest sich aber als dessen Rand, nicht als Fokus; Blau unterscheidet sich im Farbton und ist an jeder Stelle als „hier ist der Fokus“ erkennbar. Die 2 Punkte Abstand trennen Rahmen und Gold sichtbar. |
| Zustände | aus `[state]` der Marken: schwebend/gedrückt = 10 % / 20 % der Vordergrundfarbe über gefüllten Flächen (neutral: Fläche „schwebend“ / Linie), deaktiviert = ganzer Baustein mit 0,40 Deckkraft, Abdunkeln hinter Dialog und Blatt = Grundfarbe mit 0,60 (dunkel) / 0,40 (hell). Linienbreite `size.border`, Zeilen mind. `size.row_min`, Kästchen/Kreis `size.box`. Akzent (Gold) nur als Füllung hinter Text „auf Akzent“; Grafiken, Ränder und Markierungen („an“, „gewählt“, „wie viel“) in Akzent-Text (≥ 3 : 1 in beiden Themen). |
| Hinweis (Toast) | **Entscheidung Runde 5:** Ein Hinweis ohne Aktion schließt nach `motion.toast` (4 s). Ein Hinweis **mit** Aktion bleibt `motion.toast_action` (10 s); die Zeit steht still, solange der Mauszeiger darüber ist oder einer seiner Knöpfe den Tastaturfokus hat (Tab nach dem letzten Element erreicht ihn) – WCAG 2.2.1. Esc und der Schließen-Knopf schließen ihn jederzeit sofort. Seine Knöpfe wirken beim Loslassen wie jeder Knopf (Wegziehen nimmt den Druck zurück). |
| Markierungen | Haken für „gewählt/an“ in Menüs und Auswahllisten: in einer Spalte am Anfang (16, Akzent-Text). Aktueller Eintrag einer Seitenleiste: Balken 4 × 16 am Ende. Gewählte Zeile einer Liste: Balken am Anfang. Regel (Runde 3): wo keine Marke gilt, ist das freigegebene Entwurfsbild maßgeblich – Werkzeugkasten und Bilder folgen derselben Festlegung. |

**Bausteine in `ultar-ui`** (Apps bauen nur daraus): Knopf (Haupt · normal · leise · Gefahr), Symbolknopf, Schalter, Auswahl,
Schieber, Eingabefeld (mit Fehlertext), Suchfeld, Liste/Zeile, Karte, Seitenleiste, Kopfleiste, Reiter, Menü, Kontextmenü,
Dialog, Blatt (kompakt: von unten), Hinweisleiste, Fortschritt, Leerzustand, Tabelle, Baum, Tooltip, Abzeichen.
Jeder Baustein kennt: normal, schwebend, gedrückt, fokussiert, deaktiviert, lädt, Fehler.
Ausnahmen: Optionsgruppe und Reiter haben weder „lädt“ noch „Fehler“ – die Seite bzw. das Formular um sie zeigt beides.

---

## 4. Symbole – ein Symbol, eine Bedeutung

Ein Satz, eine Strichstärke (1,5 bei 24), zwei Größen (16, 24), als Umriss; gefüllt nur für „aktiv/an“.
Jedes Symbol hat genau eine Bedeutung im ganzen System; jede Bedeutung genau ein Symbol. Neue Symbole nur über das
Verzeichnis (`docs/ui/icons.toml`: Name, Bedeutung, Spiegeln ja/nein, wo benutzt).

| Gruppe | Symbol → Bedeutung |
|---|---|
| Navigation | Pfeil-Anfang → zurück · Pfeil-Ende → weiter/öffnen · Haus → Startordner · Raster (9 Punkte) → alle Apps · Lupe → suchen · drei Striche → Hauptmenü der App · drei Punkte → weitere Aktionen zu *diesem* Element |
| Fenster | Strich → verkleinern · Quadrat → maximieren · zwei Quadrate → wiederherstellen · Kreuz → schließen (nur Fenster/Dialoge, nie „löschen“) |
| Aktionen | Plus → neu/hinzufügen · Stift → bearbeiten · Papierkorb → löschen (immer mit Rücknahme oder Rückfrage) · Diskette entfällt → „Speichern“ als Text · zwei Blätter → kopieren · Klemmbrett → einfügen · Schere → ausschneiden · Pfeil-aus-Kasten → teilen/exportieren · Pfeil-in-Kasten → importieren · Pfeil-nach-unten-auf-Linie → herunterladen · Kreispfeil → neu laden · Pfeil-zurück-gebogen → rückgängig |
| Zustand | Haken → erledigt/gewählt · Kreis-i → Hinweis · Dreieck-! → Warnung · Achteck-! → Fehler · Schloss zu/offen → gesperrt/entsperrt · Schild → Sicherheit · Auge/Auge-durchgestrichen → sichtbar/verborgen · Sanduhr entfällt → Fortschrittsring |
| Verbindung | Fächer (0–4 Stufen) → WLAN-Stärke · Kabelstecker → Kabelnetz · Balken (0–4) → Mobilfunk · Flugzeug → Flugmodus · B-Rune → Bluetooth · Schild-mit-Schlüssel → VPN · zwei verbundene Kreise → Verbund · Wolke → Ultar-Konto/Cloud |
| Gerät | Lautsprecher (0–3 Wellen, durchgestrichen = stumm) → Ton · Mikrofon → Aufnahme · Sonne → Helligkeit · Mond → Nachtlicht/Dunkel · Akku (Füllstand, Blitz = lädt) → Energie · Bildschirm → Anzeige · Tastatur → Tastatur · Maus → Maus/Touchpad · Drucker → Drucken · Chip → Treiber & Hardware |
| System | Zahnrad → Einstellungen (nur das) · Ein/Aus-Ring → Ausschalten-Menü · Person → Konto · Glocke (durchgestrichen = Nicht stören) → Benachrichtigungen · Kalenderblatt → Datum · Uhr → Zeit · Einkaufstasche → Ultar Store · Ordner → Dateien · Stern → Favorit · Stecknadel → anheften · Figur im Kreis → Barrierefreiheit · Sprechblase-mit-Funke → Helfer |

App-Symbole: eigene Familie (farbig, gleiche Grundform, gleiche Lichtrichtung), jedes eindeutig ohne Text erkennbar;
Datei-Symbole nach Art (Text, Tabelle, Bild, Ton, Film, Archiv, Programm, unbekannt).

---

## 5. Desktop-Hülle (`shell-desktop`) – neu gedacht

### 5.1 Aufbau des Bildschirms

> **Standard ist seit 22.11.2026 die eine Leiste unten** (Abschnitt 11). Alle Funktionen der folgenden Tabelle gelten
> unverändert; „Leiste oben“ und „Dock“ liegen im Standard gemeinsam in der unteren Leiste. Das App-Menü der aktiven
> App entfällt dort (seine Einträge stehen im Rechtsklick-Menü des App-Symbols).

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [Übersicht]  [App-Menü der aktiven App]        So 22 Nov  16:52        [Status] │  Leiste oben (32)
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│                         Arbeitsfläche mit Fenstern                           │
│                                                                              │
│              ┌───────────────────────────────────────────┐                   │
│              │ [Raster] │ angeheftete + laufende Apps │ [Papierkorb] │       │  Dock unten (56)
│              └───────────────────────────────────────────┘                   │
└──────────────────────────────────────────────────────────────────────────────┘
```
Die Zeichnung zeigt die Latein-Richtung; in Arabisch ist alles gespiegelt (Übersicht am Anfang = rechts).

| Element | Funktion (vollständig) |
|---|---|
| **Übersicht-Knopf** (Markenzeichen, am Anfang der Leiste) | Klick oder Taste „Super“: Übersicht. Kein Menü, keine zweite Bedeutung. |
| **App-Menü** | Name der aktiven App; öffnet: Neues Fenster, Über, Einstellungen der App, Beenden. Leer, wenn keine App aktiv. |
| **Datum & Zeit** (Mitte) | Klick: Blatt mit Kalender (Monat, Hijri-Datum daneben), heutigen Terminen, Benachrichtigungsliste, Schalter „Nicht stören“, „Alle löschen“. |
| **Status** (am Ende) | Symbole nur, wenn sie etwas sagen: Netz · Ton · Akku (nur mit Akku) · Bluetooth (nur wenn an) · VPN (nur wenn an) · Verbund (nur wenn installiert und an) · Tastaturbelegung (nur bei >1) · Aufnahme läuft (Kamera/Mikrofon/Bildschirm – immer sichtbar, nicht abschaltbar). Klick: **Schnelleinstellungen**. |
| **Schnelleinstellungen** | Kacheln mit Zustand: WLAN (Netz wählen), Kabel, Bluetooth (Gerät wählen), Mobilfunk, Flugmodus, VPN, Nachtlicht, Dunkel, Nicht stören, Energiemodus, Verbund, Tastaturbeleuchtung – jede nur bei vorhandener Fähigkeit. Schieber: Lautstärke (mit Ausgabegerät-Wahl), Mikrofon (nur wenn in Benutzung), Helligkeit. Unten: Akku-Stand → Energie, Bildschirmfoto, Einstellungen, Sperren, Ein/Aus-Menü (Bereitschaft, Neustart, Ausschalten, Abmelden, Benutzer wechseln). |
| **Dock** | Anfang: Raster-Knopf (alle Apps). Mitte: angeheftete Apps, danach laufende. Punkt unter dem Symbol = läuft (mehrere Punkte = mehrere Fenster). Klick: starten bzw. Fenster holen; bei mehreren Fenstern Vorschau zur Wahl. Mittelklick: neues Fenster. Rechtsklick: Fensterliste, Neues Fenster, App-eigene Schnellaktionen, Anheften/Lösen, Details im Store, Beenden. Ziehen: Reihenfolge; Datei auf Symbol ziehen: damit öffnen. Abzeichen: Zahl ungelesener Meldungen, Fortschritt (Kopieren/Download). Ende: eingehängte Datenträger, Papierkorb. Einstellbar: Lage (unten · Anfang · Ende), Größe, automatisch ausblenden, auf allen Bildschirmen. |
| **Übersicht** | Oben Suchfeld (sofort tippen). Mitte: Fenster der aktuellen Arbeitsfläche als Vorschau (Klick holt, Kreuz schließt, Ziehen verschiebt auf Arbeitsfläche/Bildschirm). Darüber: Arbeitsflächen-Streifen (dynamisch: immer eine leere am Ende). |
| **Suche** | Ein Feld für alles, Ergebnisse gruppiert: Apps · Einstellungen (springt zur Zeile) · Dateien · Rechnen/Einheiten · Zeichen · Store (nicht installierte Apps) · Befehle („ausschalten“). Arabische Suche ohne Rücksicht auf Vokalzeichen/Schreibvarianten. Quellen einzeln abschaltbar. |
| **App-Raster** | alle Apps alphabetisch (arabische Reihenfolge), Ordner per Ziehen, Seiten; Rechtsklick wie im Dock + Deinstallieren. |
| **Arbeitsfläche (Hintergrund)** | Symbole (abschaltbar, Größe und Startecke aus `desktop.icons.*`): Persönlicher Ordner, eingehängte Laufwerke, App-Verknüpfungen der Person, Dateien/Ordner des Schreibtisch-Ordners. Klick wählt, Doppelklick öffnet. Rechtsklick auf eine Verknüpfung: Öffnen, Umbenennen, An Leiste anheften, Vom Schreibtisch entfernen, Eigenschaften; auf Datei/Ordner: Öffnen, In Dateien zeigen, Umbenennen, In den Papierkorb, Eigenschaften (die Änderung macht Files); auf ein Laufwerk: Öffnen, Auswerfen. Rechtsklick auf den Hintergrund: Neue Verknüpfung (App im App-Raster wählen), Neuer Ordner, Einfügen, Symbolgröße, Symbole anzeigen, Nach Name sortieren, Hintergrund ändern, Anzeige-Einstellungen, Schreibtisch anpassen, Terminal hier. Verknüpfung auch über „Zum Schreibtisch hinzufügen“ im App-Raster oder durch Ziehen einer App aus dem App-Raster auf den Schreibtisch. |

### 5.2 Fenster

| Thema | Verhalten |
|---|---|
| Titelleiste | App zeichnet sie nicht selbst – vom Compositor, überall gleich: Titel in der Mitte, Knöpfe am **Ende** (verkleinern, maximieren, schließen). Doppelklick: maximieren. Rechtsklick: Fenstermenü (verschieben, Größe, immer oben, auf alle Arbeitsflächen, auf andere Arbeitsfläche/Bildschirm, schließen). Apps dürfen eine Kopfleiste in die Titelleiste legen (Suchen, Menü). |
| Verschieben/Größe | Ziehen an Titelleiste; Super+Ziehen überall; Ränder/Ecken 8 breit zum Ziehen. |
| Andocken | an den Rand ziehen: Hälfte; in die Ecke: Viertel; nach oben: maximieren. Beim Hälfte-Andocken Vorschlag für die andere Hälfte. Gemeinsame Kante zweier angedockter Fenster verschiebt beide. Tasten: Super+Pfeile. |
| Wechseln | Alt+Tab (Apps), Alt+` (Fenster der App), Super+Tab; Vorschau mit Titel. |
| Arbeitsflächen | dynamisch oder feste Zahl; nur Hauptbildschirm oder alle; Super+Bild↑/↓, mit Umschalt: Fenster mitnehmen; Touchpad: drei Finger wischen. |
| Mehrere Bildschirme | Anordnen, Haupt wählen, spiegeln/erweitern; Fenster merken sich Bildschirm; beim Abziehen wandern Fenster zurück und beim Anstecken wieder hin. |
| Skalierung | je Bildschirm 100–300 % in 25er-Schritten, Vorschlag aus DPI; Fenster wechseln den Maßstab beim Bildschirmwechsel scharf (kein Hochrechnen). |
| Dialoge | hängen am Elternfenster, verdunkeln nur dieses. Systemdialoge (Anmeldung/Bestätigung, Datei wählen, Drucken, Berechtigung) kommen vom System, nie von der App. |
| Keine Antwort | Fenster reagiert 5 s nicht: Hinweis „Warten · Beenden erzwingen“. |

### 5.3 Weitere Flächen der Hülle

| Fläche | Inhalt |
|---|---|
| Anmeldebildschirm | Benutzerliste, Kennwort/PIN, Fingerabdruck/Gesicht (wenn vorhanden), Tastaturbelegung, Barrierefreiheit, Ein/Aus. Verschlüsselte Platte: vorher Entsperr-Bildschirm (Passphrase), arabisch. |
| Sperrbildschirm | Uhr, Datum, Benachrichtigungen (Inhalt je Einstellung verborgen), Medien-Steuerung, Notfall-Ausschalten. |
| Benachrichtigungen | Banner am oberen Ende (5 s, bleibt bei Mauszeiger), nach App gruppiert, Aktionen direkt (Antworten, Öffnen), Dringlichkeit: leise/normal/wichtig; Vollbild/Präsentation unterdrückt Normales. |
| Bildschirmfoto/-aufnahme | Taste „Druck“: Auswahl Bereich · Fenster · Bildschirm, Foto oder Film, mit/ohne Zeiger; Ergebnis in Bilder/Bildschirmfotos + Zwischenablage; Meldung mit „Öffnen · Bearbeiten · Löschen“. |
| Zwischenablage | Verlauf (Super+V), anheftbar, „vertraulich“ markierte Einträge (Kennwörter) nie im Verlauf. |
| Bildschirmtastatur | erscheint bei Touch ohne Tastatur; arabische Belegung zuerst. |
| Rückmeldungen | Lautstärke/Helligkeit/Tastaturbeleuchtung als kleine Anzeige; Datenträger eingesteckt → Meldung mit „Öffnen · Auswerfen“. |
| Systemdialoge | Bestätigung mit Kennwort/Fingerabdruck (nennt *was* und *welche App*), Datei öffnen/speichern, Drucken, Berechtigungsfrage (Kamera, Mikrofon, Standort, Bildschirm teilen, Dateien außerhalb des App-Bereichs), App wählen („Öffnen mit“), Farbe wählen, Schrift wählen. |
| Helfer | kleiner Knopf in den Schnelleinstellungen und Super+Leertaste; nur wenn Komponente „Helfer“ installiert ist. |

### 5.4 Tastenkürzel (Standard, alle änderbar)
Super: Übersicht · Super+A: App-Raster · Super+L: Sperren · Super+D: Arbeitsfläche zeigen · Super+E: Dateien · Super+T/Strg+Alt+T: Terminal ·
Super+I: Einstellungen · Super+V: Zwischenablage · Super+Leertaste: Eingabesprache wechseln (Helfer dann Super+H) · Druck: Bildschirmfoto ·
Alt+F4: Fenster schließen · Super+Pfeile: Andocken · Super+Bild↑/↓: Arbeitsfläche · Alt+Tab · Super+1…9: Dock-App n · Super+P: Bildschirm-Modus · Strg+Alt+Entf: Ein/Aus-Menü.
Super+Leertaste wechselt die Sprache (wie Ubuntu); der Helfer liegt auf Super+H.

### 5.5 Dieselbe Hülle auf Handy und Tablet
`shell-mobile` nutzt dieselben Dienste und Bausteine: Leiste → Statusleiste; Schnelleinstellungen → Wisch von oben;
Dock → Favoritenzeile; Übersicht → Wisch von unten (letzte Apps); Fenster → Vollbild/geteilter Bildschirm.
Desktop-Modus am Handy/Tablet = `shell-desktop` auf dem externen Bildschirm, das Gerät wird Touchpad/Tastatur.

---

## 6. Installation – Ablauf wie bei Ubuntu 26.04, mit Ultar-Schritten

Start vom Medium in eine **Live-Umgebung** (wie Ubuntu: „Ausprobieren oder installieren“); der Installer ist eine normale
App (`apps/installer`) und läuft auf dem Ultar-Kern.

| # | Schritt | Inhalt | Regeln |
|---|---|---|---|
| 1 | **Sprache** | Liste, Arabisch zuerst und vorausgewählt; ganze Oberfläche wechselt sofort | – |
| 2 | **Barrierefreiheit** | Sehen (große Schrift, hoher Kontrast, Vergrößerung, Bildschirmleser), Hören (optische Signale), Tippen (Bildschirmtastatur, Einfingerbedienung), Zeigen (Klickhilfe) | sofort wirksam, werden ins System übernommen |
| 3 | **Tastatur** | Belegung + Variante, Erkennung per Tastendruck, Testfeld; zweite Belegung (Arabisch + Latein) als Vorschlag | Kürzel zum Wechseln wird angezeigt |
| 4 | **Netz** | Kabel · WLAN wählen · verstecktes Netz · „ohne Internet fortfahren“ | Installation geht vollständig ohne Netz |
| 5 | **Ausprobieren oder installieren** | „Ultar OS installieren“ · „Erst ausprobieren“ | – |
| 6 | **Art** | Geführt · Automatisch (Antwortdatei von Stick/Adresse, für viele Geräte) | – |
| 7 | **Hardware & Treiber** *(Ultar)* | erkannte Geräte mit Treiber und Zustand; Alternativen wählbar; Geräte ohne Treiber deutlich genannt mit Folge („WLAN geht nicht – Kabel nutzen“); „Bauplan speichern“; bei Netz: neuere Treiber holen | alles aus dem Medium, Probe live |
| 8 | **Umfang** | **Grundsystem** (Standard) · **Erweitert** · **Eigene Auswahl** → Komponentenliste (Abschnitt 7) mit Größe; Summe und freier Platz immer sichtbar | spätere Installation über den Store wird hier gesagt |
| 9 | **Zusätze** | Medienformate (Codecs) · Firmware der Hersteller (geschlossen, wird nur geladen) – je mit Erklärung | Standard: an, wenn im Medium |
| 10 | **Platte** | „Ganze Platte verwenden“ · „Neben vorhandenem System“ (Regler, zeigt Windows/Linux mit Namen) · „Manuell“ (Partitionen, Einhängepunkte, Dateisystem, Startpartition) | fremde Systeme werden erkannt und nie ohne ausdrückliche Wahl berührt |
| 11 | **Verschlüsselung** | keine · Passphrase · Gerätechip (TPM) + Wiederherstellungsschlüssel (anzeigen, QR, auf Stick speichern) | TPM-Wahl erscheint nur, wenn nutzbar; sonst Grund nennen |
| 12 | **Konto** | Name, Rechnername, Benutzername, Kennwort (Stärke-Anzeige), „Kennwort zur Anmeldung verlangen“, Profilbild | Rechnername aus Name vorgeschlagen, lateinisch |
| 13 | **Zeitzone** | Karte + Liste, Vorschlag aus Netz/Sprache; Uhrformat 12/24 | – |
| 14 | **Zusammenfassung** | alles auf einer Seite, besonders: **was gelöscht wird** (rot, mit Plattenname und Größe); „Installieren“ erst nach Haken bei Löschung | letzte Stelle ohne Schreibzugriff |
| 15 | **Installation** | Fortschritt mit Schrittname, Restzeit, Protokoll einblendbar; dabei kurze Einführung (5 Seiten: Übersicht, Dock, Store, Datenschutz, Helfer) | Abbruch bis „Daten kopieren“ möglich; Fehler nennt Ursache + Ausweg + Protokoll speichern |
| 16 | **Fertig** | „Neu starten“ · „Weiter ausprobieren“; Hinweis „Medium entfernen“ | – |

**Ersteinrichtung nach dem ersten Start** (kurz, jeder Punkt überspringbar): Willkommen → Datenschutz (Standort, Diagnose-
berichte: Standard **aus**, klar erklärt) → Ultar-Konto (optional: Store-Käufe, Sicherung, Verbund über Internet) →
Verbund (nur wenn gewählt: Gerät koppeln) → Helfer (nur wenn gewählt: Modell wählen/laden) → „Mehr Apps im Store“ → fertig.

**Handy:** derselbe Ablauf (gleicher Code `installer/flow`), Bedienung über Ultar Flash am PC bzw. am Gerät; Schritte 10/11
entfallen bzw. sind fest.

---

## 7. Grundsystem und Komponenten – wählen bei der Installation, später über den Ultar Store

**Grundsatz:** Das Grundsystem ist klein und vollständig bedienbar. Alles andere ist eine **Komponente**: ein signiertes
Paket (`.uap`) oder eine Gruppe davon, mit Diensten, Rechten und Größe. Installieren und Entfernen geht jederzeit über den
Store – ohne Rest, ohne Neustart (außer es steht dabei).

### 7.1 Immer dabei (Grundsystem)
Hülle, Einstellungen, Dateien, Ultar Store, Browser, Terminal, Texteditor, Bild-/Dokument-/Medien-Betrachter (leicht),
Rechner, Uhr, Systemmonitor, Bildschirmfoto, Hilfe, Datenträger, Schriften/Zeichen, Sicherung, Kennwörter, Drucken/Scannen,
Protokolle, Treiber & Hardware, Updates, Datenschutz-Zentrale.

### 7.2 Wählbare Komponenten

| Komponente | Inhalt | Dienste | Im Umfang |
|---|---|---|---|
| **Büro** | Dokumente (Word-Formate), Tabellen (Excel-Formate), Präsentationen, PDF-Werkzeuge (bearbeiten, unterschreiben, zusammenfügen) | – | Erweitert |
| **Coder** | Code-Editor mit Projekten, Git, Terminal-Panel, Sprachserver; Rust-/C-/Python-Werkzeuge einzeln | – | Auswahl |
| **Medien** | Musik, Video (voll), Foto-Verwaltung/-Bearbeitung, Aufnahme | Medien-Index | Erweitert |
| **Kommunikation** | E-Mail, Kalender (voll), Kontakte, Videoanrufe | Konten-Abgleich | Erweitert |
| **Verbund** | Geräte koppeln, Bildschirm/Eingabe teilen, Dateien/Zwischenablage, Rechenleistung bündeln | `mesh` (aus = nicht installiert → keine offenen Ports, kein Hintergrunddienst) | Auswahl |
| **Helfer & lokale KI** | Helfer-Figur, Sprachsteuerung, Modell-Verwaltung | `llm`, Sprach-Dienst | Auswahl (prüft RAM/GPU, empfiehlt Modell) |
| **Linux-Programme** | Ubuntu-Bereich für `.deb` | `compat` | Auswahl |
| **Windows-Programme** | `.exe/.msi` in eigener Zelle | – | Auswahl |
| **Android-Apps** | abgeschottete Laufzeit | – | Auswahl |
| **Container & Server** | Zellen/Kiri, Web-Verwaltung, SSH-Server | `cells`, `admin`, `sshd` | Auswahl (Server: Standard) |
| **Telefon** | Anrufe, SMS, USSD | `radio` | automatisch bei Modem |
| **Fernzugriff** | Bildschirm freigeben/steuern | Freigabe-Dienst | Auswahl |

Regeln: Komponenten hängen nur vom Grundsystem ab (oder nennen ihre Abhängigkeit ausdrücklich); eine Datei, deren App
fehlt, bietet beim Öffnen die Installation an („Tabellen aus dem Store installieren – 38 MB“); Einstellungen einer
Komponente erscheinen erst, wenn sie installiert ist.

### 7.3 Ultar Store – Flächen und Funktionen

| Fläche | Inhalt |
|---|---|
| Entdecken | Empfehlungen, Kategorien, „Komponenten von Ultar“ oben |
| Suche | Name, Beschreibung, Dateityp („.xlsx“ findet Tabellen), arabisch tolerant |
| App-Seite | Symbol, Name, Herausgeber (geprüft?), Bilder, Beschreibung, **Rechte in Klartext**, Größe (Download/installiert), Version, Änderungen, Quelle (Ultar · Linux · Windows · Android), Knopf: Installieren → Fortschritt → Öffnen · Entfernen |
| Installiert | Liste mit Größe, zuletzt benutzt; Entfernen (Daten behalten/löschen), Rechte ändern |
| Aktualisierungen | System (A/B, Neustart nötig) getrennt von Apps; automatisch ja/nein, nur im WLAN, Zeitfenster; Verlauf; Zurückrollen einer App-Version |
| Datei installieren | lokale Paketdatei (USB) – Signatur wird geprüft, unsigniert nur nach Freischaltung in Datenschutz & Sicherheit |
| Offline-Quelle | Stick/Ordner als Store-Quelle (für Orte ohne Internet), signierter Katalog |
| Konto | Anmeldung optional; Käufe/Lizenzen |

Zustände je Fläche: lädt · offline (zeigt Installiertes + Offline-Quellen) · Fehler mit Ursache · zu wenig Platz (zeigt, was frei wird).

---

## 8. Einstellungen – vollständige Liste

Aufbau: Seitenleiste (Bereiche, mit Suche) + Inhalt; kompakt: Liste → Seite. Jede Änderung wirkt sofort; Riskantes
(Auflösung, Netz) nimmt sich ohne Bestätigung zurück. Jede Zeile ist über die Systemsuche erreichbar.
Bedingung = Fähigkeit, ohne die der Bereich nicht erscheint.

| Bereich (Bedingung) | Einstellungen |
|---|---|
| **WLAN** (wifi) | an/aus · Netze (Stärke, Sicherheit) · verbinden (Kennwort, WPA2/WPA3, Unternehmen) · verstecktes Netz · bekannte Netze (automatisch verbinden, vergessen, als getaktet markieren) · je Netz: Adresse automatisch/fest, DNS, Proxy, zufällige Geräteadresse · Hotspot (Name, Kennwort, Band, QR) · Flugmodus |
| **Netzwerk** (network) | Kabel: Zustand, Geschwindigkeit, IPv4/IPv6 automatisch/fest, DNS, Routen, MTU · VPN: hinzufügen (WireGuard, Datei importieren), an/aus, „immer an“ mit Sperre · Proxy · Brandmauer: Profil (öffentlich/privat), offene Dienste |
| **Mobilfunk** (modem) | Daten an/aus · Roaming · bevorzugte Netzart · Netzwahl automatisch/manuell · Zugangspunkt · SIM-PIN · Verbrauch + Limit · zweite SIM |
| **Bluetooth** (bluetooth) | an/aus · sichtbar · Geräte koppeln/verbinden/entfernen · je Gerät: Art, Akku, Tonprofil |
| **Anzeige** (display) | **Anordnung:** Bildschirme als nummerierte Rechtecke, per Ziehen an die echte Lage anpassen (rasten an Kanten ein); „Bildschirme kennzeichnen“ (Nummer groß auf jedem Bildschirm), „Erkennen“ · **Modus bei mehreren Bildschirmen:** Erweitern · Spiegeln (gleiche Auflösung wird automatisch gewählt; wählbar, welche Bildschirme spiegeln) · Nur ein Bildschirm · **Schnellwahl Super+P:** Nur dieser Rechner · Spiegeln · Erweitern · Nur zweiter Bildschirm · **Je Bildschirm:** Hauptbildschirm (Leiste, neue Apps, Anmeldebildschirm), Auflösung (empfohlene markiert), Größe der Elemente 100–300 %, Bildwiederholrate (variabel wenn möglich), Ausrichtung (quer/hoch/gedreht), HDR, Farbprofil, Helligkeit (auch externer Bildschirm über DDC, wenn unterstützt), Übergröße-Ausgleich (TV) · **Allgemein:** Nachtlicht (Plan, Wärme), Fensterlage je Bildschirm merken, Leiste/Dock auf Haupt- oder allen Bildschirmen, Verhalten bei zugeklapptem Deckel mit externem Bildschirm · **Sicherheit:** jede Änderung von Auflösung/Modus/Anordnung fragt „Behalten?“ und nimmt sich nach 15 s zurück · gemerkt wird je Bildschirm-Kombination (Dock zu Hause ≠ Beamer im Büro) |
| **Ton** (audio) | Ausgabegerät, Lautstärke, Balance, Test · Eingabegerät, Pegel-Anzeige · Lautstärke je App · Systemklänge (Thema, Lautstärke) · Über-Verstärkung an/aus |
| **Energie** (immer; Akku-Teile nur mit battery) | Modus Leistung/Ausgewogen/Sparen · Akku-Stand, Verlauf, Zustand · Ladegrenze (80 %) · Bildschirm dunkel/aus nach · Bereitschaft nach (Netz/Akku) · Netzschalter-/Deckel-Aktion · Akku in % zeigen · Sparen automatisch bei x % |
| **Arbeitsfläche & Dock** | Dock: Lage, Größe, ausblenden, alle Bildschirme, Papierkorb/Datenträger zeigen · Schreibtisch-Symbole: an, Größe, Anfangsecke · Leiste: Sekunden, Wochentag, Datum, Hijri |
| **Fenster & Arbeitsflächen** | Andocken an Rändern · Vorschlag beim Andocken · Arbeitsflächen dynamisch/fest (Zahl) · nur Hauptbildschirm · Fensterwechsel nur aktuelle Arbeitsfläche · heiße Ecke · Fokus per Klick/Zeiger |
| **Aussehen** | Hell/Dunkel/automatisch (Sonnenstand) · Akzentfarbe · Hintergrund (Bild, eigenes, Diaschau) · Schriftgröße · Symbolgröße |
| **Benachrichtigungen** | Nicht stören (+ Zeitplan) · auf Sperrbildschirm (Inhalt zeigen) · je App: an, Banner, Ton, Sperrbildschirm, wichtig |
| **Suche** | Quellen an/aus und Reihenfolge · Orte für Dateisuche · Verlauf |
| **Apps** | Standard-Apps (Web, E-Mail, Kalender, Musik, Video, Fotos, Dokumente, je Dateityp) · je App: Rechte (Kamera, Mikrofon, Standort, Dateien, Netz, Hintergrund, Benachrichtigungen), Speicher, Autostart, Standard für · Wechselmedien (Aktion je Art) |
| **Maus & Touchpad** (pointer) | Haupttaste · Zeigergeschwindigkeit · Beschleunigung · Bildlaufrichtung · Touchpad: Tippen = Klick, Zwei-Finger-Bildlauf, beim Tippen aus, Gesten (mit Vorschau) · Zeigergröße |
| **Tastatur** (keyboard oder touch) | Eingabesprachen (Reihenfolge, je Fenster merken) · Wechsel-Kürzel · Sonderzeichen-Taste · Wiederholung/Verzögerung · Kürzel (alle, mit Suche, Konflikt-Anzeige, eigene) · Bildschirmtastatur · Korrektur/Vorschläge |
| **Drucker & Scanner** | hinzufügen (Netz automatisch, USB, Adresse) · Standard · Warteschlange · Optionen (Papier, beidseitig, Qualität) · Testseite · Scanner |
| **Farbe** (display) | Profil je Gerät, importieren, kalibrieren (mit Messgerät) |
| **Barrierefreiheit** | Sehen: hoher Kontrast, große Schrift, Vergrößerung (Faktor, Folgen), Bildschirmleser (Stimme, Tempo), Zeiger groß, Bewegung verringern, Farbfilter · Hören: optisches Signal, Mono, Untertitel · Tippen: Bildschirmtastatur, Einrast-, Anschlag-, Tastenverzögerung · Zeigen: Maustasten über Tastatur, Klick durch Verweilen, Doppelklick-Zeit · immer in der Leiste zeigen |
| **Datenschutz & Sicherheit** | Bildschirmsperre (nach, bei Bereitschaft, Meldungen) · Hauptschalter und einzeln: Kamera, Mikrofon, Standort, Sensoren, Netz · je App-Rechte (Verweis) · Dateiverlauf + Papierkorb/Temp automatisch leeren · Diagnoseberichte · Verschlüsselung: Zustand, Passphrase ändern, Wiederherstellungsschlüssel, TPM · Anmeldung: Kennwort, PIN, Fingerabdruck, Gesicht · unsignierte Pakete erlauben (aus) · Gerätesicherheit: Startkette, Firmware-Stand · Upload-Protokoll je App |
| **Freigaben** | Rechnername · Fernzugriff (Bildschirm) · Dateifreigabe · Medienfreigabe · SSH – jede mit Netz-Auswahl; nur wenn Komponente da |
| **Konten** | Ultar-Konto · Online-Konten (E-Mail/Kalender/Kontakte/Dateien: Nextcloud, Google, Microsoft, IMAP/CalDAV) – was je Konto abgeglichen wird |
| **Benutzer** | hinzufügen/entfernen · Art (Verwalter/Standard) · Kennwort · automatische Anmeldung · Profilbild · Sprache je Benutzer · Kinderschutz (Zeiten, Apps) |
| **Region & Sprache** | Sprache des Systems · Formate (Datum, Zahl, Währung, Maße, Papier) · erster Wochentag · Kalender (gregorianisch + Hijri) |
| **Datum & Zeit** | automatisch (Netz) · Zeitzone automatisch/Wahl · 12/24 · Hardware-Uhr |
| **Speicher** | Belegung nach Art · große Dateien · Aufräumen (Papierkorb, Temp, alte Updates) automatisch · Datenträger (Zustand, einhängen, formatieren → App „Datenträger“) |
| **Treiber & Hardware** *(Ultar)* | Geräteliste: Treiber, Zustand, Alternativen, wechseln, zurückrollen · Geräte ohne Treiber → Bauplan · Firmware-Updates (USB/online) · Selbsttest |
| **Updates** | Stand · suchen · automatisch (System/Apps getrennt) · Zeitfenster · Kanal (stabil/Vorschau) · Offline-Update von USB · Verlauf · vorige Systemfassung starten |
| **Sicherung** | Ziel (USB, Netz, Ultar-Konto) · was · Zeitplan · verschlüsselt · wiederherstellen (einzelne Dateien, alles) |
| **Verbund** (Komponente) | an/aus · dieses Gerät sichtbar · gekoppelte Geräte (Rechte je Gerät: sehen, steuern, Dateien, Rechenleistung) · über Internet (Ultar-Konto) · Leistung teilen (Grenzen, nur am Netzteil) |
| **Helfer** (Komponente) | an/aus · Weckwort · Stimme · Modell (lokal, Größe, Speicherort) · was er sehen/tun darf · Verlauf löschen |
| **System / Info** | Gerätename · Ultar-Version, Kern-Version · Prozessor, Speicher, Grafik, Platte · Seriennummer · Lizenz/Herausgeber · Protokolle · Entwickleroptionen (aus: Fern-Shell, Fehlersuche, Leistungsanzeige) · Zurücksetzen (Einstellungen / alles löschen) |

---

## 9. Apps des Grundsystems – Verzeichnis mit Ubuntu-Gegenstück

| Ultar-App | Ubuntu 26.04 | Muss können (Kern der Beschreibung) |
|---|---|---|
| Dateien | Dateien (Nautilus) | Seitenleiste (Orte, Favoriten, Datenträger, Netz), Raster/Liste, Reiter, Suche (Name+Inhalt), Vorschau (Leertaste), Kopieren mit Fortschritt/Konflikt, Papierkorb mit Wiederherstellen, Umbenennen (mehrere), Packen/Entpacken, Eigenschaften + Rechte, Netzlaufwerke (SMB, SFTP, WebDAV), „Öffnen mit“, Ziehen & Ablegen |
| Terminal | Ptyxis | Reiter, Profile, Suche, Farben, arabischer Text (BiDi), Kopieren/Einfügen, Verweise anklickbar |
| Texteditor | Texteditor | Reiter, Suchen/Ersetzen, Hervorhebung, Zeilennummern, Kodierung, automatisches Sichern, Sitzung wiederherstellen |
| Dokumente | Papers | PDF anzeigen, suchen, Inhaltsverzeichnis, markieren/anmerken, Formulare, unterschreiben, drucken, Nachtmodus |
| Bilder | Loupe | anzeigen (alle gängigen Formate), Zoom/Gesten, drehen/zuschneiden, Infos, als Hintergrund, drucken |
| Videos / Musik (leicht) | Showtime | abspielen, Untertitel, Tonspur, Tempo, Bild-in-Bild |
| Systemmonitor | Resources | Prozessor/Speicher/Grafik/Platte/Netz/Akku mit Verlauf, Apps und Prozesse (beenden), Autostart, je App Upload/Download |
| Ultar Store | App Center | Abschnitt 7.3 |
| Datenschutz-Zentrale | Security Center | Rechte je App, Hauptschalter, Protokoll der Zugriffe |
| Einstellungen | Einstellungen | Abschnitt 8 |
| Ultar Browser (Servo), Rückfall Firefox als Komponente | Firefox | Reiter, Lesezeichen, Verlauf, Downloads, privates Fenster, Kennwörter, Werbe-/Tracking-Schutz, PDF, Übersetzen, Erweiterungen (später) |
| Rechner · Uhr · Kalender (leicht) · Zeichen · Schriften | gleichnamig | Rechner: einfach/wissenschaftlich/Programmierer/Umrechnen · Uhr: Weltuhr, Wecker, Stoppuhr, Timer · Kalender mit Hijri |
| Datenträger · Belegung | Laufwerke · Festplattenbelegung | Partitionieren, formatieren, Zustand (SMART), Abbild schreiben/erstellen (Startmedium), Belegung als Karte |
| Kennwörter & Schlüssel | Passwörter und Verschlüsselung | Schlüsselbund, SSH-/Signier-Schlüssel, Zertifikate |
| Sicherung | Datensicherungen | Abschnitt 8 „Sicherung“ |
| Scannen · Kamera · Bildschirmfoto | Dokument-Scanner · Kamera · eingebaut | – |
| Protokolle · Hilfe | Protokolle · Hilfe | Hilfe: durchsuchbar, offline, arabisch zuerst; verlinkt direkt in die Einstellung |
| Fernzugriff (Komponente) | Verbindungen | RDP/VNC-Client |

„Leicht“ heißt: reicht zum Öffnen und Benutzen; die volle App kommt als Komponente (Abschnitt 7.2).

---

## 10. Gleichwertigkeit mit Ubuntu 26.04 – Abnahmeliste

Fertig ist der Desktop erst, wenn jede Zeile erfüllt und am Testrechner in Arabisch gezeigt ist:

- [ ] Installation in ≤ 10 Minuten, alle 16 Schritte, ohne Internet, neben Windows, mit Verschlüsselung
- [ ] Anmelden, Sperren, Benutzer wechseln, Bereitschaft und Aufwachen (Deckel)
- [ ] Zwei Bildschirme mit verschiedener Skalierung, An-/Abstecken im Betrieb
- [ ] WLAN (WPA2/WPA3), Kabel, VPN, Hotspot, Bluetooth-Kopfhörer und -Maus
- [ ] Ton aus Lautsprecher/Kopfhörer/USB/Bluetooth mit automatischem Wechsel; Mikrofon
- [ ] Drucken und Scannen im Netz (treiberlos, IPP)
- [ ] USB-Stick: einstecken → Meldung → Dateien → auswerfen
- [ ] Jede Grund-App aus Abschnitt 9 mit ihren Muss-Funktionen
- [ ] Jede Einstellung aus Abschnitt 8 wirkt und übersteht den Neustart
- [ ] Store: installieren, aktualisieren, entfernen – online und von USB; Komponenten nachinstallieren
- [ ] System-Update mit Rückfall auf die vorige Fassung
- [ ] Bedienung nur mit Tastatur; nur mit Bildschirmleser; mit 200 % Schrift
- [ ] Leistungsbudgets aus PLAN §3.6
- [ ] Kein sichtbares Element ohne Funktion; kein englischer/deutscher Rest in Arabisch

Bewusst **anders** als Ubuntu: Arabisch zuerst · Rechte je App für alle Apps (nicht nur bestimmte Paketarten) ·
Treiber-Schritt im Installer · Komponenten statt fester App-Auswahl · Diagnoseberichte standardmäßig aus.

---

## 10b. Entwurfsbilder

Die Bilder zur Freigabe liegen auf der Entwurfsfläche „Ultar OS – Desktop-Entwurf“ (claude.ai, Design). Stil: an Windows 11
angelehnt (zentrierte Symbole, runde Ecken, Einstellungen mit Seitenleiste und Karten), eigene Marke und Farben.

## 11. Entscheidungen (22.11.2026)

| # | Frage | Entscheidung |
|---|---|---|
| 1+2 | Leiste und Dock | **Standard: eine einzige Leiste unten wie bei Windows 11** – Mitte: Übersicht · alle Apps · Suche · angeheftete und laufende Apps; Ende: Tastaturbelegung · Status (öffnet Schnelleinstellungen) · Uhr/Datum (öffnet Kalender und Benachrichtigungen); Anfang: Papierkorb. Schnelleinstellungen und Kalender öffnen von unten am Ende. Umstellbar: Symbole am Anfang statt Mitte · Leiste oben + getrenntes Dock (Abschnitt 5.1 beschreibt diese Variante) |
| 3 | Fensterknöpfe | am **Ende** der Titelleiste (Arabisch: links) |
| 4 | Browser | siehe unten |
| 5 | Farbwelt | **Dunkel („Ultar Noir“ mit Gold) als Standard**, Hell wählbar, dazu „automatisch nach Sonnenstand“ |

### Browser – Empfehlung (angenommen)

**Grund-App „Ultar Browser“ = eigene Oberfläche (Rust, `ultar-ui`) auf der Engine Servo.**
- Servo ist in Rust geschrieben und als Bibliothek einbettbar (Crate `servo` seit April 2026) – passt zu „eigener Kern,
  kein Linux-Unterbau“ und läuft als normale App in einer Zelle.
- Stand November 2026: monatliche Fassungen (0.7), viele echte Seiten laufen, aber **noch nicht jede** – für den Alltag
  allein reicht das nicht sicher.
- Deshalb zwei Stufen:
  1. **Jetzt:** Ultar Browser (Servo) ist die Grund-App. Erkennt er eine Seite, die nicht richtig läuft, bietet er
     „In Firefox öffnen“ an.
  2. **Rückfall:** **Firefox** als wählbare Komponente über den Store (läuft über die Komponente „Linux-Programme“).
     Im Installer-Umfang „Erweitert“ vorausgewählt, solange Servo Lücken hat.
- Der bisherige Weg (WPE WebKit in `apps/ultar-web`) entfällt: C++-Engine mit Linux-Abhängigkeiten, widerspricht der
  Kern-zuerst-Regel.
- Was der Ultar Browser selbst liefert (nicht Servo): Reiter, Adresszeile mit Suche, Lesezeichen, Verlauf, Downloads,
  privates Fenster, Kennwörter (Schlüsselbund), Werbe-/Tracking-Sperre, Rechte je Seite, arabische Oberfläche.
- Risiko offen benannt: DRM-Videos (Widevine) und einige Bank-/Konferenzseiten gehen vorerst nur im Rückfall-Browser.

---

## 12. Nachtrag aus der Bestandsprüfung (22.11.2026)

Drei Prüfer haben das alte System gegen diesen Entwurf gehalten. Die vollständigen Listen – jede Zeile mit Fundstelle im
alten Code und Vorschlag – liegen unter `docs/ui/audit/` und werden Zeile für Zeile ins Verzeichnis (`inventory.toml`)
übernommen. Ergebnis: Konto/Anrufe/Sicherheit **71** Funktionen, davon 51 nicht voll abgedeckt · Systemfunktionen/Apps
**191**, davon 130 nicht voll abgedeckt, dazu 31 Dopplungen im alten System mit je einem Zielort.
Die folgenden Punkte sind damit **Teil des Entwurfs**:

### 12.1 Ultar-Konto (Einstellungen → Konten → Ultar-Konto; Server `ultar.code-craft.tech`)

| Element | Funktion |
|---|---|
| Karte „Abgemeldet“ | Benutzername, Kennwort (Stärke, anzeigen), „Anmelden“ · „Konto anlegen“; Satz, wofür das Konto dient (Anrufe, Store, VPN, Verbund über Internet, Sicherung) |
| Zustände | abgemeldet · meldet an · angemeldet · offline (zeigt letzten Stand, nichts geht verloren) · Fehler mit Ursache (falsches Kennwort, Server nicht erreichbar, Zeit falsch) · gesperrt nach Fehlversuchen |
| Dieses Gerät | Gerätename, „registriert / nicht registriert“, „Gerät registrieren“, Liste meiner Geräte mit „Abmelden/Entfernen“ |
| **Ultar-Nummer** | `UOS-XXXX-XXXX` groß, Kopieren, QR; auch in System/Info und im Kopf der Telefon-App |
| Dienste des Kontos (je Schalter) | Ultar-Anrufe annehmen (Standard aus) · Ultar-VPN · Store-Käufe · Sicherung · Verbund über Internet |
| Erweitert | Server-Adresse (nur abgemeldet änderbar, mit Erreichbarkeits-Prüfung) · Kennwort ändern · Abmelden · Konto löschen |
| Ohne Konto | Gerät bleibt voll benutzbar; Store und Updates gehen dann über Offline-Quellen (USB). **Entscheidung nötig:** soll der Store online auch ohne Konto gehen? Der heutige Server verlangt ein Konto. |

### 12.2 Telefon – eine App, zwei Wege (Komponente „Kommunikation“; Mobilfunk-Teil nur mit Modem)

| Fläche | Inhalt |
|---|---|
| Wählfeld | Umschalter **Ultar-Nummer · Mobilfunk** (Mobilfunk nur mit SIM); Ziffern, `UOS-`-Vorsatz, Kontaktvorschläge, Sprach- oder Videoanruf |
| Eingehender Anruf | Vollbild/Banner: Name oder Nummer, Weg (Ultar/SIM 1/SIM 2), Annehmen · Ablehnen · mit Nachricht ablehnen; auch auf dem Sperrbildschirm |
| Gespräch | Dauer, Weg, Verschlüsselung (Ende-zu-Ende bei Ultar), Stumm, Lautsprecher/Ausgabegerät, Tastenfeld, Halten, Video an/aus, Kamera wechseln, Bildschirm teilen, Auflegen |
| Anrufliste | ein-/ausgehend/verpasst, Rückruf, Kontakt anlegen, Nummer sperren, Eintrag löschen |
| Zustände | nicht angemeldet · nicht erreichbar · Gegenseite offline · kein Mikrofon/keine Kamera (Recht fehlt) · schlechte Verbindung |
| Einstellungen | Klingelton, Vibration, Nicht stören-Ausnahmen, gesperrte Nummern, Anrufe auf anderen eigenen Geräten klingeln lassen |
| Mobilfunk zusätzlich | SMS (in „Nachrichten“), USSD-Codes mit Antwortdialog, Dual-SIM-Zuordnung (Daten/Anrufe/SMS), Netz- und **Funkmast-Wahl** (Auto · Smart · Manuell, Zelle anheften), Live-Signalwerte |

### 12.3 Weitere Ergänzungen

| Bereich | Ergänzt |
|---|---|
| **VPN** | „Ultar-VPN“ mit einem Tipp über das Konto (zusätzlich zum Datei-Import), Sperre bei Abbruch (Killswitch), Anzeige des Verbindungswegs |
| **Werbe-/Tracker-Sperre** | systemweit (nicht nur im Browser): Schalter, Liste wählen/aktualisieren, Ausnahmen – unter Datenschutz & Sicherheit |
| **Zellen & Container** (neuer Bereich, Komponente) | je Zelle: Netz (aus · nur VPN · frei), Identität, Speicher, Grafikkarte, Protokoll der Uploads; Kiri-Container. **Offen:** der Ultar-Kern hat laut alter Unterlage noch keine Namensräume – Voraussetzung für Zellen, vor P5 zu klären |
| **Notfall** (neuer Bereich) | Notruf, Notfallkontakt, medizinische Angaben auf dem Sperrbildschirm, SOS-Licht; Tasten-Notausstieg in drei Stufen (Laut+Leise) |
| **Helfer** | Rechte-Stufen für Verwalter-Aktionen (immer mit Bestätigung), Link-Prüfung, Modellquellen Hugging Face/Ollama, Bild-Modell, geheimes Wort, Besitzer-Stimme, eigene Sprachbefehle |
| **Anmeldung** | Besitzer-Stimme und geheimes Wort neben PIN/Fingerabdruck/Gesicht |
| **Neue Grund-Apps** | Notizen (mit Aufgaben und Erinnerungen) · in „Uhr“: Gebetszeiten, Qibla, Fokus-Timer · „Werkzeuge“: Kompass/Wasserwaage (nur mit Sensoren), Zähler, Kennwort-Erzeuger |
| **Systemfunktionen** | Benachrichtigungsverlauf · Tonprofile/Vibration · automatische Helligkeit · Nutzungszeit · App-Fixierung · Schlafmodus · geplanter Neustart · Netzdiagnose · Dienste-Liste (in Systemmonitor) |
| **Verbund** | App-Weitergabe zwischen Geräten, Recht „Ton“. **Entscheidung nötig:** Mining im Verbund/Konto übernehmen oder streichen? |
| **Store** | Formate AppImage, Flatpak, rpm; Web-Apps (WhatsApp, YouTube) und Telegram/Spotify als normale Store-Einträge (Bereich 7) |
| **Installer** | Auswahl der Ausgabe „Server“ (ohne Oberfläche, mit Web-Verwaltung) |
| **Freigaben** | Web-Verwaltung als eigene Freigabe; SSH nur noch an **einer** Stelle (Freigaben), nicht zusätzlich in den Entwickleroptionen |

### 12.4 Entwurfsprüfung Runde 1 (vier Rollen)

Gefunden: 71 Abweichungen von den Gestaltungswerten, 31 Klicks ohne festgelegte Wirkung, 35 Arabisch-/Richtungs-Funde,
Kontrast der Texte in Ordnung (kleinster Wert 5,1:1). **Sofort behoben:** Zurück-Pfeil in Arabisch, Zahlen/Einheiten in
arabischem Text (3840 × 2160, 60 Hz), Begriffe (إظهار أرقام الشاشات, الأدوات المكتبية, تكرار), Monatsname تشرين الثاني,
Firefox setzt „Linux-Programme“, Kontrast der Rahmen und der App-Symbole im hellen Thema.
**Offen für Runde 2 (vor Freigabe):** Symbole mit Doppelbedeutung auflösen, alle Maße auf das Raster, Schriftstufen,
jeden undefinierten Klick festlegen, Zustandsbilder (leer, lädt, Fehler, offline, ohne Hardware), Größen statt Platzhalter.

### 12.5 Entscheidungen vom 22.11.2026 (abends)

| Frage | Entscheidung | Folge |
|---|---|---|
| Store und Updates ohne Konto | **Ja – beides geht ohne Ultar-Konto.** Später: Premium-Apps und bestimmte Updates nur mit Zugang. | Server bekommt offene Lese-Endpunkte für Katalog, kostenlose Apps und System-Updates (weiter mit Signaturprüfung am Gerät). Im Store drei Zustände je App: frei · „Premium – Anmeldung nötig“ · gekauft/freigeschaltet. Update-Kanäle: frei (Standard) · mit Zugang (z. B. Vorab, Langzeit-Pflege). Konto bleibt nötig nur für: Anrufe über Ultar-Nummer, Ultar-VPN, Sicherung, Verbund über Internet, Premium. |
| Mining | **Optional** – nicht im Grundsystem. | Eigene Komponente „Rechenleistung teilen (Mining)“ im Store, standardmäßig nicht installiert; nach Installation aus, mit Grenzen (nur am Netzteil, Temperatur, CPU-Anteil) und klarer Anzeige in der Leiste, solange es läuft. Kein Eintrag im Konto-Bildschirm. |
| Standard-Desktop | **Eine Leiste unten wie bei Windows 11** | siehe Abschnitt 11; Entwurfsbilder umgestellt, Leiste oben + Dock bleibt als Variante |
| Namensräume | **Werden in den Ultar-Kern eingebaut**, zusammen mit Gruppen und später Profilen mit Rechten | siehe PLAN.md §3.7 |

## 13. Verhalten je Element (aus Entwurfsrunde 2 übernommen)

Diese Festlegungen gelten verbindlich; die vollständige Klickliste aller Tafeln steht in `docs/ui/CLICKS.md`, die Symboltabelle in `docs/ui/ICONS.md`, die Tafeln samt Erzeuger in `design/boards/`. Die Abschnittsangabe in Klammern nennt, wozu die Festlegung gehört.



1. `shell.search` [§5.1] – Opens the Overview with the caret in its search field; typing searches at once. The bar itself holds no second search.
2. `shell.trash` [§5.1] – Click opens the Trash in Files. Files dropped on it are moved to the Trash (with undo). Right click: Open, Empty trash (asks first).
3. `shell.drive` [§5.1] – Click opens the mounted drive in Files. Right click: Open, Eject. The tile exists only while a drive is mounted.
4. `shell.kbd` [§5.1] – Shown only with more than one input language. Click opens a menu: the input languages (choosing one switches at once), "Keyboard settings". Super+Space switches without the menu.
5. `shell.status` [§5.1] – Opens the quick settings sheet above the end of the bar; second click, Esc or a click outside closes it and focus returns to this button.
6. `shell.clock` [§5.1] – Opens the calendar and notifications sheet above the end of the bar; closes like the quick settings. Only one of the two sheets is open at a time.
7. `desk.icon` [§5.1] – Single click selects the icon; double click or Enter opens it (folder or drive in Files). Right click: Open, Rename, Move to trash, Properties; on a drive also Eject.
8. `files.back` [§9 Files] – Goes back in the folder history of this window (Alt+Left; in Arabic the arrow points right). Disabled when there is no history.
9. `files.forward` [§9 Files] – Goes forward in the folder history of this window. Disabled when there is nothing to go forward to.
10. `files.place` [§9 Files] – Opens the place in this window.
11. `files.drive` [§9 Files] – Opens the mounted drive in this window.
12. `files.eject` [§9 Files] – Ejects the drive safely; a notification says when it may be removed. If files are open on it: names the app and offers "Eject anyway".
13. `files.search` [§9 Files] – Typing filters the current folder by name and content at once; Esc clears and leaves the field.
14. `files.newfolder` [§9 Files] – Creates a folder in the current folder with the name selected for typing (Ctrl+Shift+N).
15. `files.item` [§9 Files] – Single click selects (Ctrl/Shift: several). Double click or Enter opens: a folder in this window, a file with its default app; if that app is missing the Store offers to install it. Space: preview. Right click: context menu.
16. `browser.tabclose` [§9/§11 Browser] – Closes this tab (Ctrl+W); closing the last tab closes the window.
17. `browser.shield` [§9/§11 Browser] – Opens the page panel: blocked trackers and ads of this page, a switch "Protection for this site", the permissions of the site (camera, microphone, location, notifications) and the connection state. It does not switch anything by itself.
18. `browser.downloads` [§9/§11 Browser] – Opens the download list of the browser (progress, Open, Show in folder, Clear).
19. `qs.wifi` [§5.1 Quick settings] – Switches Wi-Fi on or off.
20. `qs.wifi.list` [§5.1 Quick settings] – Opens the list of networks inside the sheet (strength, security, connect, "Wi-Fi settings"); Back returns to the tiles.
21. `qs.wired` [§5.1 Quick settings] – Wired network has no switch: the tile shows the state and opens Settings > Network.
22. `qs.bluetooth` [§5.1 Quick settings] – Switches Bluetooth on or off.
23. `qs.bluetooth.list` [§5.1 Quick settings] – Opens the list of devices inside the sheet (connect, disconnect, "Bluetooth settings").
24. `qs.vpn` [§5.1 Quick settings] – Switches the last used VPN on or off; with no VPN set up it opens Settings > Network > VPN.
25. `qs.vpn.list` [§5.1 Quick settings] – Opens the list of VPN connections inside the sheet to choose which one is used.
26. `qs.toggle` [§5.1 Quick settings] – Switches the mode on or off at once (airplane mode, night light, dark theme, do not disturb). Airplane mode on disables the Wi-Fi, Bluetooth and mobile tiles.
27. `qs.powermode` [§5.1 Quick settings] – Steps through the power modes Performance, Balanced, Saver.
28. `qs.powermode.list` [§5.1 Quick settings] – Opens the three power modes inside the sheet as a choice.
29. `qs.mesh` [§5.1 Quick settings] – Switches Mesh on or off (component installed).
30. `qs.mesh.list` [§5.1 Quick settings] – Opens the list of paired devices inside the sheet.
31. `note.clear` [§5.1/§5.3] – Removes all notifications from the list; a bar "Undo" stays for 5 s.
32. `note.dismiss` [§5.1/§5.3] – Removes this one notification. The body of a notification is not clickable; only its buttons act.
33. `cal.day` [§5.1] – Selects the day and shows its appointments below the month; arrow keys move in the grid, the grid is one Tab stop.
34. `cal.month` [§5.1] – Shows the previous or next month (in Arabic the "previous" arrow points right).
35. `cal.event` [§5.1] – Opens the appointment in the Calendar app.
36. `lock.unlock` [§5.3] – Shows the sign-in for the locked user (any key, click or swipe does the same).
37. `login.show` [§5.3] – Shows or hides the typed password while pressed.
38. `display.select` [§8 Display] – Selects the display whose settings the card on the end side shows. Dragging moves it in the arrangement (snaps to edges) and asks "Keep?". Arrow keys change the selection.
39. `display.identify` [§8 Display] – Shows the number large on every display for 5 s.
40. `display.detect` [§8 Display] – Looks for displays again; shows a progress ring on the button and then "n displays found" or "No new display found".
41. `display.mode` [§8 Display] – Sets the mode and asks "Keep?". Mirror with three or more displays shows a row "Mirrored displays" with one check box per display; "Only one display" shows a choice "Display that stays on".
42. `display.revert` [§8 Display] – Restores the previous settings and closes the dialog. The same happens after 15 s without an answer. The dialog takes the focus when it opens ("Revert" is focused); Esc = Revert.
43. `display.keep.more` [§8 Display] – Restarts the 15 s once more (enough time for keyboard and screen-reader users).
44. `projection.mode` [§8/§5.4] – Super+P steps through the four modes, Enter confirms, Esc cancels; a mouse click selects and confirms at once. After a change the "Keep?" dialog follows. With three or more displays "second display" is the first external one.
45. `account.create` [§12.1] – Opens the page "Create account" (user name, password twice, optional e-mail for recovery).
46. `account.delete` [§12.1] – Deletes the account after a dialog that names what is lost and asks for the password.
47. `installer.step` [§6] – Jumps back to a finished step; everything entered stays. Later steps cannot be jumped to.
48. `installer.cancel` [§6] – Asks "Cancel the installation?" and returns to the live session; nothing has been written before step 15.
49. `installer.prev` [§6] – Goes to the previous step; entries of the current step stay.
50. `installer.next` [§6] – Goes to the next step. Disabled while the step is not valid (not enough space, passphrases differ, erase not confirmed); the reason stands next to it. On the summary it is called "Install" and starts step 15.
51. `installer.component` [§6] – Selects or deselects the component; the sum changes. Selecting Firefox selects Linux programs too; deselecting Linux programs deselects Firefox after a hint. A component that does not suit the device is disabled with the reason.
52. `installer.enc.show` [§6] – Shows or hides the passphrase.
53. `installer.edit` [§6] – Jumps to the step of this line; "Next" there returns to the summary.
54. `store.open` [§7.3] – Opens the app page (rights in plain words, size, Install or Remove). Cards never install directly.
55. `store.search.clear` [§7.3] – Empties the search field and shows Discover again.
56. `store.retry` [§7.3] – Tries to load the catalogue again.
57. `store.log` [§7.3] – Saves the error log as a file.
58. `store.free.cancel` [§7.3] – Cancels the installation; nothing is freed.
59. `phone.backspace` [§12.2] – Deletes the last digit; held: deletes all.

Further additions that are not a single click:

60. [§3] State tokens: `[state]` (overlays, disabled opacity, scrim, focus-ring role), `size.border`, `size.row_min`, `size.box` in `design/tokens.toml`; boards and toolkit read the same values (section 3, rows "Fokusrahmen" and "Zustände").
61. [§4] Icon table: see `ICONS.md` (new and split meanings).
62. [§5.1] Sheets of the shell (quick settings, calendar) take the focus when they open, keep it inside (Tab cycles) and return it to their trigger when they close.
63. [§5.2] The three title-bar buttons are not Tab stops; keyboard uses the window menu (Alt+Space) and the shortcuts. In Arabic the start position of new windows is mirrored.
64. [§8] Row "Main display": the main display shows a badge; every other display shows a button "Make this the main display". There is no "off".
65. [§3/§10] Units inside Arabic text are written in Latin (GB, MB, Hz, Gbit/s) as a left-to-right island; technical words are transliterated (واي فاي, بلوتوث), acronyms stay Latin (VPN, USB, HDR, PDF, SIM, QR, TPM, PIN).
