# Ultar A30 – Geordneter Testplan (nach Neustart)

Reihenfolge zum Abarbeiten, sobald das Gerät neu gestartet ist und die Tür (COM13) antwortet.
Werkzeuge liegen unter /mnt/user-data/outputs/ bzw. werden auf das Gerät nach /data/ gepusht.
Tür: `ucmd.ps1 "<cmd>"`, Push `upush.ps1 <lokal> <dev>`, schnell: `upush2.ps1`/`upull2.ps1`.

## 0. Grundstatus
1. Port prüfen: COM13 da = gebootet, COM12 = Download-Modus.
2. Tür: `ucmd.ps1 "uname -a; cat /proc/uptime"` → antwortet.

## 1. Neue Oberfläche einspielen (ohne Flashen)
Root ist rw. Neue ultar-shell (mit Kontrollzentrum/Sensoren/GPS/Telefon/Nachrichten/Kamera):
1. `upush.ps1 ultar-shell.gz /data/us.gz` (gzip ~3 MB)
2. `ucmd.ps1 "cd /data; gzip -d -f us.gz; mount -o remount,rw /; mv /usr/bin/ultar-shell /usr/bin/ultar-shell.old; cp /data/us /usr/bin/ultar-shell; chmod 755 /usr/bin/ultar-shell; kill <pid ultar-shell>"`
3. Screenshot: `kill -USR1 <neue pid>` → `upull2.ps1 /data/ultar/screen.png screen.png` → in WSL dekodieren, ansehen.
   Erwartung: Home mit Telefon/Nachrichten/Kamera/Datenschutz; Einstellungen → Datenschutz & Sicherheit.
(Alternativ sauberer Flash: Image s11d, `UltarFlash6.exe --cli flash neu/s11d a30-RR8M6060BMP --testdevice --yes SM-A305F`.)

## 2. Speicher (bereits bestätigt)
`ucmd.ps1 "df -h /data"` → ext4 ~51 GB (mmcblk0p32), vom Boot automatisch gemountet.

## 3. Sensoren
1. `upush.ps1 senstest /data/senstest`
2. `ucmd.ps1 "/data/senstest"` → zeigt sysfs-Struktur je Sensor (accel/gyro/light/prox/magnet/step…).
3. Bei Samsung: Werte oft erst nach `echo 1 > /sys/class/sensors/<x>/enable`. Kandidatenlisten in sensors.rs ggf. anpassen.
4. UI: Einstellungen → Sensoren, Screenshot → Live-Werte x·y·z / lux / Näherung / Schritte.

## 4. Tasten (Laut/Leise/Power)
Am Gerät drücken; Lauter/Leiser → Lautstärke-Toast, Power → Schirm aus/an. (Aktiv in input.rs/main.rs.)

## 5. WLAN
1. `/vendor` mounten (Firmware): `ucmd.ps1 "mount -t ext4 -o ro /dev/mmcblk0p26 /vendor"`.
2. Chip hoch: `ucmd.ps1 "ifconfig wlan0 up; dmesg | grep -i slsi | tail"` → Firmware-Load, MAC.
3. Scan nativ: `ucmd.ps1 "/usr/lib/ultar/iw dev wlan0 scan | grep SSID | head"`.
4. UI: Einstellungen → Netzwerk → Netz wählen, Passwort → wpa_supplicant (nativ, statisch) + eigener DHCP.

## 6. GNSS/GPS
1. `upush.ps1 gnsstest /data/gnsstest`; `ucmd.ps1 "/data/gnsstest; ls -la /dev/gnss_ipc; dmesg | grep -i gif"`.
2. SLSI „Kepler" über /dev/gnss_ipc; bootet erst mit Firmware (suchen: /vendor/firmware, kleine Partitionen p17/18/20/21).
3. UI: Einstellungen → Standort (GPS), Screenshot → Fix/Satelliten.
HINWEIS: Kepler-Firmware-Boot ist noch offen (eigenes Paket) – zuerst Knoten/dmesg prüfen.

## 7. Kamera
1. `upush.ps1 camctl /data/camctl` (NEU: fork+2s-Timeout, hängt die Tür nicht mehr auf).
2. `ucmd.ps1 "/data/camctl list"` → Capture-Nodes (card nennt MCSC/scaler). Blockierende Nodes werden übersprungen.
3. Je Kandidat: `ucmd.ps1 "/data/camctl grab /dev/videoN /data/cam.ppm"`; `upull2.ps1 /data/cam.ppm cam.ppm` → PNG, ansehen.
4. Fehler STREAMON/„kein Frame" → Exynos-ISP-Pipeline (media-ctl) und/oder ISP-Firmware aus /vendor/firmware nötig (siehe docs/A30-KAMERA.md).
5. UI: Kamera-App (öffnet Node nur auf Antippen).

