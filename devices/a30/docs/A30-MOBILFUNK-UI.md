# Mobilfunk in der Oberfläche: SIM, Netz und Funkturm-Wahl

Umsetzung des Designkonzepts „Ultar OS · Mobilfunk, SIM & Funkturm“ in `ultar-shell`, dazu das gemeinsame
Modell `ultar-radio` und der Funkdienst `ultar-ril --daemon`.

![Funktürme](bilder/mobilfunk-tuerme.png) ![Mobilfunk](bilder/mobilfunk-zentrum.png)
![Start](bilder/mobilfunk-start.png) ![Schnelleinstellungen](bilder/mobilfunk-schnell.png)

## Designsprache

| Farbe | Wert | Bedeutung |
|---|---|---|
| Markengold | `#C9A227` / hell `#E6C65C` | deine Wahl, aktiv, Hauptknopf, angepinnte Zelle |
| Funkblau | `#5FB3D9` / hell `#9FD3EC` | Signal, Zellen, Netzwahl |
| Orange | `#F08A4B` | Warnung, gesperrte SIM, volle Zelle |
| Grün | `#7ED6A0` | freie Zelle, OK |

Signalbalken (RSRP, LTE): 4 = ab −80 dBm, 3 = ab −95, 2 = ab −105, 1 = ab −115, sonst 0. Neben jedem Balken steht der
Messwert in dBm – Bedeutung nie nur über Farbe. Symbol `Ic::Tower` (Mast mit Wellen) für alles Mobilfunk.

## Bedienung

| Ort | Was | Geste |
|---|---|---|
| Statusleiste | **Netz-Pille** neben der Uhr: Technik (4G+/4G/3G …), Band, Balken; Gold-Punkt = Zelle angepinnt. Zustände: „Kein Netz“, „Keine SIM“, „SIM gesperrt“ (orange), „Flugmodus“ | Tippen → Funkturm-Wahl, lange drücken → Mobilfunk-Zentrum |
| Startbildschirm | Verbindungsleiste zwischen Datum und Apps: Anbieter · Zelle · Band, Tempo, Modus-Marke | Tippen → Funkturm-Wahl |
| Schnelleinstellungen | Netz-Leiste: verbundene Zelle + dBm, Umschalter Auto/Smart/Manuell, „Türme ›“ | Wisch von oben |
| Einstellungen → Mobilfunk | Mobilfunk-Zentrum | |

### Mobilfunk-Zentrum (`Screen::Radio`)
- Zwei SIM-Karten nebeneinander (Gold-Rahmen = gezeigte SIM, „DATEN“ = SIM für mobile Daten).
- Live-Karte der verbundenen Zelle: RSRP (Signalstärke), SINR (Störabstand), RSRQ (Qualität) als Wert + Balken.
- Gesperrte SIM: PIN-Feld + „Entsperren“ → Befehl `pin <slot> <pin>` an den Funkdienst.
- „Welche SIM wofür“: mobile Daten, Anrufe, SMS (SIM 1 / SIM 2 / Fragen).
- Netz: Netzwahl, bevorzugte Technik (Auto/4G/3G/2G), Roaming, Verbindungsweg.

### Funkturm-Wahl (`Screen::Cells`)
- Radar: Gerät in der Mitte, Zellen nach Richtung/Entfernung (sonst gleichmäßig verteilt, Abstand nach Signal);
  blaue Linie = verbunden, goldene Linie = Ziel.
- **Auto** = stärkstes Signal (wie Android). **Smart** = bestes Tempo: gemessen, sonst geschätzt aus Störabstand,
  Kanalbreite des Bands und Auslastung; Zellen unter −115 dBm zählen nicht. **Manuell** = angepinnte Zelle
  (Tippen auf eine Zelle pinnt sie).
- Zellenliste: Balken, Band/MHz/Entfernung/dBm, Auslastungslinie (grün / orange ab 70 %), Tempo (≈ = geschätzt),
  Marken VERBUNDEN / ZIEL / ANGEPINNT.
- Fester Knopf unten: „Zu Zelle N jetzt wechseln“ → Befehl `select N`.
- Ehrliche Hinweise: kann das Modem nicht erzwingen → „nur bevorzugen“; liefert es keine Nachbarzellen → Hinweis.

### Verbindungsweg (`Screen::Route`)
Gerät → WLAN bzw. Zelle → Kernnetz des Anbieters → Ultar-VPN → Internet (mit Werbesperre). Fehlendes VPN und
fehlende Verbindung sind orange markiert; darunter Datenverkehr (Monitor) und „VPN einschalten“.

## Datenfluss

