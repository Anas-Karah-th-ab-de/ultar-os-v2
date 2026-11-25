# Handy-Oberfläche (`platform/ultar-mobile`)

Stand 25.11.2026. Das Handy hat eine **eigene Oberfläche**, getrennt von der des Desktops
(`platform/ultar-desktop`). Kein Oberflächen-Code der einen steckt in der anderen. Gemeinsam sind nur
Bausteine ohne eigene Ansicht:

| Paket | Bereich | Inhalt |
|---|---|---|
| `platform/ultar-ui` | gemeinsam | Werkzeugkasten: Tokens, Widgets, Text, Ereignisse |
| `platform/ultar-shell-host` | gemeinsam | Compositor-Anbindung: Flächen, Eingabe, Wischen, Zeichnen – ohne eigene Ansicht |
| `platform/ultar-lock-core` | gemeinsam | Zustand der Sperre: Stufen, wer entsperren darf, Wartezeit nach Fehlversuchen |
| `system/hal/ultar-hal-system` (`load`) | gemeinsam | Prozessorlast und belegter Speicher |
| `platform/ultar-mobile` | **nur Handy** | Handy-Oberfläche und Handy-Sperrbildschirm |
| `platform/ultar-desktop` | **nur Desktop** | Leiste, Übersicht, Desktop-Sperrbildschirm, Systemdialoge |

Das Layout folgt dem Handy des ersten Ultar OS („Ultar Noir“, Repo `ultar-os`, `crates/ultar-shell`).

## Programme

| Programm | Rolle / Benutzer | Dienstdatei | Aufgabe |
|---|---|---|---|
| `ultar-mobile` | `ui.shell`, 816 | `mobile.toml` | Statusleiste und Navigationsleiste als Panels, Startbildschirm als Hintergrund, Schnelleinstellungen / Zuletzt verwendet / Suche als Blatt zwischen den Leisten; Apps öffnen dazwischen |
| `ultar-mobile-lock` | `ui.lock-screen`, 814 | `mobile-lock.toml` | Sperrbildschirm: Abdeckung und Anmeldung (Kennwort oder PIN); bietet `lock.lock`; entsperrt nur nach `auth.verify` und wenn der gemeinsame Sperrzustand zustimmt |

Beide gehören zur Ausgabe `phone` (`editions = ["phone"]`); `desktop.toml` und `lock.toml` nur noch zur
Ausgabe `desktop`. `scripts/build-initramfs.sh` legt je Ausgabe nur deren Programme ins Abbild.

## Bildschirme

| Bildschirm | Inhalt |
|---|---|
| Sperrbildschirm | Datum, Punktmatrix-Uhr, Hijri-Datum; wartende Mitteilungen nur mit App und Anzahl (Inhalt verborgen); Entsperren; nach oben wischen = Anmeldung |
| Anmeldung | Bild und Name, Feld für Kennwort oder PIN mit Fehler und Wartezeit, Entsperren, Kennwort ↔ PIN, Zurück |
| Startbildschirm | Ultar-Zeichen am Anfang, Punktmatrix-Uhr mit Datum und Hijri-Datum am Ende; Karten „Als Nächstes“ (nächster Termin des Tages) und „Gerät“ (CPU, RAM, Akku); Suche mit Sprachsuche; App-Raster mit 4 Spalten in **Seiten** mit Punkten; Dock (4 Apps, Telefon am Ende) |
| Schnelleinstellungen | Uhrzeit und Datum, Einstellungen, Ein/Aus-Menü (Neu starten, Ausschalten, Sperren); große Kacheln WLAN und mobile Daten (mit „mehr“ → Einstellungsseite); runde Schalter in 4 Spalten; Helligkeit und Lautstärke; Mitteilungen mit „Alle löschen“ |
| Zuletzt verwendet | neueste zuerst, öffnen oder schließen, „Alle schließen“ |
| Suche | Zurück, Suchfeld, passende Apps im Raster |

Gesten: von der Statusleiste nach unten = Schnelleinstellungen, auf dem Startbildschirm nach oben = Suche,
seitlich = Seiten (Arabisch in Leserichtung). Kacheln richten sich nach der Hardware: ohne Modem keine
mobilen Daten, ohne Bluetooth kein Bluetooth, ohne Akku kein Energiesparen.

## Woher die Daten kommen (Bus)