## 8. SIM / Mobilfunk (Shannon SS310AP, modem_v1)
1. `upush.ps1 ultar-modem /data/ultar-modem`.
2. Firmware: `ucmd.ps1 "dd if=/dev/mmcblk0p19 of=/data/cp.bin bs=1M"` (p19 = TOC/BOOT/MAIN/VSS/NV, bestätigt).
3. Trocken: `ucmd.ps1 "/data/ultar-modem --fw /data/cp.bin"` → saubere TOC-Stufen.
4. Scharf (KORRIGIERTE Reihenfolge: MODEM_ON→XMIT_BOOT→SECURITY_REQ→DL_START→BOOT_ON→…→STATUS):
   `ucmd.ps1 "/data/ultar-modem --fw /data/cp.bin --arm 2>&1 | tee /data/modem.log; dmesg | tail -40"`
   Erwartung: `ss310ap_boot_on: +++ … ---` (KEIN EACCES), cp_status kommt, Status → ONLINE.
5. Fehlversuch → `ucmd.ps1 "/data/ultar-modem --reset"` (MODEM_OFF+RESET), dann Schritt 4 erneut.
6. Danach offen: RIL auf SIPC5 (SIM lesen/Netz registrieren) – eigenes großes Paket; dann app_phone-Backend tauschen.

## 9. Ton/Audio (dokumentiert, braucht ASoC-Impl)
`audctl` + mixer_paths.xml: Route setzbar, aber PCM-Write EIO (Codec/UAIF-Takt über rohe Mixer nicht hochfahrbar).
Nächster Bau: tinyalsa-ASoC + volle HAL-Init + Codec-DAPM. Mikrofon teilt dieselbe Grenze.

## 10. Bluetooth
`ucmd.ps1 "ls /sys/class/bluetooth; cat /proc/modules | grep scsc_bt"`; HCI hochbringen (hciconfig/btmgmt – Werkzeug noch offen).

## 11. Datenschutz & Sicherheit (Kontrollzentrum)
Einstellungen → Datenschutz & Sicherheit: „Gerät aktiv" aus → alles aus (Screenshot).
Einzelne Gruppen testen: Netzwerk aus → `iw dev wlan0 scan` schlägt fehl / Schnittstelle unten; Sensoren/Kamera/GPS-Screens zeigen „deaktiviert".
Durchsetzung: /run/ultar/privacy + Schnittstellen-ioctl + rfkill.

## 12. Sicherheit, Cloud & VPN (neu — erst nach Grundfunktionen testen)
Voraussetzung am Gerät: statische Werkzeuge in /usr/lib/ultar/ (siehe rootfs/.../README-sicherheit-cloud.md)
und erreichbarer Server ultar.code-craft.tech (oder Test-IP in /data/ultar/cloud.conf).

- **Verschlüsselung:** Einstellungen → Sicherheit → „Gerät verschlüsseln" (ACHTUNG: löscht /data,
  nur auf Testgerät). Neustart → Passwortabfrage → /data entschlüsselt gemountet.
  Tür: `ucmd.ps1 "/usr/bin/usecurectl status"`.
- **Verified Boot:** mkverity.sh im Image; stage0 als /init. Prüfen: bootet; absichtlich ein Byte in
  der system-Partition ändern → Start muss anhalten („System verändert").
- **Konto/Nummer:** Einstellungen → Konto → anmelden → „Meine Nummer holen". Nummer erscheint in
  Systeminfo + Telefon. Tür: `ucmd.ps1 "/usr/bin/ucloud whoami"`.
- **Internet-Anruf:** zwei Geräte (oder Gerät + Test-Client) mit je eigener Nummer; anrufen, klingeln,
  annehmen, auflegen. Medien (Audio/Video) nach Fertigstellung der webrtc-Engine (A30-ANRUFE.md).
- **VPN:** Einstellungen → VPN → Verbinden. Prüfen: `ip route` Default über wg0; ohne Tunnel kein
  Verkehr (Killswitch nft ultarks). Tür: `ucmd.ps1 "/usr/bin/uvpn status"`.
- **Store/Update:** Einstellungen → Ultar Store → „Apps laden"; System-Update → „Nach Updates suchen".