| Datei | Schreibt | Liest |
|---|---|---|
| `/run/ultar/radio/state` | `ultar-ril --daemon` | Oberfläche (höchstens 1 s alt) |
| `/data/ultar/radio.conf` | Oberfläche | Funkdienst (Modus, angepinnte Zelle, SIM-Rollen, Roaming, Technik) |
| `/run/ultar/radio/cmd` (0600) | Oberfläche | Funkdienst (`select N`, `pin S CODE`, `rescan`) |

Format und Logik liegen in `crates/ultar-radio` (nur std, 7 Tests): Zustand/Vorgaben lesen und schreiben (atomar),
`level`, `est_mbps`, `strongest`, `fastest`, `target` und die Hysterese `SmartGate` (Wechsel erst bei ≥ 20 % mehr
Tempo, 30 s stabil).

Dienst: `rootfs/etc/ultar/services/ril.toml` startet `ultar-ril --daemon` (16 MB). Ist das Modem nicht online,
schreibt der Dienst `modem=offline` mit Grund; die Oberfläche zeigt dann „Kein Netz“ bzw. „Modem nicht bereit“.

## Stand / offen (ehrlich)

- **Am Gerät noch nicht getestet** – das Modem (CP) ist auf dem A30 noch nicht ONLINE (siehe A30-MODEM.md).
- Der Funkdienst liefert heute: SIM 1-Status, Registrierung, Technik, PLMN, Signal → **nur die verbundene Zelle**.
- **Nachbarzellen-Liste und Zellsperre** brauchen den passenden Samsung-IPC-Befehl (noch zu bestimmen). Bis dahin
  meldet der Dienst `neighbours=0`, `pin_supported=0`; `select N` wird als „bevorzugt“ quittiert.
- SIM 2 (zweiter FMT-Kanal) wird noch nicht abgefragt.
- `sim::enter_pin` ist nach libsamsung-ipc gebaut (`// VERIFY` am Gerät).
- Anbietername: derzeit PLMN-Ziffern; Klartext braucht eine PLMN-Tabelle bzw. den NITZ/Operator-Namen des Modems.

## Prüfen ohne Gerät
```bash
ULTAR_DATA=/tmp/u ULTAR_RADIO_DEMO=1 ultar-shell --shot 1080 2340 cells de cells.bmp   # auch: radio, route, shade, home
cargo test -p ultar-radio -p ultar-ril
```
`ULTAR_RADIO_DEMO=1` zeigt Beispieldaten (Demo-Netz, 5 Zellen) und schreibt keine Vorgaben.

## Layout nach Designkonzept (Start, Schnelleinstellungen, Statusleiste)
- **Start** (ab 600 dp Höhe, Hochformat): Markenzeile (Logo + ULTAR OS), große Uhr an der Anfangsseite, Datum,
  **Verbindungskarte** (Mobilfunk bevorzugt, sonst WLAN; Download/Upload live aus /proc/net/dev, Signal mit Balken;
  „Turm wählen“ / „SIM & Netz“), darunter Glas-Kacheln (gelernte Lieblings-Apps zuerst) + **Alle Apps** (`Screen::Apps`).
  Kleine/quer liegende Bildschirme: klassischer Start (Uhr + volles Raster).
- **Schnelleinstellungen**: Uhr/Datum, Akku groß, Einstellungen-Knopf; Verbindungs-Cluster mit 2×2 großen Kacheln
  (WLAN mit Netzname, Mobile Daten mit SIM/Technik, Bluetooth, Flugmodus – AN/AUS + Zustand), Netz-Leiste mit
  Auto/Smart/Manuell; vier kleine Schalter mit Symbolen (Lampe, VPN, Ton, Datenschutz); Helligkeit; Benachrichtigungen als Karten.
- **Statusleiste**: Uhr · Netz-Pille … WLAN-Bögen · VPN-Schild (Gold) · Mobile-Daten-Pfeile · Akku.
- Neue Symbole: Wifi, UpDown, Bt, Plane, Torch, Speaker, Lock, Grid, Sliders, Phone, Chat, Camera.

## Gerätetest ohne Antippen (Fern-Navigation)
`/run/ultar/shell.goto` mit `<bildschirm> [sprache]` (z. B. `cells`, `radio ar`, `shade`, `apps`) + `kill -USR1 <pid>`
→ Shell öffnet den Bildschirm und legt `/data/ultar/screen.png` ab. PC-Helfer: `mf_deploy.ps1` (Shell einspielen +
Beispielzustand) und `mf_shot.ps1 <bildschirm> [sprache]` im UltarFlash-Ordner.