| Was | Quelle |
|---|---|
| Apps | `launcher.list`; starten/schließen `launcher.start` / `launcher.stop` |
| WLAN, Flugmodus | `net.state`; schalten `net.set_wifi`, `net.set_airplane` |
| Bluetooth | `bluetooth.state`; `bluetooth.set` |
| Lautstärke | `audio.state`; `audio.set_volume` |
| Akku | `power.state` |
| Modem vorhanden | `modem.state` |
| Nicht stören, Nachtlicht, Standort, Energiesparen | `settings.get` / `settings.set` / `settings.changed` |
| Helligkeit | `display.<id>.brightness` (das Panel meldet sich per `settings.set_displays` an) |
| Mitteilungen | `notify.list`, `notify.posted`, `notify.dismissed`; löschen `notify.dismiss` |
| Nächster Termin | `calendar.day`, neu bei `calendar.changed` |
| CPU, RAM | `ultar-hal-system::load` |
| Ein/Aus | `power.restart`, `power.off`, `lock.lock` |

## Aufbau von `ultar-mobile`

| Schicht | Ordner | Aufgabe |
|---|---|---|
| Modell | `src/model/` | Zustand ohne Zeichnen: Apps und Seiten, Schnelleinstellungen, Zeit, Mitteilungen |
| Ansicht | `src/view/` | ein Baum je Bildschirm, in Teilen (Statusleiste, Navigationsleiste, Inhalt); nur `ultar-ui`; Punktmatrix-Uhr in `clock.rs` |
| Klicks | `src/click.rs`, `src/app.rs` | was jeder Knoten bedeutet; Eingabe → Modell → Wirkungen (`Effect`) |
| Programme | `src/program/` | Compositor und Bus: `shell.rs`, `lock.rs`, `bus.rs`, `bus_map.rs`, `lock_bus.rs` |

## Prüfung

```bash
cargo test -p ultar-mobile                                              # 40 Tests (dazu Tests in ultar-ril, ultar-powerd, ultar-hal-system, ultar-devd)
ULTAR_MOBILE_OUT=/tmp/mobile cargo test -p ultar-mobile --test render   # Bildschirmfotos als PNG
```

- `tests/render.rs`: jeder Bildschirm und die Anmeldung (Fehler, Wartezeit, PIN) in Arabisch, Englisch,
  Deutsch, hell und dunkel – nichts ragt aus dem Bildschirm, kein Schlüssel bleibt unübersetzt, Arabisch
  läuft von rechts.
- `tests/clicks.rs`: jeder Knopf jedes Bildschirms wird angetippt und muss etwas bewirken; Gesten; jede App
  ist über die Seiten erreichbar; Suche beim Tippen; Ein/Aus-Menü.
- `tests/e2e.rs`: echter Compositor (Handy-Bildschirm 360 × 760), echter Bus, Launcher und Anmeldedienst
  als Stellvertreter. Ein Finger tippt Statusleiste, Schalter, Navigationsleiste und eine App (Start über den
  Launcher); der Sperrbildschirm bleibt bei falschem Kennwort zu und öffnet nur mit dem richtigen.

## Aus dem ersten Ultar OS übernommen (dort auf dem A30 erprobt)

| Funktion | Alt (`ultar-os`, `quick.rs`) | Neu (v2) |
|---|---|---|
| Taschenlampe | Oberfläche schrieb als root direkt in `sysfs` | `ultar-hal-system::torch` (gleiche Logik: Samsung `rear_flash`, sonst LED `*torch*`/`*flash*`, nie `*s2mu*`); `ultar-powerd` schaltet über `power.torch`; `ultar-devd-seat` gibt nur diese Datei der Gruppe 819 (`torch_group` in `devd-seat.toml`); Kachel und Knopf auf dem Sperrbildschirm |
| Mobile Daten | Oberfläche schrieb `/run/ultar/mobiledata.on` | `modem.set_data` an den Funkdienst (`ultar-ril`), der dieselbe Datei führt und die Datenverbindung wie bisher aufbaut |
| Zeitzone | Minuten-Abstand als Einstellung | `datetime.timezone.name` → Zonendatei (TZif) oder Tabelle der Zonen ohne Sommerzeit; Mitteilungszeiten in Ortszeit |

## Noch offen

- Auf einem echten Handy (A30) noch nicht gestartet; dafür fehlt der ARM64-Abbild-Weg (siehe `PLAN.md` P11).
- Notruf und Kamera vom Sperrbildschirm: im alten System lief alles in einem Prozess als root. In v2 braucht
  das Telefon einen Notruf-Wählbildschirm als `CallScreen` über der Sperre (nur Notrufnummern) und eine
  Kamera-App; beides gibt es noch nicht, darum werden die Knöpfe dort noch nicht angeboten.
- Sperrbildschirm und Zeitzone: er darf keine Einstellungen lesen und zeigt die Standardzone
  (`Asia/Damascus`), bis ihm die Zone auf einem eigenen, engen Weg gemeldet wird.
- Mitteilungstexte aus fremden Katalogen: unbekannte Schlüssel zeigen den App-Namen statt des Titels (wie am Desktop).
- Der Geräteverbund (Weitergeben) hat auf dem Handy noch keine Oberfläche.
