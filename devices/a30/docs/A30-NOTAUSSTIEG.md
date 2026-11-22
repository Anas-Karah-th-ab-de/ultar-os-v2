# Ultar A30 – Notausstieg bei hängendem System + Treiberstand

Stand 19.11.2026. Code: `crates/ultar-init/src/rescue.rs`, Tür-Zeitlimit in `diag.rs`, Lebenszeichen in `ultar-shell/src/main.rs`.

## 1. Notausstieg (läuft in PID 1, unabhängig von der Oberfläche)

**Laut + Leise gleichzeitig halten:**

| gehalten | Rückmeldung | Aktion beim Loslassen |
|---|---|---|
| ≥ 2 s | 1× kurz vibrieren | **Stufe 1:** Oberfläche + Browser-Sitzung (weston/epiphany/WebKit/librespot) hart beenden → init startet die Oberfläche neu |
| ≥ 6 s | 2× vibrieren | **Stufe 2:** alle Dienste hart beenden (kill -1) → init startet sie neu; Tür und Wachhund laufen weiter |
| ≥ 10 s | lang vibrieren | **Stufe 3 sofort:** sync (max. 4 s), Dateisysteme read-only (SysRq u), Neustart |

- Kurzes gemeinsames Drücken (< 2 s) tut nichts; einzelne Tasten bleiben Lautstärke.
- Nach einer Aktion 3 s Sperrzeit.
- Nach Stufe 1/2 erscheint eine Benachrichtigung („Notausstieg …“, in der Systemsprache).
- Auch ohne Tasten: `ultar-ctl rescue 1|2|3`.

**Wenn der Kernel selbst steht:** Leiser + Ein/Aus ~10 s halten = Zwangsneustart durch Samsung (PMIC/Bootloader). Zusätzlich setzt
der Hardware-Wachhund (von init gefüttert) das Gerät zurück, und `kernel.panic = 5` startet nach einer Kernel-Panik neu.

## 2. Automatik

- **Wachhund der Oberfläche:** `ultar-shell` schreibt `/run/ultar/ui.alive` (höchstens alle 5 s). Älter als 150 s → init beendet die
  Oberfläche wie Stufe 1 (Leerlauf-Schlaf der Oberfläche ist max. 60 s, Browser-Schleife schreibt ebenfalls). Alte Shell-Fassungen ohne
  Lebenszeichen werden nicht angefasst.
- **Firmware-Nachlader** (`crates/ultar-init/src/fwhelper.rs`): bedient Firmware-Anfragen des Kernels sofort aus /lib/firmware, /vendor/firmware, /vendor/etc/wifi, /data/ultar/firmware; fehlende Firmware wird sofort abgelehnt (kein Warten). Auch für fest eingebaute Treiber, die vor PID 1 anfragen (Audio-DSP).
- **Firmware-Zeitlimit 10 s als Reserve** (`/sys/class/firmware/timeout`, vorher 60 s): fehlende Firmware ließ Kamera-/MFC-Knoten beim `open`
  60 s im Kernel hängen – das war die Hauptursache der Einfrierer.
- **Tür (COM13) mit Zeitlimit:** jeder Befehl max. 120 s, dann SIGKILL an die ganze Prozessgruppe; `@<sek> befehl` setzt ein eigenes
  Limit. Bei Abbruch listet die Tür hängende Kernel-Aufrufe. Vorher konnte z. B. `cat /proc/kmsg` die Tür für immer blockieren.
- **Protokoll:** jede Rettung schreibt nach `/run/ultar/rescue.log` und `/data/ultar/rescue.log` alle Threads im Zustand **D**
  (hängen im Treiber) mit Kernel-Funktion (`wchan`) – damit ist sichtbar, welcher Treiber hängt.
  Hinweis: Prozesse im Zustand D sterben erst, wenn der Treiber zurückkehrt – das Firmware-Zeitlimit sorgt dafür, dass das schnell geht.

## 3. Treiberstand A30 (live geprüft 19.11.2026 abends, Uptime 254 s)

| Bereich | Stand | Offen |
|---|---|---|
| Display, Touch, Tasten, Speicher (/data 51 GB), Akku | ✅ | – |
| /vendor + Firmware | ✅ beim Boot gemountet; Pfad bleibt /vendor/firmware (wurde vorher von ultar-compat/Treibersuche auf /compat überschrieben → Video/Kamera fanden nichts) | – |
| WLAN | ✅ verbunden (Code-Craft-tech, 5 GHz, −59 dBm, DHCP 192.168.100.78) | WPA3/SAE |
| Sensoren | ✅ | – |
| GNSS | ❌ /dev/gnss_ipc öffnet, keine Daten | Kepler-Firmware finden + booten. Oberfläche öffnete den Knoten bei jedem Bild (Hunderte dmesg-Zeilen/s) → jetzt max. 1×/s |
| Kamera | ❌ Knoten da, hängt nicht mehr; einfaches V4L2 wird abgelehnt (REQBUFS -22, Treiber verlangt Multi-Planar + Samsung-HAL-Ablauf) | fimc-is-Ablauf nachbauen (Sensor-Knoten, S_INPUT, Stream-Konfiguration, Multi-Planar) – großes Paket |
| Video-Decoder (MFC) | ⚠️ mfc_fw.bin jetzt verfügbar | ungetestet |
| Ton | ✅ DSP bootet (19.11. abends): Firmware-Nachlader in ultar-init bedient die Kernel-Anfrage über sysfs – kein Kernel-Neubau nötig. `ultar-audio --tone` und `ultar-shell --beep` laufen ohne Fehler, Verstärker SMA1301 wird eingeschaltet | Hörprobe am Gerät; Mikrofon/Anruf-Ton |
| SIM/Mobilfunk/mobile Daten | ⚠️ Modem bootet bis ONLINE (19.11. nachts), Funkdienst liest IMEI und findet das Netz (41701, nur Notrufe); **SIM meldet „keine Karte"** | SIM-Erkennung klären, dann PIN/Anmeldung/Daten/Anrufe – siehe A30-MODEM.md |
| Bluetooth | ⚠️ eigener Host `ultar-bt` über /dev/scsc_h4_0: Chip startet, Suche klassisch + LE läuft, Bildschirm in den Einstellungen | Koppeln und Profile (Tastatur/Maus, Audio) – siehe A30-BLUETOOTH.md |
| Externe Helfer im Image | ❌ cryptsetup, wg, curl, unotify fehlen | statisch bauen, ins Image |
| Kleinigkeit | `ultar-compat` bleibt als Zombie stehen | Reaper/Startskript prüfen |
