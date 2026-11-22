# Ultar OS v2 – Stand

Stand 22.11.2026 · alle Zahlen gemessen in dieser Sitzung (Bau-Rechner: 2 Kerne, QEMU ohne KVM)

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
- Zwei Tests schlugen fehl: Bildschirm-Maßstab (Test passte nicht mehr zum Entwurf mit 390 Punkten Breite) und
  Bildschirmübertragung im Verbund (Überlauf im Testmuster).
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
- Die GitHub-Prüfung (`.github/workflows/ci.yml`) ist geschrieben, aber noch nie auf GitHub gelaufen.
- Kein ARM-Bau der neuen Bausteine (nur x86_64); `ultar-llm` (llama.cpp) wurde nicht gebaut.
- Arabische Texte sind nicht von einem Muttersprachler gegengelesen.
- Zielwerte in `tools/budgets.toml` sind Vorgaben, keine Messwerte.

## Entscheidungen der Bausteine, die noch bestätigt werden sollten

Siehe `docs/integration/*.md`, jeweils letzter Abschnitt (u. a.: ohne gesetzte Sprache ist Arabisch Standard;
ein erzwungener Modus des Nutzers gilt auch auf Servern; Handgerät ab 178 mm Diagonale zählt als Tablet;
`ultar-busd` startet nur mit Rechte-Datei).
