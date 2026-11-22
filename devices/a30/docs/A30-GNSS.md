# A30 GNSS/GPS

## Hardware
Die A30 (Exynos 7904 / universal7904) nutzt **keinen externen NMEA-GPS-Chip**. Im Defconfig
(`arch/arm64/configs/exynos7885-a30_defconfig`) sind **alle `CONFIG_GPS_BCM*` aus** und
**`CONFIG_GNSS_SHMEM_IF=y`** gesetzt. Das GPS ist der integrierte **Samsung-SLSI-GNSS-Co-Prozessor
„Kepler"**, angebunden über Shared Memory (Treiber `drivers/misc/gnss_if`, Controller
`gnss_keplerctl_device.c`), wie das Modem – nur eigener Prozessor/eigene Mailbox/PMU.

- **Device-Knoten:** `/dev/gnss_ipc` (DT: `shmem,device_node_name = "gnss_ipc"`).
- **ioctl-Magic:** `'K'` → `_IO('K', nr)`:
  - `GNSS_IOCTL_RESET` 0x4b00
  - `GNSS_IOCTL_LOAD_FIRMWARE` 0x4b01
  - `GNSS_IOCTL_REQ_FAULT_INFO` 0x4b02
  - `GNSS_IOCTL_REQ_BCMD` 0x4b03
  - `GNSS_IOCTL_READ_FIRMWARE` 0x4b04
  - `GNSS_IOCTL_CHANGE_TCXO_MODE` 0x4b06
  - `GNSS_IOCTL_SET_SENSOR_POWER` 0x4b07
  - `GNSS_IOCTL_RELEASE_RESET` 0x4b08
- **Transport:** `exynos_ipc`-Frames (`include/exynos_ipc.h`): 12-Byte-Header, Start 0xABCD (LE),
  Länge @ Offset 6 (u16, inkl. Header), Kanal-ID @ Offset 8 (u8), dann Payload. Dieselbe Rahmung
  wie beim Modem.

## Boot-Ablauf (wie Modem, VERIFY am Gerät)
Der Kepler bootet nicht von selbst; er braucht seine Firmware im Shared Memory:
1. `/dev/gnss_ipc` öffnen.
2. GNSS-Firmware in den reservierten GNSS-SHMEM-Bereich schreiben (der Treiber mappt ihn; die
   Firmware kommt aus einer GNSS-Partition bzw. einer Datei – **VERIFY**, welche; Kandidaten unten).
3. `ioctl(GNSS_IOCTL_LOAD_FIRMWARE)` → Signatur/Load.
4. `ioctl(GNSS_IOCTL_RELEASE_RESET)` → Kepler startet.
5. Danach liefert der Kepler über `read(/dev/gnss_ipc)` IPC-Frames; der Payload des GNSS-Kanals
   transportiert die Positionssätze (NMEA-Text bzw. Samsung-GNSS-Binär – am Gerät prüfen).

Hinweis: Die eigentliche GNSS-Firmware-Datei und der Partitionsname sind auf diesem Build noch zu
bestätigen (anders als beim Modem gibt es keine offensichtliche `p19`-TOC-Partition; die GNSS-FW
liegt bei Samsung oft in der CP-/radio-Firmware oder unter `/vendor/firmware`). Erst ohne Boot
testen (Device ist da, aber leer), dann den Boot nachziehen.

## Gerätetest (Koordinator, über die Tür)
1. **Knoten prüfen:**
   `ls -la /dev/gnss_ipc; cat /proc/devices | grep -i gnss`
2. **Firmware suchen:**
   `mount -o ro /dev/mmcblk0p26 /vendor 2>/dev/null; ls /vendor/firmware 2>/dev/null | grep -iE 'gnss|kepler|gps'`
   und die kleinen Partitionen nach einem GNSS-Image absuchen (wie beim Modem p19):
   `for p in 17 18 20 21; do echo p$p; dd if=/dev/mmcblk0p$p bs=512 count=1 2>/dev/null | od -A d -c | head -2; done`
3. **gnsstest pushen + laufen lassen** (blockiert nicht):
   `/data/gnsstest` → zeigt, ob `/dev/gnss_ipc` da ist und ob (ungebootet erwartungsgemäß) Daten kommen.
4. **dmesg** nach dem Öffnen: `dmesg | grep -iE 'gnss|kepler|gif:'` → Zustand/Fehler des GNSS-Treibers.
5. Erst wenn der Boot-Schritt (Firmware) steht: nach `RELEASE_RESET` erneut `/data/gnsstest /dev/gnss_ipc`
   → NMEA-Treffer erwartet; dann im Freien auf Fix warten.

## Software (dieses Repo)
- `crates/ultar-shell/src/gnss.rs`: vollständiger **NMEA-0183-Parser** (GGA/RMC/GSV → Position,
  Fix-Qualität, Höhe, HDOP, Satelliten, UTC), **exynos_ipc-Entrahmung** von `/dev/gnss_ipc`,
  API `read_fix(&mut GnssState)` und `render_gnss(ui, y, state, t)` für den Standort-Screen.
  Unit-Tests für NMEA + ioctl-Nummern enthalten. Host-`cargo check` grün.
