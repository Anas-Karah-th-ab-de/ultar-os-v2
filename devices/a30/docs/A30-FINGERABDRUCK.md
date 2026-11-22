# A30 – Fingerabdruck (Stand 20.11.2026, Kette steht – Praxistest mit Finger offen)

## Hardware / Ausgangslage
- Sensor **Goodix GW36H3** (`/sys/class/fingerprint/fingerprint`, `/dev/goodix_fp` = nur Steuerung/Interrupt).
- Bilddaten und Abgleich laufen in Samsungs Sicherheitsumgebung **TEEgris** (`/dev/tzdev`, `/dev/tziwsock`,
  Trustlets in `/vendor/tee`). Linux kann den Finger nicht selbst lesen.
- Vorhanden: Binder (`/dev/binder`, `/dev/hwbinder`, `/dev/vndbinder`), `CONFIG_TZDEV=y`,
  `/vendor/bin/tzdaemon`, `tzts_daemon`, `libteecl.so`, `libgf_in_system_lib.so`,
  `vendor.samsung.hardware.biometrics.fingerprint@3.0-service`.

## Weg: Samsungs Dienste in einer abgeschotteten Android-Umgebung (chroot)
1. **WSL:** `tools/android-fp/1-entpacken-wsl.sh` – Original-System (Ultar-Flash-Sicherung
   `a30-RR8M6060BMP/partitions/system.img.lz4`, Android 11) entpacken.
2. **WSL:** `tools/android-fp/2-packen-wsl.sh` – nur Laufzeit (bin, lib64, etc, APEX runtime/vndk/i18n …) → 347 MB.
3. **Übertragung per WLAN** statt USB-Tür (14,7 MB/s statt 0,06 MB/s): auf dem A30
   `toybox nc -l -p 9000 > /data/android/aroot.tar.gz`, am PC per TCP senden (PowerShell TcpClient).
4. **A30:** `tools/android-fp/3-umgebung.sh` – chroot unter `/data/android/root` mit /vendor, /dev, /proc,
   /sys, APEX-Bindungen, VNDK, `linkerconfig`, EFS nur lesend.

## Erreicht
- ✅ Android-Programme laufen im chroot (`android-ok`), Linker-Konfiguration erzeugt.
- ✅ `hwservicemanager` läuft (lädt HIDL-Manifeste von System und Vendor).

## Fortschritt 20.11.2026 (nachts)
- `strace` (statisch, aarch64) zeigt: `tzdaemon` öffnet `/dev/tzdev` erfolgreich (ioctl ok) und will dann eine
  System-Property setzen (`/dev/socket/property_service`) → ohne Property-Dienst „Exiting".
- Gelöst mit `tools/android-fp/propsvc.c`: minimaler Property-Dienst (alte und neue Setz-Protokolle), schreibt
  `name=wert` nach `/data/android/log/props.txt`. Ohne `/dev/__properties__` liefert bionic Standardwerte.
- Kernel-Panik in `mmc_srpmb_worker` beim ersten Lauf → Ursache gefunden und behoben (siehe unten).
- Wichtig für Skripte über die Tür: Hintergrundprozesse immer mit `>log 2>&1 </dev/null` starten, sonst
  wartet die Tür auf deren Ausgabe.

