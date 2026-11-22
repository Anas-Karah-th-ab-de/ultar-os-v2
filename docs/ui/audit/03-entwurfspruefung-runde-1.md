# Entwurfsprüfung Desktop – vier Rollen

Stand 22.11.2026 · geprüft: `/home/claude/design/project/*.dc.html` (7 Bilder), `gen.py`, `b1.py`–`b5.py`, gegen `docs/DESKTOP-SPEC.md` §3–§8 (+§11).
Methode: nur Dateien gelesen, nichts gerendert. Die 7 HTML-Dateien sind bytegleich mit der Ausgabe der Generatoren (in Kopie unter /tmp nachgerechnet) – Fundstellen im Generator gelten also 1:1 für die Bilder.
Nicht geprüft: echtes Schriftbild/Umbruch (kein Rendern), Größenklassen kompakt/mittel (es gibt nur „weit“), hoher Kontrast (keine Palette vorhanden).

Kürzel: Main = M · DesktopBottom = DB · QuickSettings = QS · SettingsDisplay = SD · Projection = P · Installer = I · Store = S.

---

## Rolle 1 – DESIGNER (Abgleich mit §3/§4)

### 1.1 Abstände (erlaubt: 4·8·12·16·24·32·48)

| ID | Datei (Quelle) | Element | gefunden | verlangt |
|---|---|---|---|---|
| D1 | alle (gen.py `topbar`) | Leiste: Innenabstand, Lücken | `padding 0 10px`, `gap 10px`, `gap 6px` | 8 oder 12; 4 oder 8 |
| D2 | M, QS (gen.py `dock`) | Dock: Innenabstand/Lücke | `padding 8px 12px 10px`, `gap 10px` | 8/12, 8 oder 12 |
| D3 | alle (gen.py `tile`) | „läuft“-Punkt | `bottom -7px`, 5×5, `margin-left -2.5px` | 4 bzw. 8; Größe 4 |
| D4 | alle (gen.py `titlebar`) | Titelleiste: Lücken | `gap 6px`, `gap 2px` | 4 oder 8 |
| D5 | SD (gen.py `toggle`) | Schalter-Innenabstand | `padding 0 3px` | 4 |
| D6 | M (b1 `side`,`item`,`sidebar`,`grid`,`term`) | Dateien-Seitenleiste, Dateikacheln, Terminal | `padding 0 10px`, `gap 10px`, `padding 10px`, `gap 2px`, `padding 10px 4px`, `gap 6px`, `padding 14px 16px` | 8/12, 4, 12/16 |
| D7 | DB (b2) | Reiter, Adresszeile, Startseite, Leiste | `padding 0 14px`, `gap 10px`, `gap 20px`, `padding 0 10px` | 12/16, 8/12, 16/24 |
| D8 | QS (b3) | Kacheln, Schieber, Fuß, Blatt | `gap 10px` (15×), `padding 14px`, `gap 14px`, `gap 20px`, `padding 10px 12px`, `margin-bottom 10px`, `margin-top 2px`, `gap 2px`, Abstand zum Rand `10px`, `top 44px` | 8/12/16/24 |
| D9 | SD (b4) | Inhalt, Karten, Abzeichen | `padding 20px 32px`, `margin-bottom 14px/10px`, `gap 20px/14px/2px/6px`, `padding 14px 16px`, `padding 2px 6px`, `top/inset 6px` | 16/24, 12/16, 4/8 |
| D10 | P (b4) | Blatt, Kacheln | `padding 20px`, `gap 14px` | 16/24, 12/16 |
| D11 | I (b5) | Seitenleiste, Inhalt, Optionen, Zeilen, Fuß | `padding 20px 18px`, `padding 24px 28px`, `padding 14px`, `padding 0 14px`, `padding 0 28px`, `gap 10px` (18×), `margin-bottom 14px`, `margin-top 6px` | 16/24, 24/32, 12/16, 8/12 |
| D12 | S (b5) | Inhalt, Karten, Suchfeld | `padding 22px 28px`, `gap 14px`, `gap 10px` (9×), `gap 6px`, `gap 2px`, `margin-bottom 6px`, `padding 0 14px` | 24/32, 12/16, 8, 4 |

### 1.2 Maße der Hülle

| ID | Datei | Element | gefunden | verlangt (§) |
|---|---|---|---|---|
| D13 | M, QS | Leiste oben | 36 hoch, Knöpfe darin 28 hoch | 32 (§5.1), Ziele ≥ 32 (§3) |
| D14 | M, QS | Dock | 44 + 8 + 10 + 2 Rand = 64 hoch | 56 (§5.1) |
| D15 | alle Fenster | Titelleiste | 40 hoch, Knöpfe 30×30 | Höhe nicht festgelegt; Ziele ≥ 32 |
| D16 | DB | Leiste unten | 56; Reihenfolge `[EN] · Übersicht · Suche · Apps · Papierkorb · Status · Uhr` | §11 Nr. 2: `Übersicht · Dock · Uhr · Status`; Suchknopf, alleinstehendes „EN“ am Anfang nicht vorgesehen; Raster-Knopf und App-Menü fehlen |

### 1.3 Ecken (erlaubt: 8 Elemente · 12 Karten/Fenster · rund nur Schalter/Profilbild)

| ID | Datei | Element | gefunden | verlangt |
|---|---|---|---|---|
| D17 | M, QS | Dock-Behälter | 18 | 12 |
| D18 | QS | Schnelleinstellungen, Kalenderblatt | 16 | 12 |
| D19 | P | Blatt / Kacheln | 18 / 14 | 12 / 12 |
| D20 | M | Dateikacheln (`item`) | 10 | 8 |
| D21 | SD | Bühne, Modus-Kacheln; Abzeichen „الرئيسية“ | 10; 6 | 8 oder 12; 8 |
| D22 | I | Ankreuzfeld | 6 | 8 (oder eigener Baustein-Wert festlegen) |
| D23 | DB | Übersicht-Knopf in Leiste | 10 | 8 |
| D24 | DB, S | Suchfelder/Adresszeile als Pille | 17 / 18 / 22 | 8 („rund“ gilt nur für Schalter, Profilbild) |
| D25 | QS, I | Kalendertag (15), Schrittpunkt (11), Schieber (3/9), Punkt (3) | rund | zulässig nur, wenn „rund“ für diese Bausteine in §3 ergänzt wird |

### 1.4 Schrift (Stufen 12·14·16·20·24·32; Arabisch: Naskh/Kufi, Zeilenhöhe 1,6)

| ID | Datei | Element | gefunden | verlangt |
|---|---|---|---|---|
| D26 | alle | Schriftfamilie | `Noto Sans Arabic` + `Inter`, per Google-Fonts-Link aus dem Netz | Noto Naskh Arabic (Lesetext), Noto Kufi Arabic (Titel), Noto Sans (Latein); offline |
| D27 | alle | Zeilenhöhe Grundtext | 1.5 (DB Uhr 1.3, SD Zahl 1) | 1,6 für Arabisch |
| D28 | alle | Knöpfe, Auswahl, Seitenleisten, Kacheltitel, Listen | 13px (M 12×, DB 5×, QS 14×, SD 31×, I 27×, S 23×) | 12 oder 14 |
| D29 | QS, SD, I | Kachel-Untertitel, Kalenderkopf, Meldungs-Kopf, Bildschirmname, Schrittzahl, „موصى به“ | 11px (QS 18×, SD 6×, I 17×) | 12 (kleinste Stufe) |
| D30 | SD | Abzeichen „الرئيسية“ | 10px | 12 |
| D31 | QS | Überschriften „الإشعارات“, „نوفمبر 2026“ | 15px | 14 oder 16 |
| D32 | SD | Bildschirmnummer in Bühne / Nummern-Chip | 26px / 18px | 24 / 16 oder 20 |
| D33 | DB | „Ultar Browser“ Startseite | 22px | 20 oder 24 |
| D34 | P | Titel „وضع العرض“ | 16 ✓, aber Kachelbeschriftung erbt 14 ✓ | – (in Ordnung) |

### 1.5 Farbrollen, Schatten, Symbolstrich

