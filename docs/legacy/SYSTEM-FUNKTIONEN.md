# Ultar OS – Systemfunktionen (Stand 19.11.2026)

50 grundlegende Funktionen eines Handy-Betriebssystems, eigenständig ergänzt. Code:
`crates/ultar-shell/src/sysx.rs` (Logik, getestet), `sysd.rs` (Hintergrunddienst),
`app_sys.rs` (Oberfläche), Einstellungen in `/data/ultar/system.toml`.
Fern-Navigation zum Prüfen: `echo <name> [sprache] > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)`.

| # | Funktion | Wo / Fern-Name | Stand |
|---|---|---|---|
| 1 | Sperrbildschirm (Uhr, Datum, Akku, nur Anzahl der Benachrichtigungen) | `locknow` | ✅ A30 (Ein/Aus sperrt) |
| 2 | PIN 4–8 Ziffern, gesalzener Hash (20 000 × SHA-256) | Einstellungen → Bildschirmsperre, `pinset` | ✅ A30 (setzen, falsche PIN abgewiesen, richtige entsperrt) |
| 3 | Automatisch sperren beim Ausschalten des Bildschirms | Ein/Aus, Bildschirm-Timeout | ✅ |
| 4 | Sperre nach Fehlversuchen (ab 5: 30 s, verdoppelnd bis 15 min) | Sperrbildschirm | ✅ Test |
| 5 | Sperre beim Start, wenn eingeschaltet | Start | ✅ |
| 6 | Zeitabgleich SNTP (eigene DNS-Auflösung, Ersatz über HTTP-Date) | `timezone` | ✅ A30 (Uhr war 35 min falsch, jetzt gestellt) |
| 7 | Automatischer Abgleich: nach Start, alle 12 h, nach Fehler in 5 min | sysd | ✅ |
| 8 | Zeitzonen mit Regeln (29 Städte, EU/USA/Ägypten/Libanon) | `timezone` | ✅ Test |
| 9 | Sommerzeit automatisch (Versatz wird selbst nachgeführt) | tick | ✅ Test |
| 10 | 12-/24-Stunden-Format (Statusleiste, Start, Sperre, Weltuhr) | `timezone` | ✅ |
| 11 | Energiesparmodus (Helligkeit ≤ 50 %, CPU-Regler sparsam) | `battery` | ✅ |
| 12 | Energiesparen automatisch ab einstellbarem Akkustand, aus beim Laden | sysd | ✅ Test |
| 13 | Hinweis „Akku schwach" bei 15 % und 5 % | sysd | ✅ Test |
| 14 | Hinweise „Wird geladen" / „Akku voll" | sysd | ✅ Test |
| 15 | Akku-Details: Temperatur, Zustand, Spannung | `battery` | ✅ |
| 16 | Hitzewarnung ab 70 °C (höchstens alle 30 min) | sysd | ✅ |
| 17 | Leistungsprofil Stromsparend/Ausgewogen/Leistung (CPU-Regler) | `battery` | ✅ |
| 18 | Nachtlicht (warmer Filter, Stärke einstellbar) | `nightlight` | ✅ A30 |
| 19 | Nachtlicht-Zeitplan | `nightlight` | ✅ |
| 20 | Automatische Helligkeit über Lichtsensor (Samsung: Sensor wird eingeschaltet, R/G/B/W-Format) | `nightlight` | ✅ A30 Sensor liefert Werte |
| 21 | Hoher Kontrast | `nightlight` | ✅ |
| 22 | Bildschirmfoto: Leiser + Ein/Aus → /data/Pictures/Screenshots | Tasten | ✅ A30 |
| 23 | Tonprofile Normal / Vibration / Lautlos | `profiles` | ✅ |
| 24 | Nicht stören mit Zeitplan (Anrufe, Wecker, Erinnerungen kommen durch) | `profiles` | ✅ |
| 25 | Lautstärke-Anzeige bei den Lautstärketasten | Tasten | ✅ |
| 26 | Vibration bei Berührung | `profiles` | ✅ |
| 27 | Benachrichtigungen „Alle löschen" (Schnelleinstellungen) | Wisch von oben | ✅ |
| 28 | Benachrichtigungsverlauf (300 Einträge) | `history` | ✅ |
| 29 | Verstecktes WLAN hinzufügen | `hiddenwifi` | ✅ |
| 30 | Echte WLAN-Signalstärke in der Statusleiste | Statusleiste | ✅ |
| 31 | Eigene DNS-Server (ultar-net beachtet /data/ultar/dns.override) | `nettools` | ✅ |
| 32 | Netzdiagnose: Router-Ping, Internet-Ping, DNS, HTTPS | `nettools` | ✅ |
| 33 | Monatliches Datenlimit mit Warnung bei 80 % / 100 % | `nettools` | ✅ Test |
| 34 | WLAN-Details (Signal, Frequenz, IP, Gateway, DNS, MAC, Sicherheit) | `nettools` | ✅ A30 |
| 35 | Systemprotokolle: Kernel (klogctl) und Dienste | `logs` | ✅ |
| 36 | Neustart in Download-Modus / Recovery (zweimal tippen) | `dev` | ✅ |
| 37 | Sicherung (Einstellungen, Notizen, Kontakte, Kalender, Wecker; ohne Kennwörter) | `backup` | ✅ A30 |
| 38 | Wiederherstellen | `backup` | ✅ Test |
| 39 | Werkseinstellungen (dreifache Bestätigung, Fotos bleiben) | `backup` | ✅ Test |
| 40 | Speicher bereinigen (Zwischenspeicher, alte Programmstände) | `backup` | ✅ |
| 41 | Entwickleroptionen, „Beim Laden wach bleiben" | `dev` | ✅ |
| 42 | Berührungen anzeigen | `dev` | ✅ |
| 43 | Bildrate / Zeichenzeit anzeigen | `dev` | ✅ |
| 44 | Kalender-App (Monatsansicht, Termine) | `calendar`, `event` | ✅ A30 |
| 45 | Termin-Erinnerungen als Benachrichtigung | sysd | ✅ Test |
| 46 | Weltuhr | `world` | ✅ |
| 47 | Wecker schlummern (5 min) | Klingel-Dialog | ✅ |
| 48 | Rechner-Verlauf (Tippen auf die Anzeige) | `calchist` | ✅ |
| 49 | Einheitenrechner (Länge, Gewicht, Temperatur, Fläche, Volumen, Tempo, Daten) | `units` | ✅ Test |
| 50 | Globale Suche (Apps, Einstellungen, Kontakte, Notizen; arabisch tolerant) | `search` | ✅ Test |

