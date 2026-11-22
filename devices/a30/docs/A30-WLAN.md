# WLAN auf Ultar OS (Stand 19.11.2026)

## Ablauf
1. `ultar-net` findet die WLAN-Schnittstelle (`wlan0`; `p2p0`/`swlan0` werden ignoriert).
2. Einschalten mit Wiederholung (1 → 30 s). Auf dem A30 lädt das Einschalten die Funk-Firmware
   (`/vendor/etc/wifi/mx140.bin`) – solange `/vendor` fehlt oder rfkill sperrt, scheitert es (z. B. os error 22).
   Status dann `wlan-start-fehler`, im Protokoll mit Fehlertext.
3. Wartet auf `/data`, legt `/data/ultar/wifi.conf` mit Steuerschnittstelle an (ältere Dateien werden ergänzt).
4. `wpa_supplicant` läuft **immer** (auch ohne gespeichertes Netz) und wird bei Absturz neu gestartet.
5. Verbindung da (carrier) → eigener DHCP-Client, Adresse/Route/DNS.

## Oberfläche (Einstellungen → Netzwerk)
- Suche startet beim Öffnen automatisch; läuft über die Steuerschnittstelle (`SCAN` → Ereignis → `SCAN_RESULTS`),
  Rückfall auf `iw` (Schnittstelle wird vorher eingeschaltet).
- Netze nach Signal sortiert, jeder Name nur einmal (2,4/5 GHz), Sonderzeichen korrekt, Qualität + dBm, „Gespeichert“.
- Verbinden ohne Neustart des Netzdienstes; mehrere Netze bleiben gespeichert. Gespeicherte/offene Netze: ein Tippen.
- Rückmeldung: „Verbunden mit …“, „Falsches Kennwort“, „Verbindung fehlgeschlagen“.
- Schnellschalter WLAN/mobile Daten ohne `ip`-Programm (ioctl).

## Am Gerät prüfen (Tür COM13)
```
cat /run/ultar/net/wlan0          # state=wlan-start | wlan-start-fehler | wlan-getrennt | dhcp | verbunden
ls /run/wpa_supplicant/           # wlan0 = Steuerschnittstelle da
ls /vendor/etc/wifi/              # mx140.bin muss sichtbar sein
cat /sys/class/rfkill/*/type /sys/class/rfkill/*/soft
```
Test ohne Hardware: `wpa_supplicant -i lo -D wired -c w.conf -B`, dann
`ULTAR_WPA_IFACE=lo cargo test -p ultar-shell -- --ignored ctrl_roundtrip`.

## Am A30 getestet (19.11.2026, 18 Uhr)
- Nach sauberem Neustart + /vendor eingebunden: Chip startet (mx140.bin geladen), wlan0 oben,
  wpa_supplicant-Steuerschnittstelle da, Suche liefert 10 Netze (inkl. arabischer Namen, 2,4 + 5 GHz).
- Wichtig: Wird der Chip in einer Sitzung mehrfach erfolglos gestartet (nach Modem-Versuchen), hängt er
  (MM_START_IND Timeout -110, PMU-Timeout) – nur ein Neustart hilft.
- Fehlt noch auf dem Gerät: /vendor beim Boot (kommt mit dem nächsten Flash, init 3ab439d);
  /efs/wifi/.mac.info fehlt -> Treiber nimmt Ersatz-MAC.
- Bildschirmfoto mit Suchergebnis: `ULTAR_SHOT_WAIT=12000 ultar-shell --shot 1080 2340 net de x.bmp`.

## Internet-Fehler behoben (19.11.2026, 21 Uhr) – „verbunden, aber kein Internet"
- Befund am A30: WLAN `verbunden`, DHCP ok, aber jedes TCP/UDP-Paket ins Netz verworfen
  (`/proc/net/dev` tx drop steigt), Treiber meldet in `/proc/driver/unifi0/fcq`: `Controlled port :Blocked`.
- Ursache: zwei wpa_supplicant-Instanzen auf wlan0 (nach hartem Neustart von ultar-net lief die alte weiter)
  bzw. ein harter Neustart ohne Abmeldung – der Samsung-SCSC-Treiber öffnet den Datenport dann nicht.
- Lösung in ultar-net: alte Instanzen vor dem Start sauber beenden (SIGTERM, dann SIGKILL, 1,5 s Pause),
  beim Stoppen des Dienstes eigene Instanzen mitnehmen, Selbstheilung: bleibt der Port trotz Verbindung
  gesperrt, DISCONNECT + RECONNECT (höchstens alle 60 s).
- Geprüft: `kill -9` auf ultar-net → neue Instanz, Port `Opened`, TCP zu 1.1.1.1 ok, Zeitabgleich ok.
- Zeitabgleich: SNTP mit eigener DNS-Auflösung, Ersatz über HTTP-`Date` (TCP 80), Wiederholung nach 5 min.
