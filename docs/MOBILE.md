# Handy-Oberfläche (`platform/ultar-mobile`)

Stand 25.11.2026. Das Handy hat eine **eigene Oberfläche**, getrennt von der des Desktops
(`platform/ultar-desktop`). Kein Oberflächen-Code der einen steckt in der anderen. Gemeinsam sind nur
Bausteine ohne eigene Ansicht:

| Paket | Bereich | Inhalt |
|---|---|---|
| `platform/ultar-ui` | gemeinsam | Werkzeugkasten: Tokens, Widgets, Text, Ereignisse |
| `platform/ultar-shell-host` | gemeinsam | Compositor-Anbindung: Flächen, Eingabe, Wischen, Zeichnen – ohne eigene Ansicht |
| `platform/ultar-lock-core` | gemeinsam | Zustand der Sperre: Stufen, wer entsperren darf, Wartezeit nach Fehlversuchen |
| `platform/ultar-dialog-core` | gemeinsam | Zustand der Systemdialoge: Warteschlange, Fristen, sichere Antworten |
| `system/hal/ultar-hal-system` (`load`) | gemeinsam | Prozessorlast und belegter Speicher |
| `platform/ultar-mobile` | **nur Handy** | Handy-Oberfläche, Handy-Sperrbildschirm, Handy-Systemdialoge |
| `platform/ultar-desktop` | **nur Desktop** | Leiste, Übersicht, Desktop-Sperrbildschirm, Systemdialoge |

Das Layout folgt dem Handy des ersten Ultar OS („Ultar Noir“, Repo `ultar-os`, `crates/ultar-shell`).

## Programme

| Programm | Rolle / Benutzer | Dienstdatei | Aufgabe |
|---|---|---|---|
| `ultar-mobile` | `ui.shell`, 816 | `mobile.toml` | Statusleiste und Navigationsleiste als Panels, Startbildschirm als Hintergrund, Schnelleinstellungen / Zuletzt verwendet / Suche als Blatt zwischen den Leisten; Apps öffnen dazwischen |
| `ultar-mobile-lock` | `ui.lock-screen`, 814 | `mobile-lock.toml` | Sperrbildschirm: Abdeckung und Anmeldung (Kennwort oder PIN); bietet `lock.lock`; entsperrt nur nach `auth.verify` und wenn der gemeinsame Sperrzustand zustimmt |
| `ultar-mobile-dialog` | `ui.system-dialog`, 815 | `mobile-dialog.toml` | Systemdialoge als Blatt vom unteren Rand: Berechtigung, Verwalter-Kennwort, Bluetooth-Kopplung, Angebot eines anderen Geräts, „Anzeige behalten?“; bietet `sysdialog` wie `ultar-sysdialog` |

Alle drei gehören zur Ausgabe `phone` (`editions = ["phone"]`); `desktop.toml`, `lock.toml` und
`sysdialog.toml` nur noch zur Ausgabe `desktop`. `scripts/build-initramfs.sh` legt je Ausgabe nur deren Programme ins Abbild.

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

## Geräteverbund (Weitergeben)

Wie am Desktop, gleicher Bus-Vertrag (`mesh.send`, Themen Zustand und Fortschritt):
- Schnelleinstellungen: Kachel **Geräteverbund** schaltet den Verbund (`mesh.set_enabled`); ihr Pfeil gibt die
  App im Vordergrund (sonst die zuletzt benutzte) weiter.
- Zuletzt benutzt: jede App hat einen Weitergeben-Knopf; App festhalten und nach oben ziehen tut dasselbe.
- Mehr als ein mögliches Gerät: Auswahlblatt (`CompactMeshSend`), Gerät wählen, Senden oder Abbrechen.
- Antworten (wartet, geöffnet, abgelehnt, keine Antwort, kein Gerät) erscheinen kurz in der Statusleiste.
- Eingehend: Systemdialog `CompactMeshIncoming` (unten).
- Ohne Verbund-Dienst gibt es weder Kachel noch Knopf.

Fenster: die Shell folgt `WindowsChanged`; Home legt Fenster weg, „Zuletzt benutzt“ zeigt sie, ein
App-Symbol holt das vorhandene Fenster nach vorn statt die App neu zu starten.

## Systemdialoge (`ultar-mobile-dialog`)

Gleicher Bus-Vertrag und gleiches Verhalten wie `ultar-sysdialog` des Desktops: meldet `sysdialog` an
(`permission`, `authenticate` mit Verwalter-Kennwort über `auth.verify`, `bt_pair`, `handoff`), fragt
„Anzeige behalten?“ zu jeder offenen Anzeige-Änderung von `settings`, eine Frage nach der anderen; wer
nicht rechtzeitig antwortet, bekommt die sichere Antwort (ablehnen, abbrechen, zurücknehmen). Die
Entscheidungen trifft der gemeinsame Zustand `ultar-dialog-core` (der Desktop nutzt ihn ebenso).

Darstellung (Board `CompactMeshIncoming`, `docs/ui/mesh.md` §2.4): eine `SystemDialog`-Fläche unter der
vertrauenswürdigen Titelleiste des Compositors, im Handy-Modus über den ganzen Arbeitsbereich. Oben der
abgedunkelte Teil in der Scrim-Farbe (ein Tipp dort beantwortet nichts), unten das Blatt mit Titel, Text
und Knöpfen in Fingergröße über die ganze Breite – die Hauptaktion **über** der sicheren; der Fokus liegt
auf der sicheren Antwort. Texte unter `mobile.dialog.*` (gleicher Wortlaut wie `dialog.*` des Desktops).

## Aufbau von `ultar-mobile`

| Schicht | Ordner | Aufgabe |
|---|---|---|
| Modell | `src/model/` | Zustand ohne Zeichnen: Apps und Seiten, Schnelleinstellungen, Zeit, Mitteilungen |
| Ansicht | `src/view/` | ein Baum je Bildschirm, in Teilen (Statusleiste, Navigationsleiste, Inhalt); nur `ultar-ui`; Punktmatrix-Uhr in `clock.rs` |
| Klicks | `src/click.rs`, `src/app.rs` | was jeder Knoten bedeutet; Eingabe → Modell → Wirkungen (`Effect`) |
| Programme | `src/program/` | Compositor und Bus: `shell.rs`, `lock.rs`, `dialog.rs`, `bus.rs`, `bus_map.rs`, `lock_bus.rs`, `dialog_bus.rs` |
| Systemdialoge | `src/dialog/` | Blatt (`sheet.rs`), Angebot eines anderen Geräts (`handoff.rs`), übrige Fragen (`questions.rs`); `choose` = was ein Tipp für den gemeinsamen Dialogzustand bedeutet |

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
- `tests/dialog_render.rs`: jede Systemdialog-Frage auf 360 × 760 in Arabisch und Englisch, hell und dunkel
  (Deutsch baut auch) – nichts außerhalb, nichts unübersetzt, Knöpfe in Fingergröße, Hauptaktion oben,
  Fokus auf der sicheren Antwort; PNGs mit `ULTAR_MOBILE_OUT`.
- `tests/dialog_clicks.rs`: Annehmen / Ablehnen / Fristablauf (= Ablehnen), Berechtigung, „behalten?“ –
  durch Tipps auf das gebaute Blatt.
- `tests/dialog_e2e.rs`: echter Compositor im Handy-Modus, echter Bus, Dialogprogramm im eigenen Thread; ein
  Mesh-Stellvertreter ruft `sysdialog.handoff`, ein Finger tippt Annehmen (Antwort `accept: true`) bzw.
  Ablehnen (`false`).

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