Zusätzlich behoben: **Wecker und Timer klingelten bei ausgeschaltetem Bildschirm nicht**
(`tick()` lief nur bei eingeschaltetem Bildschirm). Jetzt prüft die Hauptschleife auch im
Ruhezustand jede volle Minute und weckt den Bildschirm – am A30 geprüft (Bildschirm aus, Wecker
weckte pünktlich zur Minute).

„✅ Test" = durch automatische Tests abgedeckt (`cargo test -p ultar-shell`), „✅ A30" = am Gerät geprüft.

Ebenfalls am A30 gefunden und behoben: **WLAN „verbunden", aber kein Internet** (Datenport des
Samsung-Treibers gesperrt durch doppelte/hart beendete wpa_supplicant-Instanz) – Details in `docs/A30-WLAN.md`.

## Gerätetests ohne Finger
`tools/uinject.c` (statisch, aarch64) speist Berührungen und Tasten in die Eingabegeräte ein:
`uinject /dev/input/event1 tap <x> <y>` (A30-Touch 0–4095), `uinject /dev/input/event14 keys 114 116`
(Bildschirmfoto), `key 116` (Ein/Aus). Damit wurden PIN, Sperre, Nachtlicht und Sicherung am A30 geprüft.
Zeitabgleich über HTTP am A30 geprüft (Zeitserver absichtlich unerreichbar → „HTTP").

## Fingerabdruck (Stand)
Sensor: Goodix GW36H3 (`/sys/class/fingerprint/fingerprint`, `/dev/goodix_fp`). Bild und Abgleich laufen bei
Samsung in der Sicherheitsumgebung TEEgris (`/dev/tzdev`, Trustlets in `/vendor/tee`); Linux sieht nur
Steuerung/Interrupt, nicht die Bilddaten. Möglicher Weg: Samsungs Fingerabdruck-Dienst
(`vendor.samsung.hardware.biometrics.fingerprint@3.0`) in der abgeschotteten Android-Laufzeit betreiben und
nur „Finger erkannt: ja/nein" an Ultar weitergeben. Noch nicht umgesetzt.