## Durchbruch 20.11.2026 (früh) – Samsung-Fingerabdruckdienst läuft und ist registriert
- **Kernel-Panik geklärt (nicht die Kamera):** `mmc_srpmb_worker` öffnet fest `/dev/block/mmcblk0rpmb`,
  prüft das Ergebnis nicht (x0 = -2/ENOENT → NULL-Zeiger). Ultar hat kein `/dev/block/`. Sobald TEEgris RPMB
  liest (beim Start von `tzts_daemon`), stürzt der Kern ab. **Fix:** `3-umgebung.sh` legt `/dev/block/mmcblk0*`
  an → RPMB-Lesen klappt („Read request / Sent reply"), kein Absturz mehr.
- **Echter Property-Bereich:** `propsvc.c` legt jetzt `/dev/__properties__` als bionic-„pre-split"-Datei an
  (Trie wie bionic `prop_area`, Futex-Benachrichtigung, lange `ro.`-Werte), lädt `build.prop`/`default.prop`
  und `androidboot.*`. Ohne Bereich hingen `tzts_daemon` (Abbruch nach 30 s „waiting for property") und der
  Fingerabdruckdienst (100 % CPU in einer Warteschleife). Aufruf:
  `propsvc /dev/socket/property_service <protokoll> /dev/__properties__ <prop-dateien…>`.
- **`logdw.c`:** Mini-logd (`/dev/socket/logdw`) → Android-Logs lesbar in `/data/android/log/logcat.txt`.
- Ergebnis auf dem A30:
  - `tzdaemon`: „Probe for TZ successful", Property `vendor.tzdaemon=Ready`.
  - `tzts_daemon`: 3 TEE-Treiber geladen (MST, TIMA, VaultKeeper), `vendor.tzts_daemon=Ready`.
  - Fingerabdruckdienst: `Registered vendor.samsung.hardware.biometrics.fingerprint@3.0::ISehBiometricsFingerprint/default`.
- Startreihenfolge: Umgebung (Skript 3) → propsvc → logdw → hwservicemanager → tzdaemon → tzts_daemon → Fingerabdruckdienst.

## Kette bis zur Oberfläche (20.11.2026)
- **`fpclient.c`** – HIDL-Client direkt über `/dev/hwbinder` (ohne Android-Bibliotheken, statisch):
  hwservicemanager `get`, `IBiometricsFingerprint@2.1` (setNotify mit eigenem Callback-Binder, setActiveGroup,
  enumerate, authenticate, preEnroll/enroll/postEnroll, cancel, remove) und `IGatekeeper@1.0` (enroll/verify).
  Getestet: Trustlet antwortet (`TEEC_SUCCESS`), `enumerate` → 0 Finger, `authenticate` ohne Finger → Fehler 8/1004.
- **Gatekeeper/HAT:** Anlernen verlangt ein HardwareAuthToken („enroll hat is 0" ohne). Gatekeeper-TA speichert in
  `/mnt/vendor/efs/tee` → EFS bleibt nur lesend, eine Kopie unter `/data/android/efs-tee` wird darübergelegt.
  Damit: Gatekeeper enroll → Handle, verify(challenge aus preEnroll) → 69-Byte-HAT, enroll startet die Sitzung
  (ohne Finger nach Ablauf Fehler 3 = Zeit). Das Geräte-Geheimnis für Gatekeeper ist intern (`gk-ultar.handle`);
  geschützt wird über Ultars eigene PIN.
- **Dauerdienst `fpclient daemon /run/ultar/fp.sock`:** Samsungs HAL beendet sich, sobald sein Client weg ist
  → der Vermittler hält die Verbindung. Zeilenprotokoll: `auth | learn | cancel | count | remove <fid>` →
  `acq n | ok <fid> | nein | fehler <code> <vendor> | los | rest n | fertig <fid> | anzahl n | entfernt | err …`.
- **Ultar-Dienst `fingerabdruck`** (`rootfs/etc/ultar/services/fingerabdruck.toml` → `/data/android/fp-dienst.sh`):
  Umgebung, propsvc, logdw, hwservicemanager, tzdaemon, tzts_daemon, Keymaster, Gatekeeper, dann Schleife
  HAL + Vermittler. Läuft auf dem A30 (`ultar-ctl status`: fingerabdruck läuft).
- **ultar-shell** (`fp.rs`, `app_sys.rs`): Einstellungen → Bildschirmsperre → Abschnitt Fingerabdruck
  (Anzahl, „Mit Fingerabdruck entsperren" nur mit PIN, „Fingerabdruck hinzufügen" mit Fortschritt, „Alle löschen").
  Sperrbildschirm: prüft automatisch, solange er sichtbar ist; erkannt → entsperrt, sonst „nicht erkannt"
  + Vibration; nach 5 Fehlversuchen (Fehler 7) 30 s Pause, PIN geht immer. Bildschirmfoto bestätigt: Anzahl 0.

## Praxistest (braucht einen Finger am Gerät)
1. Einstellungen → Bildschirmsperre → PIN festlegen.
2. „Fingerabdruck hinzufügen" → Finger mehrmals auflegen, bis „Fingerabdruck hinzugefügt".
3. „Mit Fingerabdruck entsperren" einschalten → Bildschirm aus/an → Finger auflegen → entsperrt.
Protokolle: `/data/android/log/fpd.log`, `/data/android/log/logcat.txt`.

## Offen / nächste Schritte
- Praxistest mit echtem Finger (siehe oben).
- Aufwecken per Finger bei ausgeschaltetem Bildschirm (derzeit prüft Ultar nur bei sichtbarem Sperrbildschirm).
- `servicemanager`/`vndservicemanager` (SELinux) werden für den Fingerabdruck nicht gebraucht.
- Die Umgebung verschwindet bei jedem Neustart (Mounts) – der Dienst `fingerabdruck` baut sie beim Start neu auf.