| ID | Datei | Element | gefunden | verlangt |
|---|---|---|---|---|
| D35 | S | 6 goldene Hauptknöpfe „Install“ auf einer Fläche | 6× Akzent | Gold nur für *die eine* Hauptaktion → Karten-Knopf „normal“, Hauptaktion nur auf App-Seite |
| D36 | S | alle 9 Kartensymbole in `acctext` (Gold) | Schmuck in Akzent | Akzent nur Hauptaktion / „an“ |
| D37 | QS | Meldungssymbole in `acctext`; „مسح الكل“ in `acctext` | Akzent als Schmuck/Verweis | neutral (Text-leise) bzw. leiser Knopf |
| D38 | SD | Rahmen der Rückfrage-Meldung gold; Nummern-Chip „1“ gold | Akzent als Schmuck | Linie / neutral |
| D39 | I | Hinweis „موصى به لهذا الجهاز“ in `acctext` | Akzent für Hinweis | Rolle Info (Blau) + Symbol Kreis-i |
| D40 | gen.py `APPCOL` | 16 feste Farbwerte außerhalb der Rollen, themenunabhängig; auch für Datei-/Komponentenlisten benutzt (M, I) | freie Hex-Werte | §3 „Rollen statt Werte“; App-Familie nach §4 braucht je Thema eigene Werte (im hellen Thema 1,7–2,7:1, siehe A-Tabelle) |
| D41 | SD (Modus-Kacheln), P, SD (Bildschirm gewählt), QS (heute), DB (Schild grün) | Zustand „gewählt/an“ nur über Farbe/Rahmenfarbe | nur Farbe | „Farbe trägt nie allein die Bedeutung“ → Haken/gefülltes Symbol dazu |
| D42 | gen.py | Schatten | 4 Stufen: `0 16 40 /.35`, `0 8 24 /.28`, `0 16 40 /.4`, `0 12 32 /.45` | eine Schattenstufe |
| D43 | gen.py `ic` | Strichstärke | 1.6 (Standard), 1.7 (Dock), 1.2 (Dateien 44), 2/2.4/2.6 (Stern/Haken), 2.2 (Marke) | 1,5 bei 24 |
| D44 | alle | Symbolgrößen | 10·12·13·14·15·16·17·18·19·20·22·24·26·44 | 16 und 24 |
| D45 | – | Rollen Info, Warnung, Gefahr | in keinem Bild benutzt; Palette hat keine „Warnung (Bernstein)“ und keinen „hohen Kontrast“ | §3: 10 Rollen, Hell + Dunkel + hoher Kontrast |

### 1.6 Symboltabelle (§4: ein Symbol = eine Bedeutung)

**Ein Symbol mit mehreren Bedeutungen**

| ID | Symbol (gen.py) | Bedeutung laut §4 | im Entwurf benutzt für |
|---|---|---|---|
| D46 | `grid` (9 Punkte) | alle Apps | Dock „alle Apps“ ✓ · SD-Bereich „التطبيقات“ · S „Installed“ · S-Karte „Android apps“ |
| D47 | `shield` | Sicherheit | SD „الخصوصية والأمان“ ✓ · QS-Kachel VPN (§4 verlangt Schild-mit-Schlüssel) · S-Karte „Containers and server“ · DB Tracking-Schutz |
| D48 | `update` (Kreispfeile) | neu laden | DB „Reload“ ✓ · SD/S „Updates“ · QS-Meldung „التحديثات“ (Bedeutung „Updates“ steht nicht in §4) |
| D49 | `star` | Favorit | S „Discover“ · SD Abzeichen „Hauptbildschirm“ – beides nicht „Favorit“ |
| D50 | `file` | (Datei-Symbol Text) | Dock-App Texteditor · Ordner „المستندات“ · Dateien .docx **und** .xlsx (§4 verlangt Art-Symbole Text/Tabelle getrennt) · Komponente Büro (I, S) |
| D51 | `box` | – (nicht in §4) | Komponente „Linux-Programme“ (I, S) · S-Bereich „Ultar components“ |
| D52 | `window` | – (nicht in §4) | SD-Bereich „سطح المكتب والنوافذ“ · Komponente „Windows-Programme“ (I, S) |
| D53 | `globe` | – (App-Symbol) | Ultar Browser (Dock) · Firefox (I) – zwei Programme, ein Symbol |
| D54 | `chev` | Pfeil-Anfang/-Ende = zurück/weiter | zurück/weiter ✓ · um 90° gedreht als Auswahl-Pfeil (SD) – dritte Bedeutung, nicht in §4 |
| D55 | `bell` | Benachrichtigungen (durchgestrichen = Nicht stören) | SD ✓ · QS-Kachel „عدم الإزعاج“ ohne Durchstreichung |
| D56 | `lock` | gesperrt | QS „قفل“ ✓ · DB Adresszeile = sichere Verbindung (zweite Bedeutung) |
| D57 | `trash` | löschen (Aktion) | Dock/Seitenleiste = Ort „Papierkorb“ (in §5.1 so gewollt → §4 um „Ort Papierkorb“ ergänzen) |
| D58 | `download`, `music`, `image`, `usb` | Aktion bzw. – | Orte in Dateien **und** Aktion/Komponente/Quelle (DB Downloads, S Media, S Offline sources) – in `icons.toml` festlegen |

**Zwei Symbole für eine Bedeutung:** keines gefunden.
**Nicht in §4 eingetragen, aber benutzt:** `shot` (Bildschirmfoto), `palette` (Aussehen), `usb`, `box`, `window`, `code`, `music`, `image`, `globe`, `terminal`, `file`, Auswahl-Pfeil, Zwei-Bildschirm-Symbole (`two`).

### 1.7 Inhalt gegen Spezifikation (Auffälligkeiten)

| ID | Datei | Fund | verlangt |
|---|---|---|---|
| D59 | M | Terminal-Fenster offen, Dock-Kachel Terminal ohne „läuft“-Punkt; Einstellungen mit Punkt, aber ohne Fenster | Punkt = läuft (§5.1) |
| D60 | M, QS | USB-Stick eingehängt (Schreibtisch-Symbol, Meldung), Dock zeigt ihn nicht | §5.1 Dock-Ende: eingehängte Datenträger |
| D61 | QS | Kalenderblatt und Schnelleinstellungen gleichzeitig offen, überlappen 10 px (Blatt ab x=380, Tafel bis x=390) | je ein Auslöser, eine Fläche |
| D62 | QS | App-Menü als leerer Knopf gerendert | „Leer, wenn keine App aktiv“ = kein Element |
| D63 | QS | Blatt ohne heutige Termine, ohne Schalter „Nicht stören“, ohne Monatswechsel; Tafel ohne „Dunkel“, „Energiemodus“ als Kachel, ohne Ausgabegerät-Wahl am Lautstärke-Schieber | §5.1 |
| D64 | SD | Seitenleiste 16 Bereiche; „سطح المكتب والنوافذ“ fasst zwei Bereiche zusammen; es fehlen (ohne Hardware-Bedingung): Suche, Drucker & Scanner, Konten, Benutzer, Region & Sprache, Datum & Zeit, Speicher, Sicherung, System/Info, Farbe; außerdem Verbund/Helfer, obwohl QS „الترابط“ aktiv und I den Helfer gewählt zeigt | §8 |
| D65 | SD | je Bildschirm fehlen: Helligkeit, Übergröße-Ausgleich; allgemein: Nachtlicht-Plan/Wärme, Deckel-Verhalten; bei 3 Bildschirmen keine Wahl „welche spiegeln“/„welcher bleibt an“ | §8 Anzeige |
| D66 | SD | Rückfrage zeigt 12 s (als laufender Zähler plausibel) | 15 s – im Bild Startwert zeigen oder Zähler kenntlich machen |
| D67 | I | Firefox angehakt, „برامج Linux“ nicht | §11: Firefox läuft über Komponente „Linux-Programme“ (Abhängigkeit muss sichtbar/automatisch sein) |
| D68 | I, S | Liste unvollständig: I ohne Kommunikation (obwohl Text „…والتواصل“), Android, Container & Server, Fernzugriff; S ohne Kommunikation, Fernzugriff, Firefox | §7.2 |
| D69 | I, S | Platzhalter sichtbar: `[الحجم]`, `[المساحة]`, `[size]` | echte bzw. Beispielgrößen (§6 Nr. 8, §7.3) |
| D70 | S | Bereich „Ultar components“ als eigener Seitenleistenpunkt; „Datei installieren“ fehlt; Karten ohne Weg zur App-Seite, ohne „Öffnen/Entfernen“ bei Installiertem | §7.3 |
| D71 | canvas.json | verweist auf `Artboard-9ohh.dc.html` (Seite 2), Datei fehlt; `support.js` fehlt im Ordner | – |
| D72 | Spez. §5.1 | Zeichnung „Sa 22 Nov“ – der 22.11.2026 ist ein Sonntag (Entwurf „الأحد“ ist richtig) | Spezifikation berichtigen |