- `tools/gnsstest.c`: statisches Diagnosewerkzeug (fork+Timeout, blockiert nie).

## Offen (eigenes großes Paket)
Der GNSS-Firmware-Boot (Schritt 2–4) und – falls der Payload Samsung-Binär statt NMEA ist – der
GNSS-Protokoll-Decoder sind am Gerät zu verifizieren/auszubauen. Der NMEA-Weg ist fertig und greift,
sobald NMEA-Sätze über den Kanal kommen.

## Stand 2026-11-20: GPS läuft über Samsungs gpsd (Android-Umgebung)

Der eigene Kepler-Boot wurde nicht weiterverfolgt: Firmware und Protokoll stecken in Samsungs `gpsd`
(`/vendor/bin/hw/gpsd`, lädt den Kepler selbst über `/dev/gnss_ipc`). Weg wie beim Fingerabdruck:

    gpsd  ->  vendor.samsung.hardware.gnss@2.0-service (HIDL android.hardware.gnss@1.0..2.1::IGnss)
          ->  gnssclient daemon (eigener HIDL-Client über /dev/hwbinder, tools/android-gnss/)
          ->  /run/ultar/gnss.nmea  ->  Oberfläche (gnss.rs, Einstellungen -> Standort)

Dienst: `rootfs/etc/ultar/services/gnss.toml` startet `/data/android/gnss-dienst.sh`
(Quelle `tools/android-gnss/gnss-dienst.sh`), Programm `/data/android/gnssclient` (`tools/android-gnss/gnssclient.c`,
statisch: `aarch64-linux-gnu-gcc -O2 -static -o gnssclient gnssclient.c`).

Stolpersteine (am Gerät gefunden):

| Problem | Lösung |
|---|---|
| gpsd hängt in „Waiting for sys.boot_completed" | `setprop sys.boot_completed 1` und `service.bootanim.exit 1` (über propsvc) |
| `setCallback` kehrt nie zurück | MEMS-Teil wartet auf Androids Sensordienst → `GlueLayer_IsMemsEnable=0` in einer Kopie von `gps.cfg`, per bind-mount über `/vendor/etc/gnss/gps.cfg` im chroot |
| `/dev/gnss_ipc` gleichzeitig von der Oberfläche geöffnet | Oberfläche öffnet den Knoten nicht mehr, liest nur noch `/run/ultar/gnss.nmea` |

Ausgabe in `/run/ultar/gnss.nmea` (max. 256 KB, dann neu): NMEA-Sätze unverändert plus
`$ULLOC,lat,lon,alt,genauigkeit_m,m/s,grad,zeit_ms` (gnssLocationCb) und
`$ULSV,anzahl,benutzt,prn:el:az:cn0:benutzt,…` (gnssSvStatusCb).

Akku/Datenschutz: GNSS läuft nur, solange `/run/ultar/gnss.want` jünger als 10 s ist. Die Oberfläche berührt die
Datei nur im Standort-Bildschirm und nur, wenn „Standort" im Datenschutzzentrum erlaubt ist.

Geprüft am A30: gpsd lädt den Kepler, Dienst und Client laufen, `start`/`stop` folgen dem Wunsch,
NMEA kommt jede Sekunde ($GPGGA/$GPRMC/$PSAMCLK). Im Raum (Gerät auf dem Tisch) wurden in 2,5 Minuten
noch keine Satelliten gesehen – **Erstortung im Freien bzw. am Fenster noch zu bestätigen.**
Kaltstart ohne Hilfsdaten (keine SIM/SUPL, kein SGEE) dauert im Freien typisch 30 s bis 2 min.

## Stand 2026-11-21: Kaltstart-Hilfe (A-GPS light)

`gnssclient` speist beim Einschalten über IGnss@1.0 ein:
- `injectTime` (Code 5): Systemzeit + Bootzeit-Bezug, Unsicherheit 2 s – nur wenn die Uhr gestellt ist (nach 2025).
- `injectLocation` (Code 6): letzte eigene Position aus `/data/ultar/gnss.last` (≤ 30 Tage alt, Genauigkeit mind. 2 km angesetzt).
Nach jedem Fix (Genauigkeit < 500 m) wird `/data/ultar/gnss.last` höchstens einmal pro Minute neu geschrieben.
Damit wird der zweite und jeder spätere Start deutlich schneller. Der erste Fix braucht weiter freien Himmel.

Messung 21.11. nachts im Raum: 100 s lang kamen nur leere `$GPGGA`, kein einziger Satellit (`$ULSV` blieb aus).
Das deutet auf fehlenden Himmel hin, nicht auf einen Fehler der Kette – Test im Freien steht weiter aus.
