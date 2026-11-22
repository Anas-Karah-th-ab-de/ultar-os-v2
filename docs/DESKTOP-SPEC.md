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

**Bausteine in `ultar-ui`** (Apps bauen nur daraus): Knopf (Haupt · normal · leise · Gefahr), Symbolknopf, Schalter, Auswahl,
Schieber, Eingabefeld (mit Fehlertext), Suchfeld, Liste/Zeile, Karte, Seitenleiste, Kopfleiste, Reiter, Menü, Kontextmenü,
Dialog, Blatt (kompakt: von unten), Hinweisleiste, Fortschritt, Leerzustand, Tabelle, Baum, Tooltip, Abzeichen.
Jeder Baustein kennt: normal, schwebend, gedrückt, fokussiert, deaktiviert, lädt, Fehler.

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

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [Übersicht]  [App-Menü der aktiven App]        Sa 22 Nov  16:52        [Status] │  Leiste oben (32)
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
| **Arbeitsfläche (Hintergrund)** | Dateien/Ordner des Schreibtisch-Ordners als Symbole (abschaltbar); Rechtsklick: Neuer Ordner, Einfügen, Hintergrund ändern, Anzeige-Einstellungen, Terminal hier. |

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
Alt+F4: Fenster schließen · Super+Pfeile: Andocken · Super+Bild↑/↓: Arbeitsfläche · Alt+Tab · Super+1…9: Dock-App n · Strg+Alt+Entf: Ein/Aus-Menü.
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
| **Anzeige** (display) | Anordnung · Hauptbildschirm · spiegeln/erweitern · Auflösung · Bildwiederholrate (variabel) · Skalierung · Ausrichtung · Nachtlicht (Plan, Wärme) · HDR (wenn möglich) · Farbprofil · Helligkeit, automatisch (Sensor) |
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

## 11. Entscheidungen (22.11.2026)

| # | Frage | Entscheidung |
|---|---|---|
| 1 | Dock | **unten in der Mitte** als Standard; umstellbar auf Anfang/Ende |
| 2 | Leiste | **oben** als Standard; umstellbar nach **unten** (dann liegen Leiste und Dock als eine Zeile unten: Übersicht · Dock · Uhr · Status) |
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
