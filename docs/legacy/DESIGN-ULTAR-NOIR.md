# Ultar Noir – Designsprache der Oberfläche

Umsetzung des Entwurfs „Ultar OS – Redesign 1.0“ in `ultar-shell` (Rust, Software-Rendering).

| Baustein | Datei | Inhalt |
|---|---|---|
| Farben | `ui.rs` (`theme`, `ACCENTS`, `KM_RED`) | Grund #0E1014→#08090B, Fläche #14161B, Linie #2A2D35, Text #F2F0EA; Akzent = Logo-Gold #D8C870 (Verlauf #F0F090 → #B08838), KM-Rot #F80000 nur für Marke/Alarm. Hell: Gold abgedunkelt für Kontrast |
| Grafik | `gfx.rs` | `sdf_vgrad` (Form mit Verlauf), `ellipse` (auch gedreht), `dot_screen` (Punktmatrix über Text) |
| Startbildschirm | `app_layout.rs` (`s_home`) | Uhr als Punktmatrix, Logo + Suche rechts, Datum/Hijri, Verbindungskarte, App-Raster in **Seiten** (wischen, Punkte) – endet über dem **Dock** (Telefon, Nachrichten, Browser, Kamera); nichts liegt mehr unter dem Dock |
| Kacheln | `app_layout.rs` (`app_tile`), `app.rs` (`s_home_classic`) | neutrale Glas-Kacheln, Symbol in Schriftfarbe; Gold nur für Agent/Store, Telefon grün |
| KM-Helfer | `app_buddy.rs` | eigene Figur (kein Logo): Creme-Körper mit Verlauf, zwei Fühler mit Lichtern, Augen mit Glanz, Bäckchen, schwebende Hände, Brust-Licht in Zustandsfarbe. Zustände: fröhlich (^ ^, winkt – Anruf/Führung), denkt (Hand am Kinn, Denkblasen), spricht (Mund + Hände bewegt), Gefahr (strenger Blick, „Stopp“-Hände, rot), müde/schläft (Augen zu, „z“), blinzelt, schaut zur Berührung |
| Helfer-Platz | `app_buddy.rs` | Startbildschirm: oben rechts neben der Uhr; sonst schaut er nur halb vom Rand herein (Wählfeld/Rechner/Kamera weiter oben) – verdeckt keine Bedienelemente mehr. Gezogen bleibt er, wo man ihn hinsetzt |

Geprüft mit `ultar-shell --shot` in 1080×2340, 720×1520, 480×800 und 1280×800, Deutsch und Arabisch; `cargo test -p ultar-shell` grün.
Web-Demo derselben Gestaltung: https://code-craft.tech/products/ultar-os/demo/index.html

## 1:1 mit der Web-Demo (Stand 20.11.2026)

Die Web-Demo (code-craft.tech/products/ultar-os/demo) ist die Vorlage für das Gerät. Umgesetzt in `ultar-shell`:

- Gemeinsame Bausteine wie die Demo-CSS: Kopfzeile (`.hd`), Karten (`.card`), gruppierte Listen (`.list/.row`, farbige Symbolfelder), Abschnitte (`.sec`), Reiter (`.tabs`), Pillen (`.pill`), Knöpfe (`.btn`), Felder (`.inp`), Schalter, Blatt von unten (`sheet()`) und mittiges Fenster.
- Bildschirmtastatur: fokussiertes Feld wird immer über die Tastatur geholt; Seiten ohne eigenes Rollen werden über der Tastatur rollbar; Tasten hinter der Tastatur lösen nicht aus; Helfer weicht aus.
- Startbildschirm, Sperrbildschirm (mit PIN-Ansicht), Schnelleinstellungen, Zuletzt verwendet.
- Apps: Telefon, Nachrichten, Kontakte (+ Kontaktseite, Blatt „Neuer Kontakt"), Rechner, Uhr, Dateien, Galerie, Notizen (+ Notizseite), Kalender (+ Blatt „Neuer Termin"), Werkzeuge (+ Hijri-Datum), Einstellungen, Datenschutz-Zentrum, Ultar Store, Terminal, Monitor, Suche, Browser-Startseite, Kamera-Leiste, WhatsApp/YouTube/Spotify-Seiten, Telegram-Liste, KM-Helfer als App.
- Bewusste Abweichungen: Hilfe-Knopf (ⓘ) in der Kopfzeile bleibt; Daten sind echt (keine Beispielwerte der Demo).
