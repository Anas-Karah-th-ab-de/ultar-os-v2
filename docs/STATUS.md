# Ultar OS v2 – Stand

Stand 22.11.2026 · alle Zahlen gemessen in dieser Sitzung (Bau-Rechner: 2 Kerne, QEMU ohne KVM)

## Umsetzung Welle 2 (23.11.2026) – gebaut, eingebunden, unabhängig geprüft

| Baustein | Stand |
|---|---|
| Kern: Abschottung Stufe 1 | Mount- und PID-Namensräume, Rechte je Prozess; Ausbruchs-Tests in der Kern-Prüfung (QEMU) grün |
| `platform/ultar-ui` | Baukasten: Gestaltungswerte aus `design/tokens.toml`, Zeichnen, arabischer Textsatz mit Leserichtung, 8 Bausteine mit allen Zuständen, Bildvergleich mit 297 Referenzbildern; ganzer Bildschirm 1920×1080 in 3,6–5,7 ms |
| `system/core/ultar-settingsd` | Einstellungsdienst mit 111 Einstellungen in 10 Bereichen, „Behalten oder zurück“, Profile je Bildschirm-Kombination; Abfrage 34 µs |
| `system/core/ultar-devd` | läuft als Dienst (Start 2–3 ms), Bus-Dienst `devices`, `udevctl`; Treiberwechsel nur, wo der Kern es kann |
| Server + Gerät | Store und Updates ohne Konto, Berechtigungen für Premium und gesperrte Kanäle, Schutz der offenen Endpunkte |
| Prüfung | drei unabhängige Prüfer: 1 kritischer und 19 schwere Befunde, davon 1 kritischer und 15 schwere behoben – Rest in `docs/review/OFFENE-BEFUNDE.md` |

Zusammen jetzt **rund 1 400 automatische Tests** ohne Fehler (Arbeitsbereich, Server, Ende-zu-Ende), Kern-Prüfung grün.
**Nicht produktionsreif:** siehe „Offen – sperrt den Einsatz“ in `docs/review/OFFENE-BEFUNDE.md`. Noch keine sichtbare
neue Oberfläche: Compositor, Desktop-Hülle und Apps sind nicht gebaut; nichts lief auf echter Hardware.

## Fertig und geprüft

| Was | Nachweis |
|---|---|
| Sieben-Bereiche-Struktur, gesamter Code des Vorgängers übernommen | Workspace baut; **842 Tests bestanden, 0 fehlgeschlagen** (2 bewusst übersprungen) |
| Einheitliche Formatierung des gesamten Codes | `cargo fmt --all --check` sauber |
| Neue Bausteine (i18n, profile, hal, hal-system, bus, driverpkg, devd, check) | zusammen rund 650 Tests; `clippy -D warnings` sauber; in diesen Bausteinen keine Regelverletzung |
| Ultar-Kern baut und startet | QEMU (q35, UEFI): bereit nach **145 ms**, alle Geräte mit Treiber, Ring-3-Prüfprogramm „ALLES OK“ |
| Systembus läuft **auf dem Ultar-Kern** | statisches Prüfprogramm im Kern in QEMU: „BUS SELFCHECK OK“ |
| Systembus Leistung (Linux, 2 geteilte Kerne) | Anfrage hin und zurück 33–40 µs (Median), 165–180 Tsd. Ereignisse/s, im Leerlauf kein Aufwachen |
| Treiber-Depot | 6 signierte Pakete + signierter Index gebaut; Zuordnung für beide Testrechner geprüft |
| Regelprüfung mit Sperrklinke | 5 602 Altlasten des übernommenen Codes erfasst; die Zahl darf nur sinken |

## Beim Übernehmen gefundene und behobene Fehler

- Der Browser `ultar-web` ließ sich auf dem alten Stand nicht mehr bauen (fehlende Abhängigkeiten und Module).
- Drei Testfehler: Bildschirm-Maßstab (Test passte nicht mehr zum Entwurf mit 390 Punkten Breite) und
  Bildschirmübertragung im Verbund (Überlauf im Testmuster); dazu liefen Tests der Oberfläche, die gemeinsame Umgebungsvariablen ändern, gegeneinander (fiel erst auf GitHub auf) – jetzt nacheinander.
- Die alte Geräteart-Erkennung (`ultar-edition`) ist durch `ultar-profile` + `ultar-hal-system` ersetzt.

## Noch nicht gemacht (ehrlich)

| Phase | Offen |
|---|---|
| P2 | Die rund 1 400 Texte der alten Oberfläche und die Werkzeug-Texte sind noch nicht auf `ultar-i18n` umgestellt; `ultar-sprache` existiert noch |
| P2b | Installer, Startseite und Bedien-Konsole liegen noch im Kern |
| P3 | Noch kein echtes HAL-Backend; die Oberfläche greift weiter direkt auf Hardware zu (393 Stellen) |
| P4 | `ultar-devd` ist Logik + Tests; der Dienst selbst und die Anbindung an Kern und Bus fehlen |
| P5/P6 | Oberfläche ist weiter ein Block (`platform/ultar-shell`); Compositor und Apps als eigene Prozesse fehlen |
| P7 | Pakete tragen heute nur den Aktivierungs-Eintrag der im Kern eingebauten Treiber; ladbare Kern-Module fehlen |
| P8–P11 | Installer auf Ubuntu-Niveau, dynamische Einstellungen, Barrierefreiheit, A/B-Updates, ARM-Port |
| Treiber | Für die Testrechner fehlen: Intel-Grafik, WLAN, Bluetooth, Touchpad, Akku/Thermik (ACPI-Interpreter), Kamera, TPM und Stufe 4 |
| Umbenennung | Deutsche Bezeichner im übernommenen Code (z. B. `ultar-zellen`, `ultar-verbund`) sind noch nicht englisch |

## Nicht geprüft

- Kein Lauf auf echter Hardware in dieser Sitzung (ThinkPad, Beelink, A30) – alles in QEMU bzw. als Test.
- Die GitHub-Prüfung (`.github/workflows/ci.yml`) läuft auf GitHub: Format, Regeln, Tests, statischer ARM64-Bau der neuen Bausteine und Kern-Start in QEMU.
- ARM64: nur gebaut (statisch, in der GitHub-Prüfung), nicht ausgeführt; `ultar-llm` (llama.cpp) wurde nicht gebaut.
- Arabische Texte sind nicht von einem Muttersprachler gegengelesen.
- Zielwerte in `tools/budgets.toml` sind Vorgaben, keine Messwerte.

## Entscheidungen der Bausteine, die noch bestätigt werden sollten

Siehe `docs/integration/*.md`, jeweils letzter Abschnitt (u. a.: ohne gesetzte Sprache ist Arabisch Standard;
ein erzwungener Modus des Nutzers gilt auch auf Servern; Handgerät ab 178 mm Diagonale zählt als Tablet;
`ultar-busd` startet nur mit Rechte-Datei).