---

## Rolle 2 – TESTER

### 2.1 Jedes klickbare Element

„teilweise“ = Element ist in der Spezifikation genannt, der Klick selbst aber nicht ausgeschrieben.

**Main (37)**

| Bild | Element (Beschriftung) | in Spez.? | festgelegtes Verhalten |
|---|---|---|---|
| M | Übersicht „نظرة عامة“ | ja §5.1 | Klick/Super → Übersicht, keine zweite Bedeutung |
| M | App-Menü „الملفات“ | ja §5.1 | Menü: Neues Fenster, Über, Einstellungen der App, Beenden |
| M | Datum/Zeit „الأحد 22 نوفمبر · 16:52“ | ja §5.1 | Blatt: Kalender, Hijri, Termine, Meldungen, Nicht stören, Alle löschen |
| M | Tastaturbelegung „ع“ (eigener Knopf) | nein | **UNDEFINIERT** – §5.1 kennt sie nur als Statussymbol (Klick auf Status = Schnelleinstellungen); Umschalten/Menü per Klick nicht beschrieben |
| M | Status (WLAN·Ton·Akku) „الإعدادات السريعة“ | ja §5.1 | öffnet Schnelleinstellungen |
| M | Schreibtisch-Symbol „المجلد الرئيسي“ | teilweise §5.1 | **UNDEFINIERT** – Einfach-/Doppelklick, Auswahl, Rechtsklick auf Symbol nicht beschrieben |
| M | Schreibtisch-Symbol „ذاكرة USB“ | teilweise §5.1 | **UNDEFINIERT** (wie oben; Auswerfen?) |
| M | Dateien: „تصغير“ / „تكبير“ / „إغلاق“ | ja §5.2, §4 | verkleinern / maximieren / schließen |
| M | Dateien: „رجوع“ (Titelleiste) | teilweise §4 | Bedeutung „zurück“; Verlauf in Dateien (§9) nicht beschrieben; „vor“ fehlt |
| M | Terminal: „تصغير“ / „تكبير“ / „إغلاق“ | ja §5.2 | wie oben |
| M | Seitenleiste: „المجلد الرئيسي“, „المستندات“, „التنزيلات“, „الصور“, „الموسيقى“ | teilweise §9 | „Seitenleiste (Orte …)“ genannt; Klick = Ort öffnen nicht ausgeschrieben |
| M | Seitenleiste: „ذاكرة USB“ | teilweise §9 | Datenträger genannt; Auswerfen-Knopf fehlt/**UNDEFINIERT** |
| M | Seitenleiste: „سلة المحذوفات“ | teilweise §9 | „Papierkorb mit Wiederherstellen“ |
| M | „بحث“ (Lupe) | teilweise §9, §4 | Suche Name+Inhalt; Ablauf (Feld wo?) nicht beschrieben |
| M | „مجلد جديد“ (Plus) | teilweise §4 | Plus = neu; „Neuer Ordner“ in §9 für Dateien nicht genannt (nur Schreibtisch-Rechtsklick §5.1) |
| M | Ordner „مشاريع“, „عقود“, „فواتير“ | nein | **UNDEFINIERT** – Öffnen per Einfach- oder Doppelklick, Auswahl |
| M | Dateien „تقرير.docx“, „ميزانية.xlsx“, „مخطط.png“ | teilweise §7.2, §9 | fehlt die App → Installationsangebot (§7.2); Klickart **UNDEFINIERT** |
| M | Dock „كل التطبيقات“ | ja §5.1 | App-Raster |
| M | Dock-Apps „الملفات“, „المتصفح“, „الطرفية“, „محرر النصوص“, „متجر Ultar“, „الإعدادات“ | ja §5.1 | starten / Fenster holen / Vorschau bei mehreren; Mittel-, Rechtsklick, Ziehen |
| M | Dock „سلة المحذوفات“ | teilweise §5.1 | steht am Ende; Klick (öffnet Papierkorb in Dateien?) und Ablegen **UNDEFINIERT** |

**DesktopBottom (23)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| DB | „Minimize“ / „Maximize“ / „Close“ | ja §5.2 | wie oben |
| DB | Reiter „New tab“, Reiter „code-craft.tech“ | teilweise §9 | „Reiter“ genannt; Wechsel/Schließen (kein Schließen-Kreuz) nicht beschrieben |
| DB | „New tab“ (Plus) | teilweise §4, §9 | neuer Reiter |
| DB | „Back“ / „Forward“ / „Reload“ | ja §4 | zurück / weiter / neu laden |
| DB | „Tracking protection: on“ (Schild) | teilweise §9, §11 | Schutz genannt; Klick (umschalten? Tafel je Seite?) **UNDEFINIERT** |
| DB | „Downloads“ | teilweise §9 | Klick (Liste/Tafel) **UNDEFINIERT** |
| DB | Tastaturbelegung „EN“ | nein | **UNDEFINIERT** (siehe M); Platz am Anfang der Leiste nicht in §11 Nr. 2 |
| DB | „Overview“ | ja §5.1, §11 | Übersicht |
| DB | „Search“ (Feld-Knopf in der Leiste) | nein | **UNDEFINIERT** – Suche liegt laut §5.1 in der Übersicht; §11 Nr. 2 nennt kein Suchfeld |
| DB | Apps „Files“, „Browser“, „Terminal“, „Text editor“, „Ultar Store“, „Settings“ | ja §5.1 | wie Dock |
| DB | „Trash“ | teilweise §5.1 | wie M |
| DB | Status „Quick settings“ | ja §5.1 | Schnelleinstellungen |
| DB | Uhr „16:52 / Sun 22 Nov“ | ja §5.1 | Kalenderblatt |
| DB | *kein Knopf im HTML:* Adresszeile, Schloss, Startseiten-Suchfeld | teilweise §11 | „Adresszeile mit Suche“; Schloss/„Rechte je Seite“ **UNDEFINIERT** |

**QuickSettings (29)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| QS | „نظرة عامة“, Datum/Zeit, Status, „ع“ | wie M | wie M („ع“ **UNDEFINIERT**) |
| QS | App-Menü (leer, ohne Text) | nein | **UNDEFINIERT** – laut §5.1 dürfte es nicht da sein |
| QS | „مسح الكل“ | ja §5.1 | „Alle löschen“ (Rücknahme nicht beschrieben) |
| QS | Kachel „Wi-Fi · KM-Office“ | teilweise §5.1 | „WLAN (Netz wählen)“ – welcher Klick schaltet, welcher öffnet die Netzliste: **UNDEFINIERT** (Kachel hat nur eine Klickfläche) |
| QS | Kachel „بلوتوث · متوقف“ | teilweise §5.1 | „Bluetooth (Gerät wählen)“ – wie WLAN **UNDEFINIERT** |
| QS | Kachel „الشبكة السلكية · متصلة“ | teilweise §5.1 | genannt; Klick **UNDEFINIERT** (Kabel trennen? Einstellungen?) |
| QS | Kachel „VPN · متوقف“ | teilweise §5.1 | an/aus; welches VPN bei mehreren **UNDEFINIERT** |
| QS | Kacheln „الضوء الليلي“, „عدم الإزعاج“, „وضع الطيران“ | teilweise §5.1 | „Kacheln mit Zustand“ – Umschalten naheliegend, nicht ausgeschrieben |
| QS | Kachel „الترابط · جهازان“ | teilweise §5.1 | an/aus naheliegend; Geräteliste? **UNDEFINIERT** |
| QS | Schieber „مستوى الصوت“ | ja §5.1 | Lautstärke (Ausgabegerät-Wahl fehlt im Bild) |
| QS | Schieber „السطوع“ | ja §5.1 | Helligkeit |
| QS | „87% · متوازن“ | ja §5.1 | Akku-Stand → Energie |
| QS | „لقطة شاشة“ | ja §5.1, §5.3 | Auswahl Bereich/Fenster/Bildschirm |
| QS | „الإعدادات“ | ja §5.1 | Einstellungen |
| QS | „قفل“ | ja §5.1 | Sperren |
| QS | „إيقاف التشغيل“ | ja §5.1 | Ein/Aus-Menü: Bereitschaft, Neustart, Ausschalten, Abmelden, Benutzer wechseln |
| QS | Dock (8 Knöpfe) | wie M | wie M |
| QS | *kein Knopf im HTML:* 35 Kalendertage | nein | **UNDEFINIERT** – Klick auf Tag, Monat wechseln |
| QS | *kein Knopf im HTML:* 3 Meldungskarten; Aktionen „فتح · إخراج“, „فتح“ | ja §5.3 | Aktionen direkt (Öffnen · Auswerfen) – im Bild nur Text, nicht klickbar; Klick auf Karte, einzelnes Schließen **UNDEFINIERT** |

**SettingsDisplay (38)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| SD | „تصغير“ / „تكبير“ / „إغلاق“ | ja §5.2 | wie oben |
| SD | Bereiche „Wi-Fi“, „الشبكة“, „بلوتوث“, „الشاشات“, „الصوت“, „الطاقة“, „المظهر“, „الإشعارات“, „التطبيقات“, „الفأرة ولوحة اللمس“, „لوحة المفاتيح“, „تسهيلات الاستخدام“, „الخصوصية والأمان“, „التعريفات والعتاد“, „التحديثات“ | ja §8 | öffnet den Bereich |
| SD | Bereich „سطح المكتب والنوافذ“ | teilweise §8 | §8 hat zwei Bereiche („Arbeitsfläche & Dock“, „Fenster & Arbeitsflächen“) – Zuordnung **UNDEFINIERT** |
| SD | „تعريف الشاشات“ | ja §8 | Nummer groß auf jedem Bildschirm (Dauer nicht genannt) |
| SD | „كشف“ | teilweise §8 | „Erkennen“ genannt, Wirkung/Rückmeldung nicht beschrieben |
| SD | Bildschirm-Rechtecke „2 Dell 24″“, „1 المدمجة 15.6″“, „3 Dell 24″“ | teilweise §8 | Ziehen = anordnen (rastet ein, „Behalten?“); Klick = auswählen steht nur im Bildtext, nicht in §8 |
| SD | Modus „توسيع“ | ja §8 | Erweitern, „Behalten?“ 15 s |
| SD | Modus „تكرار (مرآة)“ | teilweise §8 | Spiegeln; bei 3 Bildschirmen „wählbar, welche“ – Bedienung **UNDEFINIERT** |
| SD | Modus „شاشة واحدة فقط“ | teilweise §8 | Nur ein Bildschirm – welcher bleibt an: **UNDEFINIERT** |
| SD | Schalter „الضوء الليلي“ | ja §8 | an/aus (Plan, Wärme im Bild nicht erreichbar) |
| SD | Schalter „تذكّر أماكن النوافذ“ | ja §8 | Fensterlage je Bildschirm merken |
| SD | Auswahl „إظهار الشريط والمرسى: على كل الشاشات“ | ja §8 | Haupt- oder alle Bildschirme |
| SD | Schalter „الشاشة الرئيسية“ | teilweise §8 | einschalten = Hauptbildschirm; **Ausschalten UNDEFINIERT** (wer wird dann Haupt?) |
| SD | Auswahl „الدقّة“ | ja §8 | Auflösung, empfohlene markiert, „Behalten?“ 15 s |
| SD | Auswahl „حجم العناصر“ | ja §8, §5.2 | 100–300 % in 25er-Schritten |
| SD | Auswahl „معدّل التحديث“ | ja §8 | Bildwiederholrate |
| SD | Auswahl „الاتجاه“ | ja §8 | quer/hoch/gedreht |
| SD | Auswahl „ملف الألوان“ | ja §8 | Farbprofil |
| SD | „تراجع“ / „احتفاظ“ | ja §8 | zurücknehmen / behalten; ohne Antwort Rücknahme nach 15 s |
| SD | *kein Knopf im HTML:* Suchfeld „ابحث في الإعدادات“ | ja §8 | Suche über Bereiche |

**Projection (4)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| P | „هذا الحاسوب فقط“, „تكرار“, „توسيع“, „الشاشة الثانية فقط“ | teilweise §8, §5.4 | vier Modi genannt; Mausklick (sofort oder erst Enter?), Esc, Zeitablauf, „Behalten?“ nach Wahl nicht beschrieben; „zweiter Bildschirm“ bei 3 Bildschirmen **UNDEFINIERT** |

**Installer (13)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| I | Option „الأساسي“ / „الموسّع“ / „اختيار مخصّص“ | ja §6 Nr. 8 | Grundsystem (Standard) / Erweitert / Eigene Auswahl → Komponentenliste |
| I | Ankreuzfelder „المكتب“, „المبرمج“, „الوسائط“, „الترابط“, „المساعد والذكاء المحلي“, „برامج Linux“, „Firefox“, „برامج Windows“ | teilweise §6 Nr. 8, §7.2 | an/abwählen, Summe ändert sich; Verhalten bei Abhängigkeit (Firefox ↔ Linux-Programme), bei zu wenig Platz **UNDEFINIERT** |
| I | „رجوع“ | nein | **UNDEFINIERT** – §6 beschreibt Schritte, nicht Zurück/Weiter (bleiben Eingaben erhalten?) |
| I | „التالي“ | nein | **UNDEFINIERT** – wann gesperrt (Platz reicht nicht)? |
| I | *kein Knopf im HTML:* 16 Schritte der Seitenleiste | nein | Sprung zu erledigten Schritten **UNDEFINIERT**; kein „Abbrechen/Beenden“ im Bild |

**Store (15)**

| Bild | Element | in Spez.? | Verhalten |
|---|---|---|---|
| S | „Minimize“ / „Maximize“ / „Close“ | ja §5.2 | wie oben |
| S | „Discover“ | ja §7.3 | Entdecken |
| S | „Ultar components“ | teilweise §7.3 | laut §7.3 Teil von „Entdecken“ (oben), kein eigener Bereich |
| S | „Installed“ | ja §7.3 | Liste, Entfernen, Rechte ändern |
| S | „Updates“ | ja §7.3 | System/Apps getrennt |
| S | „Offline sources“ | ja §7.3 | Stick/Ordner als Quelle |
| S | „Account“ | ja §7.3 | Anmeldung optional |
| S | „Install“ ×6 (Media, Mesh, Linux apps, Windows apps, Android apps, Containers and server) | teilweise §7.3 | Installieren → Fortschritt → Öffnen · Entfernen ist für die App-Seite beschrieben; direkt auf der Karte (ohne Rechte-Anzeige, ohne Bestätigung) **UNDEFINIERT** |
| S | *kein Knopf im HTML:* 9 Karten, Suchfeld, „Installed“-Marke | teilweise §7.3 | Karte → App-Seite naheliegend, nicht klickbar; Entfernen bei Installiertem fehlt |

Summe: 159 Bedienelemente im HTML (M 37 · DB 23 · QS 29 · SD 38 · P 4 · I 13 · S 15) + 8 Gruppen, die klickbar sein müssten, es im HTML aber nicht sind.

### 2.2 Fehlende Zustände

| Bild | vorhanden | fehlt |
|---|---|---|
| M | normal | leerer Ordner, lädt (Ordner/Netzlaufwerk), Fehler (kein Zugriff), ausgewählte Datei, Kopier-Fortschritt/Abzeichen am Dock, keine App aktiv, Fenster ohne Antwort (§5.2), Schweben/gedrückt/Fokus/deaktiviert für alle Bausteine |
| DB | normal (leerer Reiter) | Seite lädt, Ladefehler/offline, „In Firefox öffnen“ (§11), unsichere Verbindung, Download läuft, privates Fenster |
| QS | an/aus der Kacheln | keine Meldungen (Leerzustand), kein WLAN-Gerät/kein Akku/kein Bluetooth (Kachel entfällt – als Variante zeigen), WLAN verbindet/Fehler, offline, Flugmodus an (abhängige Kacheln gesperrt), Aufnahme läuft (Pflichtanzeige §5.1), Mikrofon-Schieber, Nicht stören an (durchgestrichene Glocke), Akku lädt/niedrig, keine Termine |
| SD | „HDR nicht unterstützt“ (deaktiviert), Rückfrage | nur ein Bildschirm (Anordnung/Modus entfallen), Bildschirm wird erkannt (lädt), Erkennen ohne Ergebnis, Modus/Auflösung fehlgeschlagen, Spiegeln aktiv (Bühne), deaktivierte Auswahl (z. B. feste Wiederholrate), Suche ohne Treffer |
| P | Auswahl | nur ein Bildschirm (Kacheln gesperrt oder Fläche erscheint nicht), Umschalten läuft, Fehler |
| I | gewählt/nicht gewählt, erledigte Schritte | zu wenig Platz (Summe > frei, „Weiter“ gesperrt), Komponente für dieses Gerät ungeeignet (Helfer: RAM/GPU zu klein, §7.2), Komponente nicht im Medium/offline, Abhängigkeit, lädt (Größen werden berechnet), Fehlerzustand |
| S | installiert / installierbar | lädt, offline (zeigt Installiertes + Offline-Quellen), Fehler mit Ursache, zu wenig Platz, Installation läuft (Fortschritt), Aktualisierung verfügbar, Entfernen, Suche ohne Treffer, ungeprüfter Herausgeber – die vier ersten verlangt §7.3 ausdrücklich |

---

## Rolle 3 – ARABISCH / RTL

### 3.1 Richtung: physisch links/rechts

| ID | Datei (Quelle) | Fundstelle | Wirkung | Korrektur |
|---|---|---|---|---|
| R1 | M (b1 Zeile `back = …`) | „رجوع“: `chev` (zeigt nach rechts) wird mit `scaleX(-1)` gespiegelt → zeigt in der arabischen Fläche nach **links** | falsch: in RTL zeigt „zurück“ zum Anfang = nach rechts; der Pfeil bedeutet so „weiter“ | Spiegelung an `dir` binden (in RTL ungespiegelt), nicht fest im Aufruf |
| R2 | SD (b4 `stage`,`mon`) | Bühne `direction: ltr`, Bildschirme mit `left:` | gewollt (echte Lage der Bildschirme) – richtig; aber Abzeichen „الرئيسية“ mit `inset-inline-start` landet dadurch links | Abzeichen an der RTL-Anfangsseite (rechts) oder mittig |
| R3 | S (b5 `side`) | `border-right` | bricht beim Spiegeln (arabischer Store) | `border-inline-end` wie in M/SD/I |
| R4 | alle (gen.py `window`) | Fenster mit `left: x` | physisch; für Fensterlage vertretbar, Compositor muss Startlage in RTL spiegeln | festlegen in §5.2 |
| R5 | alle (gen.py `tile`; b3 `sheet`) | `left: 50%; margin-left: -2.5px / -340px` | nur Zentrierung, unschädlich | `inset-inline-start` + `translate` oder Flex-Zentrierung |
| R6 | alle (gen.py `topbar`,`dock`) | `left: 0; right: 0` | symmetrisch, unschädlich | `inset-inline: 0` |
| R7 | M (b1 `term`) | `direction: ltr; text-align: left` | gewollt (Terminal) – richtig | – |
| R8 | QS (b3 `slider`) | Füllung/Griff über `inset-inline-start` | richtig: Schieber füllt in RTL von rechts | – |
| R9 | alle | Reihenfolge Leiste (Übersicht rechts, Status links), Fensterknöpfe am Ende mit „إغلاق“ ganz außen, „رجوع“ rechts von „التالي“ | richtig | – |

### 3.2 Symbole: spiegeln oder nicht

| ID | Symbol | Stand im arabischen Bild | Soll |
|---|---|---|---|
| R10 | `chev` zurück (M) | gespiegelt (zeigt links) | **nicht** spiegeln in RTL → siehe R1 |
| R11 | Zwei-Bildschirm-Symbole (SD, P): „dieser Rechner“ = linkes Rechteck, Erweitern-Pfeil nach rechts | ungespiegelt | entscheiden und in `icons.toml` eintragen: entweder physisch (dann unabhängig von Sprache, mit echter Anordnung) oder gespiegelt (Rechner = Anfang = rechts, Pfeil nach links) – jetzt widerspricht der Pfeil der Leserichtung |
| R12 | `volume` neben RTL-Schieber (QS) | ungespiegelt (Wellen nach rechts, Schieber wächst nach links) | spiegeln (Wellen zeigen in Schieberichtung) – in §3 ist der Lautsprecher weder als gerichtet noch als ungerichtet genannt |
| R13 | `file` (Eselsohr rechts, Textzeilen), `spark` (Sprechblasen-Spitze links) | ungespiegelt | spiegeln empfohlen (Dokument/Sprechblase haben Leserichtung) – in `icons.toml` festlegen |
| R14 | `terminal` (Zeichen `>`), `update`, `search`, `battery`, `check`, `moon`, `plane`, Marke | ungespiegelt | richtig, nicht spiegeln |
| R15 | Auswahl-Pfeil (`chev` 90°) | zeigt nach unten | richtig |

### 3.3 Gemischter Text und Ziffern

Ziffern: in allen 7 Dateien nur 0–9, keine arabisch-indischen Ziffern – erfüllt.

| ID | Datei | Text | Problem (Unicode-Bidi, Absatz ist RTL) | Korrektur |
|---|---|---|---|---|
| R16 | SD | „3840 × 2160 (موصى بها)“, „3840 × 2160 · متصلة داخلياً“ | zwei Zahlen mit neutralem „×“ werden in RTL getauscht dargestellt: links steht 2160, rechts 3840 | Wert als LTR-Insel: `<bdi dir="ltr">3840 × 2160</bdi>` |
| R17 | SD | „60 Hz“ | Zahl und lateinische Einheit sind getrennte Läufe → Anzeige „Hz 60“ | `<bdi dir="ltr">` oder arabisch „60 هرتز“ |
| R18 | QS | „متصلة · 1 Gb“ | Anzeige „Gb 1“; außerdem Einheit falsch (Gb = Datenmenge) | „متصلة · 1 جيجابت/ث“ |
| R19 | QS, SD | „Wi-Fi“ lateinisch, daneben „بلوتوث“ arabisch umschrieben | uneinheitlich; §10 „kein englischer Rest in Arabisch“ | „واي فاي“ (oder beide lateinisch – eine Regel für Eigennamen der Technik festlegen) |
| R20 | P | „Super + P للتنقّل · Enter للتأكيد“ | Reihenfolge stimmt; Tastennamen als Tasten-Baustein auszeichnen (LTR-Insel), sonst kippen spätere Kürzel wie „Ctrl + Alt + T“ | Baustein „Taste“ mit `dir="ltr"` |
| R21 | M | „تقرير.docx“, „ميزانية.xlsx“, „مخطط.png“ | Darstellung in Ordnung (Endung links); bei Namen mit Ziffern/Latein am Anfang kippt es | Dateinamen grundsätzlich in `<bdi>` setzen |
| R22 | I | „‎.deb‎“, „‎.exe‎ و ‎.msi‎“ | mit LRM richtig gelöst | – |
| R23 | SD | „المدمجة 15.6″“, „Dell 24″“, „200% (موصى به)“, „87% · متوازن“, „حتى 06:10“, „6 عناصر“ | in Ordnung | – |
| R24 | QS | „نوفمبر 2026“, „الأحد 22 نوفمبر“ | für Syrien/Levante sind die Monatsnamen „تشرين الثاني“ üblich (Zielmarkt Syrien: aus dem Projektgedächtnis, nicht aus den Dateien) | Monatsnamen aus „Region & Sprache“; für ar-SY „تشرين الثاني“ |
| R25 | I, S | `[الحجم]`, `[المساحة]` | Platzhalter in eckigen Klammern | Beispielwert mit Einheit festlegen: „1.2 غ.ب“ oder „1.2 GB“ – eine Schreibweise für das ganze System |

### 3.4 Wortwahl – Verbesserungen

| ID | Datei | jetzt | Problem | Vorschlag |
|---|---|---|---|---|
| R26 | SD | „تعريف الشاشات“ (Bildschirme kennzeichnen) | „تعريف“ heißt im selben System „Treiber“ („التعريفات والعتاد“) → wird als „Bildschirm-Treiber“ gelesen | „إظهار أرقام الشاشات“ |
| R27 | SD | „كشف“ | zu knapp, mehrdeutig | „البحث عن شاشات“ |
| R28 | I, (S) | „المكتب“ (Komponente Büro) | kollidiert mit „سطح المكتب“ (Schreibtisch) und „مكتبك“ im Anzeige-Text | „التطبيقات المكتبية“ |
| R29 | SD / P | „تكرار (مرآة)“ gegen „تكرار“ | zwei Namen für einen Modus | überall „تكرار“ |
| R30 | SD / I | „التعريفات والعتاد“ gegen „العتاد والتعريفات“ | Reihenfolge vertauscht | überall „العتاد والتعريفات“ (wie §6 Nr. 7 „Hardware & Treiber“) oder überall umgekehrt |
| R31 | QS | „إيقاف التشغيل“ am Ein/Aus-Knopf | Knopf öffnet ein Menü (Bereitschaft, Neustart …), nicht „Ausschalten“ | „خيارات الطاقة“ |
| R32 | QS | „متوقف“ für aus (Bluetooth, VPN, Flugmodus, Nicht stören) | verständlich, klingt nach „angehalten/gestört“ | „مُطفأ“ für Funk/VPN, „غير مفعّل“ für Modi – oder einheitlich „مُطفأ“ |
| R33 | SD | „متصلة داخلياً“ | unüblich | „شاشة داخلية“ |
| R34 | SD | „التراجع تلقائياً بعد 12 ثانية“ | Nominalstil | „سيُتراجَع عنها تلقائياً خلال 12 ثانية“ oder kürzer „تُلغى تلقائياً خلال 12 ثانية“ |
| R35 | SD | „الشاشة الرئيسية – يظهر عليها الشريط وتُفتح فيها التطبيقات الجديدة“ | gut; §8 nennt zusätzlich den Anmeldebildschirm | „…وشاشة تسجيل الدخول“ ergänzen |
| R36 | I | „المساعد والذكاء المحلي“ | „الذكاء المحلي“ unvollständig | „المساعد والذكاء الاصطناعي المحلي“ |
| R37 | I | „بلا إعادة تثبيت“ | steif | „دون إعادة تثبيت النظام“ |
| R38 | I | „رجوع“ / „التالي“ | Paar uneinheitlich (im Assistenten üblich „السابق / التالي“); „رجوع“ ist auch der Zurück-Pfeil in Dateien | im Installer „السابق“, „رجوع“ nur für Navigation |
| R39 | I | Schritt „انتهى“ | hart | „اكتمل التثبيت“ |
| R40 | QS | Kürzel „ع“ ohne Namen für Sprachausgabe | – | `aria-label="لغة الإدخال: العربية"` |
| R41 | I, QS, SD | „الترابط“ (Verbund) | abstrakt, aber einheitlich benutzt | behalten; im Untertitel erklären („ربط أجهزتك“) – Entscheidung von dir |
| R42 | P / I | „الحاسوب“ gegen „الجهاز/جهازك“ | zwei Wörter für das eigene Gerät | überall „الجهاز“ („هذا الجهاز فقط“) |

Ohne Beanstandung (natürlich und richtig): „نظرة عامة“, „الإعدادات السريعة“, „كل التطبيقات“, „سلة المحذوفات“, „المجلد الرئيسي“, „6 عناصر“, „الضوء الليلي“, „عدم الإزعاج“, „وضع الطيران“, „جهازان“ (Dual richtig), „مسح الكل“, „ترتيب الشاشات“ samt Erklärtext, „توسيع“, „الدقّة (موصى بها)“ / „حجم العناصر (موصى به)“ (Kongruenz richtig), „معدّل التحديث“, „تسهيلات الاستخدام“, „الخصوصية والأمان“, „ما الذي تريد تثبيته؟“, „اختيار مخصّص“, Kalenderkopf „س ح ن ث ر خ ج“ (1.11.2026 = Sonntag, Raster stimmt), „جمادى الآخرة 1448“.

### 3.5 Glossar der benutzten Begriffe

| Deutsch (Spez.) | Arabisch im Entwurf | Bilder | einheitlich? |
|---|---|---|---|
| Übersicht | نظرة عامة | M, QS | ja |
| Schnelleinstellungen | الإعدادات السريعة | M, QS | ja |
| Einstellungen | الإعدادات | M, QS, SD | ja |
| Dateien | الملفات | M, QS | ja |
| Browser | المتصفح / متصفح Ultar | M, QS, I | ja |
| Terminal | الطرفية | M, QS | ja |
| Texteditor | محرر النصوص | M, QS | ja |
| Ultar Store | متجر Ultar | M, QS, I | ja |
| Papierkorb | سلة المحذوفات | M, QS | ja |
| alle Apps | كل التطبيقات | M, QS | ja |
| Apps | التطبيقات | SD, I | ja |
| Programme (Linux/Windows) | برامج | I | ja (bewusst anders als „تطبيقات“ – festhalten) |
| Startordner | المجلد الرئيسي | M | ja |
| USB-Stick | ذاكرة USB | M, QS | ja |
| verkleinern / maximieren / schließen | تصغير / تكبير / إغلاق | M, SD | ja |
| zurück | رجوع | M, I | ja (siehe R38) |
| weiter | التالي | I | – |
| WLAN | Wi-Fi | QS, SD | ja, aber lateinisch (R19) |
| Bluetooth | بلوتوث | QS, SD | ja |
| Kabelnetz / Netzwerk | الشبكة السلكية / الشبكة | QS / SD, I | ja |
| VPN | VPN | QS | – |
| Flugmodus | وضع الطيران | QS | – |
| Nachtlicht | الضوء الليلي | QS, SD | ja |
| Nicht stören | عدم الإزعاج | QS | – |
| Verbund | الترابط | QS, I | ja |
| Lautstärke / Ton | مستوى الصوت / الصوت | QS / SD | ja |
| Helligkeit | السطوع | QS | – |
| Energie / ausgewogen | الطاقة / متوازن | SD / QS | ja |
| Bildschirmfoto | لقطة شاشة | QS | – |
| Sperren | قفل | QS | – |
| Ein/Aus-Menü | إيقاف التشغيل | QS | **ändern** (R31) |
| Benachrichtigungen | الإشعارات | QS, SD | ja |
| Updates | التحديثات / تحديث النظام | QS, SD | ja |
| Bildwiederholrate | معدّل التحديث | SD | ja (gleiches Wort wie „Update“ – üblich) |
| Anzeige / Bildschirm | الشاشات / الشاشة | SD, P | ja |
| Hauptbildschirm | الشاشة الرئيسية / الرئيسية | SD | ja |
| eingebauter Bildschirm | المدمجة / الشاشة المدمجة | SD | ja |
| Erweitern | توسيع | SD, P | ja |
| Spiegeln | تكرار (مرآة) / تكرار | SD / P | **nein** (R29) |
| Nur ein Bildschirm / Nur dieser Rechner / Nur zweiter | شاشة واحدة فقط / هذا الحاسوب فقط / الشاشة الثانية فقط | SD / P | ja |
| Bildschirm-Modus | وضع العرض | P | – |
| Bildschirme kennzeichnen | تعريف الشاشات | SD | **ändern** (R26) |
| Erkennen | كشف | SD | **ändern** (R27) |
| Auflösung / Größe der Elemente / Ausrichtung / Farbprofil | الدقّة / حجم العناصر / الاتجاه / ملف الألوان | SD | ja |
| empfohlen | موصى بها / موصى به | SD, I | ja |
| Leiste / Dock | الشريط / المرسى | SD | ja |
| Arbeitsfläche & Fenster | سطح المكتب والنوافذ | SD | ja |
| Aussehen | المظهر | SD | – |
| Maus & Touchpad | الفأرة ولوحة اللمس | SD | – |
| Tastatur | لوحة المفاتيح | SD, I | ja |
| Barrierefreiheit | تسهيلات الاستخدام | SD, I | ja |
| Datenschutz & Sicherheit | الخصوصية والأمان | SD | – |
| Treiber & Hardware | التعريفات والعتاد / العتاد والتعريفات | SD / I | **nein** (R30) |
| behalten / zurücknehmen | احتفاظ / تراجع | SD | ja |
| Komponente | مكوّن / المكوّنات | I | ja |
| Grundsystem / Erweitert / Eigene Auswahl | الأساسي / الموسّع / اختيار مخصّص | I | ja |
| Büro | المكتب | I | **ändern** (R28) |
| Coder | المبرمج | I | – |
| Medien | الوسائط | I | ja |
| Kommunikation | التواصل | I (nur im Text) | – |
| Helfer & lokale KI | المساعد والذكاء المحلي | I | **ändern** (R36) |
| Zelle / Bereich (abgeschottet) | خلية معزولة / منطقة معزولة | I | bewusst zwei (Zelle ≠ Ubuntu-Bereich) – festhalten |
| Gerät / Rechner | الجهاز / الحاسوب | I, QS / P | **nein** (R42) |
| Platte / Verschlüsselung / Konto / Zeitzone / Zusammenfassung | القرص / التشفير / الحساب / المنطقة الزمنية / الملخّص | I | ja |
| Installation / installieren | التثبيت / تثبيت | I, QS | ja |

Englische Gegenstücke (DB, S) decken nur einen Teil ab; für „Mesh“, „Coder“, „Assistant and local AI“ fehlt die Festlegung, ob das die Produktnamen sind.

---

## Rolle 4 – BARRIEREFREIHEIT

### 4.1 Kontrast (WCAG 2.x, relative Leuchtdichte; Paletten `DARK`/`LIGHT` aus gen.py)

**Text – alle benutzten Paare bestehen (≥ 4,5:1)**

| Thema | Paar (Vorder-/Hintergrund) | wo | Verhältnis |
|---|---|---|---|
| Dunkel | text auf bg / bar / surf / rise / hover | überall | 17,06 / 16,04 / 15,28 / 13,80 / 12,44 |
| Dunkel | mute auf bg / bar / surf / rise / hover | Untertitel, Platzhalter, Schritte, Bildschirmname | 8,66 / 8,14 / 7,76 / 7,01 / 6,32 |
| Dunkel | acctext auf surf / rise / hover | „مسح الكل“, Hinweis, aktive Symbole | 10,28 / 9,29 / 8,37 |
| Dunkel | onacc auf acc | Hauptknopf, aktive Kachel (auch 11px-Untertitel), heute | 8,96 |
| Dunkel | ok auf bg / bar | Terminal-Prompt, erledigter Schritt | 10,26 / 9,64 |
| Hell | text auf bar / surf / rise | überall | 16,87 / 17,93 / 16,30 |
| Hell | mute auf bg / bar / surf / rise | Beschreibung, „EN“, Datum, Platzhalter | 5,61 / 6,30 / 6,69 / 6,08 |
| Hell | acctext auf rise / hover | Kartensymbol, aktives Symbol | 5,69 / 5,10 |
| Hell | ok auf surf | „Installed“, Schild | 5,37 |
| Hell | onacc auf acc | „Install“ | 7,79 |

**Unter der Schwelle**

| ID | Thema | Paar | wo | Verhältnis | Schwelle |
|---|---|---|---|---|---|
| A1 | Hell | App-Farben (`APPCOL`) auf rise `#F3F4F7` | Dock-Kacheln DB: Ordner 1,97 · Globus 2,47 · Terminal 2,70 · Datei 1,96 · Tasche 1,91 · Zahnrad 2,04 · Papierkorb 2,04 | 1,91–2,70 | 3:1 (Symbol ohne Text) |
| A2 | Hell | acc `#C9A227` auf bar / surf | „läuft“-Punkt (2,28), Marke/Übersicht-Knopf (2,28), Marke Startseite (2,42); ebenso jeder „an“-Schalter im hellen Thema | 2,28–2,42 | 3:1 |
| A3 | Dunkel | line `#2E333E` auf surf / rise / bg | Rahmen von Auswahl, Suchfeld, Karten, Bühne; Schieber-Schiene; Schalter „aus“ | 1,38 / 1,24 / 1,54 | 3:1 (Grenze eines Bedienelements) |
| A4 | Hell | line `#D5D9E0` auf surf / rise / bar | Rahmen von Suchfeld, Adresszeile, Karten, Kacheln | 1,42 / 1,29 / 1,33 | 3:1 |
| A5 | Dunkel | Füllung rise auf surf; hover auf rise | Auswahlfeld gegen Karte (1,11); gewählte gegen nicht gewählte P-Kachel bzw. Bildschirm (1,11) | 1,11 | 3:1 für Zustandsunterschied – Rahmen in acc (7,48) trägt allein → siehe D41 |
| A6 | Hell | hover auf bar / surf | aktiver Bereich in S-Seitenleiste (1,15), aktive Zeile | 1,15–1,23 | 3:1; Gewicht 600 + Symbolfarbe helfen, reichen aber nicht sicher |
| A7 | beide | – | Palette „hoher Kontrast“ fehlt | – | §3 verlangt sie |

Hinweis: ok auf bg im hellen Thema wäre 4,50 (Grenzwert), mute auf acc 1,07 / 2,77 – beide Paare kommen in den Bildern nicht vor, dürfen aber nie entstehen (z. B. Untertitel in Text-leise auf goldener Kachel).

### 4.2 Schrift unter der kleinsten Stufe
10px (SD Abzeichen), 11px (QS 18×, SD 6×, I 17×) – siehe D29/D30; bei 1,6 Zeilenhöhe und arabischer Schrift zu klein.

### 4.3 Ziele unter 32 (Maus)

| ID | Datei | Element | Größe |
|---|---|---|---|
| A8 | M, QS | Leiste: Übersicht 32×28, App-Menü, Datum, „ع“, Status | 28 hoch |
| A9 | alle Fenster | Titelleiste: verkleinern/maximieren/schließen, „رجوع“ | 30×30 |
| A10 | DB | „New tab“ (Plus) | 30×30 |
| A11 | QS | „مسح الكل“ | 28 hoch |
| A12 | SD | Schalter | 44×24 |
| A13 | I | Ankreuzfeld (nur das Kästchen ist bedienbar, nicht die Zeile) | 20×20 |
| A14 | QS | Schieber: Schiene 6 hoch, Griff 18×18 | < 32 |
| A15 | QS | Kalendertage 30 hoch (und gar keine Bedienelemente) | 30 |
| – | alle | Touch-Ziel 44: nur Dock-Kacheln (44) und QS-Kacheln (60) erfüllen es | – |

### 4.4 Rollen, Namen, Struktur

| ID | Datei | Fund | Korrektur |
|---|---|---|---|
| A16 | alle | Symbolknöpfe haben `aria-label`, Schmuck-SVG `aria-hidden` – in Ordnung | – |
| A17 | M, QS, DB | „ع“ / „EN“: Name nur ein Buchstabe/Kürzel | `aria-label` „Eingabesprache: …“ |
| A18 | QS | App-Menü: leerer Knopf ohne Name, trotzdem im Fokusweg | entfernen, wenn keine App aktiv |
| A19 | SD, S, DB | Suchfelder und Adresszeile sind `div` ohne Rolle/Name (4×) | Eingabefeld mit `role="searchbox"`/Beschriftung |
| A20 | QS | Meldungsaktionen „فتح · إخراج“ sind Text, keine Knöpfe; Karten ohne Rolle | Knöpfe; Liste mit `role="list"` |
| A21 | QS | Kalender: keine `grid`-Rolle, „heute“ nur farbig, kein `aria-current="date"`, Tage nicht fokussierbar | Raster-Baustein |
| A22 | SD, S, M | aktiver Bereich der Seitenleiste ohne `aria-current`; Seitenleiste ohne `nav` | ergänzen |
| A23 | SD, P | sich ausschließende Wahl mit `aria-pressed` | `radiogroup` + `radio` (wie im Installer) |
| A24 | SD | Schalter „الشاشة الرئيسية“ als `switch` | Aktion/Radio – „aus“ hat keine Bedeutung |
| A25 | SD | Rückfrage ist `role="status"`, enthält aber Knöpfe und läuft ab | `alertdialog`, Fokus hineinsetzen, Restzeit ansagen; Zeitlimit verlängerbar (WCAG 2.2.1) |
| A26 | I | Ankreuzfeld als `span` mit `aria-label`, nicht mit sichtbarem Text/Beschreibung verbunden; Zeile nicht klickbar | ganze Zeile als Ziel, `aria-describedby` für Untertitel/Größe |
| A27 | I | Schrittliste ohne Liste/`aria-current="step"`; „erledigt“ nur als verstecktes Haken-Symbol | Textalternative „erledigt“ |
| A28 | QS | Schieber ohne `aria-valuetext` („62%“), kein `aria-orientation` | ergänzen |
| A29 | alle | keine Überschriften (`h1`/`h2`), keine Bereichsrollen, Fenster ohne Rolle/Name; `dir` am `div` statt an `html` | Struktur in den Bausteinen festlegen |
| A30 | alle | kein Fokusrahmen gezeichnet und kein Wert dafür festgelegt (Farbe, Stärke, Abstand); Zustände schwebend/gedrückt/fokussiert/deaktiviert fehlen in allen Bildern | §3 verlangt „Fokusrahmen immer sichtbar“ → ein Bild „Zustände der Bausteine“ |
| A31 | DB | Schutz „an“ nur über grüne Farbe sichtbar | gefülltes Symbol/Haken |

### 4.5 Fokusreihenfolge (aus der Reihenfolge im HTML)

| ID | Datei | Problem |
|---|---|---|
| A32 | SD | Rückfrage „تراجع/احتفاظ“ steht als letztes Element nach 35 anderen – mit Tastatur in 12–15 s nicht erreichbar |
| A33 | QS | Reihenfolge: Leiste → Kalenderblatt → Schnelleinstellungen → Dock; keine Fokusfalle in der geöffneten Fläche, Rückkehr zum Auslöser nicht festgelegt |
| A34 | M | Hintergrundfenster (Terminal) kommt vor dem aktiven Fenster (Dateien); Schreibtisch-Symbole liegen zwischen Leiste und Fenstern |
| A35 | SD | Bildschirme in Reihenfolge 2 → 1 → 3 (Lage von links), in einer RTL-Fläche gegen die Leserichtung und nicht nach Nummer |
| A36 | I | 3 Optionen sind 3 Tab-Halte (statt einer Gruppe mit Pfeiltasten); danach 8 Kästchen, „رجوع“ vor „التالي“ – Hauptaktion zuletzt ist in Ordnung |
| A37 | alle Fenster | Titelleisten-Knöpfe liegen vor dem Fensterinhalt im Tab-Weg (3–4 Halte je Fenster) – besser nur per Fenstermenü/Kürzel |

---

## Reihenfolge der Korrekturen

### Vor der Freigabe (muss)

| # | Korrektur | Funde |
|---|---|---|
| 1 | Zurück-Pfeil in RTL richtig herum; Spiegelregel an `dir` binden; Spiegel-Entscheidung für Zwei-Bildschirm-Symbole und Lautsprecher | R1, R10–R12 |
| 2 | Zahlen-/Einheiten-Inseln: „3840 × 2160“, „60 Hz“, „1 Gb“ | R16–R18 |
| 3 | Begriffe: „تعريف الشاشات“ → „إظهار أرقام الشاشات“; „المكتب“ → „التطبيقات المكتبية“; „تكرار“ einheitlich; Treiber-Begriff in einer Reihenfolge; Ein/Aus-Knopf; „Wi-Fi“ | R19, R26–R31 |
| 4 | Symbole mit Doppelbedeutung auflösen (`grid`, `shield`, `update`, `star`, `file`, `box`, `window`, `globe`), VPN = Schild-mit-Schlüssel, Nicht stören = durchgestrichene Glocke; fehlende Symbole in §4 eintragen | D46–D58 |
| 5 | Undefinierte Klicks festlegen (Spez. oder Bild ändern): Tastaturbelegung-Knopf, WLAN-/Bluetooth-/Kabel-Kachel, Schalter „Hauptbildschirm“ aus, Modus bei 3 Bildschirmen, Installer Zurück/Weiter/Abbrechen, „Install“ auf der Karte, Suchknopf der unteren Leiste, Datei-/Schreibtisch-Klick | Tabelle 2.1 |
| 6 | Schriftstufen (13 → 14 bzw. 12; 11/10 → 12), Schriftfamilien Naskh/Kufi, Zeilenhöhe 1,6 | D26–D33 |
| 7 | Raster und Ecken auf die festen Werte (10/14/20/28 → 8/12/16/24; Ecken 10/14/16/18 → 8/12); Leiste 32, Dock 56 oder Spez. ändern | D1–D25 |
| 8 | Inhaltliche Widersprüche: Firefox ohne Linux-Programme, fehlende Komponenten, fehlende Einstellungsbereiche, Platzhalter `[الحجم]`, „läuft“-Punkte, leeres App-Menü, überlappende Flächen | D59–D70 |
| 9 | Rückfrage „Behalten?“: Fokus, Rolle, 15 s | A25, A32, D66 |
| 10 | Helles Thema: App-Farben und Akzent ≥ 3:1; Rahmen von Eingabe-Bausteinen ≥ 3:1 in beiden Themen | A1–A6 |

### Sollte (vor dem Bau der Bausteine)

| # | Korrektur | Funde |
|---|---|---|
| 11 | Gold nur für eine Hauptaktion je Fläche (Store 6×), kein Gold als Schmuck; Zustand nie nur über Farbe | D35–D41 |
| 12 | Ziele ≥ 32: Leisten- und Titelleisten-Knöpfe, Schalter, Kästchen (ganze Zeile), Schieber | A8–A15 |
| 13 | Ein Bild „Zustände“: Fokusrahmen, schwebend, gedrückt, deaktiviert, lädt, Fehler; dazu je Fläche Leer/Lädt/Fehler/Offline/ohne Hardware | 2.2, A30 |
| 14 | Rollen/Namen: Suchfelder, Meldungsaktionen, Kalender, `aria-current`, `radiogroup`, Überschriften | A17–A29 |
| 15 | Eine Schattenstufe, Strich 1,5, Symbolgrößen 16/24 | D42–D44 |
| 16 | Untere-Leiste-Variante nach §11 Nr. 2 ordnen (Raster-Knopf, App-Menü, Reihenfolge Uhr · Status) | D16 |
| 17 | Übrige Wortwahl (R32–R42), Monatsnamen nach Region | R24, R32–R42 |

### Später

| # | Korrektur | Funde |
|---|---|---|
| 18 | Palette „hoher Kontrast“, Rolle Warnung; Rollen Info/Gefahr im Entwurf zeigen | D45, A7 |
| 19 | Größenklassen kompakt/mittel und Gegenstücke hell/dunkel × arabisch/englisch je Fläche (§2.2 Schritt 3) | – |
| 20 | Physische `left/right`-Reste im Generator, `border-right` im Store, `dir` an `html` | R3–R6, A29 |
| 21 | canvas.json aufräumen (fehlendes Bild, `support.js`), Schriften nicht aus dem Netz laden; „Sa 22 Nov“ in der Spez. berichtigen | D26, D71, D72 |
| 22 | Fokusweg: Titelleisten-Knöpfe, Fensterreihenfolge, Gruppen mit Pfeiltasten | A33–A37 |
