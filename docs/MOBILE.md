# Handy-Oberfläche (`platform/ultar-mobile`)

Stand 25.11.2026. Die Handy-Oberfläche ist ein **eigenes Paket**, getrennt von der Desktop-Oberfläche
(`platform/ultar-desktop`). Beide teilen nur das Oberflächen-Werkzeug `ultar-ui` und die Sprachbasis
`ultar-i18n` – kein Code der einen steckt in der anderen. So bleibt jede Seite für sich lesbar und baubar.

Das Layout folgt dem Handy des ersten Ultar OS („Ultar Noir“, Repo `ultar-os`, `crates/ultar-shell`).

## Bildschirme

| Bildschirm | Inhalt |
|---|---|
| Sperrbildschirm | Datum, Punktmatrix-Uhr, Hijri-Datum; wartende Mitteilungen nur mit App und Anzahl (Inhalt verborgen); Taschenlampe · Entsperren · Kamera; Notruf; nach oben wischen = entsperren |
| Startbildschirm | Marke (Ultar-Zeichen) am Anfang, Punktmatrix-Uhr mit Datum und Hijri-Datum am Ende; Karten „Als Nächstes“ und „Gerät“ (CPU, RAM, Akku); Suche mit Sprachsuche; App-Raster mit 4 Spalten in **Seiten** mit Punkten; Dock (4 Apps, Telefon am Ende) |
| Schnelleinstellungen | Uhrzeit und Datum, Einstellungen, Ein/Aus; große Kacheln WLAN und mobile Daten (mit „mehr“); runde Schalter in 4 Spalten; Helligkeit und Lautstärke; Mitteilungen mit „Alle löschen“ |
| Zuletzt verwendet | neueste zuerst, öffnen oder schließen, „Alle schließen“ |
| Suche | Zurück, Suchfeld, passende Apps im Raster |

Statusleiste oben (Uhrzeit am Anfang, Funk und Akku am Ende), Navigationsleiste unten
(Zuletzt verwendet · Start · Zurück). Gesten: von oben nach unten = Schnelleinstellungen, auf dem
Startbildschirm nach oben = Suche, seitlich = Seiten (Arabisch in Leserichtung).

Kacheln richten sich nach der Hardware: ohne Modem keine mobilen Daten, ohne Bluetooth kein
Bluetooth, ohne Akku kein Energiesparen, ohne Kameralicht keine Taschenlampe.

## Aufbau

| Schicht | Ordner | Aufgabe |
|---|---|---|
| Modell | `src/model/` | Zustand ohne Zeichnen: Apps und Seiten, Schnelleinstellungen, Zeit, Mitteilungen |
| Ansicht | `src/view/` | ein Baum je Bildschirm, nur `ultar-ui`-Bausteine und Tokens; Punktmatrix-Uhr in `clock.rs` |
| Klicks | `src/click.rs` | was jeder bedienbare Knoten bedeutet |
| App | `src/app.rs` | Eingabe → Modell → Wirkungen (`Effect`) für die Plattform: App starten, Passwort prüfen, Funk schalten … |

Die Oberfläche spricht selbst weder mit dem Bus noch mit dem Compositor. Das Wirt-Programm setzt die
`Effect`s in Bus-Aufrufe um; so ist alles ohne laufendes System prüfbar.

## Prüfung

```bash
cargo test -p ultar-mobile                               # 29 Tests
ULTAR_MOBILE_OUT=/tmp/mobile cargo test -p ultar-mobile --test render   # Bildschirmfotos als PNG
```

- `tests/render.rs`: jeder Bildschirm in Arabisch, Englisch, Deutsch, hell und dunkel – nichts ragt aus
  dem Bildschirm, kein Schlüssel bleibt unübersetzt, Arabisch läuft von rechts.
- `tests/clicks.rs`: jeder Knopf jedes Bildschirms wird angetippt und muss etwas bewirken; Gesten;
  jede App ist über die Seiten erreichbar; Suche beim Tippen.

## Offen (nächste Schritte)

1. Wirt-Programm `ultar-mobile` auf dem Compositor: Statusleiste und Navigationsleiste als Panels,
   Startbildschirm als Hintergrund, Schnelleinstellungen/Zuletzt/Suche als Overlay; `Effect`s an den Bus
   (Launcher, Einstellungen, Netz, Ton, Mitteilungen).
2. Eigener Sperrbildschirm des Handys mit `ultar-authd` (Passwort/PIN), ohne das Sicherheitsniveau von
   `ultar-lock` zu senken.
3. Die Handy-Ausgabe (`EDITION=phone`) startet `ultar-mobile` statt der Kompakt-Ansicht von
   `ultar-desktop`; danach wird die Kompakt-Ansicht (`view/compact`, `lock/compact.rs`) aus
   `ultar-desktop` entfernt, damit der Desktop nur noch Desktop ist.
4. Echte Daten: Apps vom Launcher, Termine vom Kalenderdienst, CPU/RAM/Akku vom Gerät.
