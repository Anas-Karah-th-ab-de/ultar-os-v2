# A30 Audio – ABOX/DPCM-Takt- und Firmware-Pfad (Audio-1)

## Kernerkenntnis: EIO = ABOX-DSP-Firmware fehlt (nicht Route, nicht Takt)
Analyse der Kernelquelle (/home/claude/a30k, sound/soc/samsung/exynos7885_cod3035.c + abox/abox.c):

1. **Takt ist korrekt verdrahtet, läuft automatisch.** Der Machine-Treiber hat LEERE `uaif_ops`
   (kein set_sysclk/hw_params). Der Takt kommt aus dem ABOX-UAIF-DAI selbst:
   - `abox_uaif_startup()` → `clk_enable(clk_bclk[id])` + `clk_bclk_gate[id]` (BCLK an).
   - `abox_uaif_hw_params()` → setzt AUD_PLL (48000-Zweig), `clk_audif`, und
     `clk_set_rate(clk_bclk[id], rate*channels*width)` → ABOX ist **BCLK-Master**.
   - `abox_uaif_trigger(START)` → setzt `ABOX_SPK_ENABLE` in UAIF_CTRL0.
   Diese Kette läuft **automatisch über DPCM**, sobald das UAIF-BE aktiviert wird (FE-PCM geöffnet +
   DAPM-Route verbindet RDMA→UAIF). Rohe Mixer-Writes auf die SPUS/SIFS/UAIF-Mux-Enums aktualisieren
   den DAPM-Graphen (wie Samsungs tinymix im HAL), also ist die Route NICHT das Problem.

2. **Die eigentliche Ursache:** Der ABOX-DSP („Calliope", Cortex-M in der Audio-Box) lädt beim
   Probe per `request_firmware_nowait` **`calliope_sram.bin`** und danach **`calliope_dram.bin`**
   (abox.c ~Z.5543/5888). Ohne laufenden DSP bekommt die RDMA-FE-Mailbox (`abox_rdma_trigger` →
   `pcmtask_msg.trigger`) keine Antwort → der Stream startet nicht → **EIO beim Write**.
   In Ultar war /vendor beim Boot nicht gemountet, als ABOX probte → Firmware-Load schlug fehl →
   DSP DISABLED. Exakt dasselbe Muster wie WLAN (mx140.bin) und Modem (CP-Firmware).

## Werkzeug: `ultar-audio` (crates/ultar-audio, statisch)
- `--status` – prüft, ob calliope_sram.bin/calliope_dram.bin in /vendor/firmware /lib/firmware
  /etc/firmware /firmware/image liegen, zeigt firmware_class.path und das Audio-Kernel-Log.
- `--tone [rdma] [hz]` – setzt Route (SPUS OUT<rdma>→SIFS1→UAIF SPK3/UAIF3) + Codec (Speaker Mute aus,
  HP HP On, Volume) und spielt einen Testton auf pcmC0D<rdma>p. Bei EIO wird das Kernel-Log gezeigt.
- `--play <raw> [rdma]` – 16-bit/48k/Stereo-RAW.

## Geräte-Testanleitung (über die Tür)
1. `upush.ps1 ultar-audio /data/ultar-audio`
2. **/vendor mounten + Firmware suchen:**
   `ucmd.ps1 "mount -t ext4 -o ro /dev/mmcblk0p26 /vendor; ls -la /vendor/firmware | grep -i calliope"`
   (Falls nicht in /vendor/firmware: auch /vendor/etc/firmware, /system/vendor/firmware, kleine
   Partitionen prüfen. Datei ggf. nach /lib/firmware kopieren.)
3. `ucmd.ps1 "/data/ultar-audio --status"` → zeigt, ob Firmware gefunden.
4. **ABOX zur Firmware zeigen + neu anstoßen.** Firmware wird beim Probe geladen; zum Nachladen:
   - `ucmd.ps1 "echo /vendor/firmware > /sys/module/firmware_class/parameters/path"`
   - ABOX-Treiber neu binden (Pfad des Plattform-Device, i.d.R. 0x14a51000.abox):
     `ucmd.ps1 "ls /sys/bus/platform/drivers/abox/"` → Device-Name ermitteln, dann
     `ucmd.ps1 "echo <dev> > /sys/bus/platform/drivers/abox/unbind; echo <dev> > /sys/bus/platform/drivers/abox/bind"`
     (Beim Bind lädt der Treiber die Firmware erneut; im dmesg "SRAM firmware loaded" / "calliope" erwarten.)
   - Falls unbind/bind nicht geht: die calliope_*.bin fest nach /lib/firmware legen und sicherstellen,
     dass /vendor beim BOOT gemountet ist (ultar-init: /vendor-Mount steht bereits), dann neu starten.
5. **Ton testen:** `ucmd.ps1 "/data/ultar-audio --tone 0 440"` – anhören.
   Erwartete dmesg-Zeilen bei Erfolg (kein EIO):
   `abox_uaif_startup[3:P]` · `abox_uaif_hw_params[3:P]` · `Set AUD_PLL rate` · `abox_uaif_trigger[3:P] trigger=1`.
   Tauchen diese NICHT auf → DAPM-Route verbindet FE→BE nicht (dann Route prüfen / anderen rdma-Index
   probieren, z.B. `--tone 7 440`, weil der HAL-Lautsprecherpfad SPUS OUT7 nutzt).
   Kommt `abox ... IPC ... timeout` / `calliope ... not enabled` → Firmware-Boot noch nicht erfolgt (Schritt 4).

## Offen / VERIFY am Gerät
- Genaue Partition/Pfad der calliope_*.bin (VERIFY beim Test).
- Ob unbind/bind des ABOX-Treibers im laufenden System sauber geht, oder ob ein Reboot mit beim Boot
  gemountetem /vendor nötig ist (sauberster Weg).
- Mikrofon nutzt denselben ABOX/Codec-Pfad – sobald der DSP läuft, greift `codec::enable_mic()` + WDMA-Capture.

## ENDGÜLTIGE URSACHE (2026-11-19, am Gerät bestätigt): initcall-Timing, nicht /vendor

Live am A30 über die Tür verifiziert:

- Boot-Log zeigt bei **t≈1,29 s**: `samsung-abox 14a50000.abox: Direct firmware load for
  calliope_sram.bin failed with error -2` (ENOENT).
- `calliope_version` bleibt leer, `reset_count` = 0, PCM-Prepare → `PREPARE: Invalid argument`,
  jeder CPU-Gear-Request → `abox has been disabled`. Der DSP wurde also NIE gebootet.
- ABOX ist ein **built-in-Treiber** und probt während der Kernel-`device_initcalls`. Das läuft
  **bevor** PID 1 irgendetwas mountet. Deshalb hilft der /vendor-Mount (ultar-init) dem ABOX nicht:
  er kommt immer zu spät. (Für Kamera/Video/WLAN reicht er, die fordern Firmware erst bei Benutzung an.)
- Gegenprobe: calliope_*.bin nach **/lib/firmware** auf die Rootfs (mmcblk0p25) kopiert, Reboot →
  trotzdem -2, weil die Rootfs zur initcall-Zeit noch nicht gemountet ist. Nur die in den Kernel
  eingebackene **Initramfs** (`CONFIG_INITRAMFS_SOURCE`, = Ultar-`rootfs/`) ist bei den initcalls da.
- `samsung-abox/unbind` erzwingt zwar eine Neuprobe, löst aber beim Teardown einen **Kernel-Reboot**
  aus (Watchdog) – kein brauchbarer Live-Weg.

### Umgesetzte Lösung (Produktion)
Die DSP-Firmware muss zur initcall-Zeit verfügbar sein → in die eingebackene Initramfs legen:

- **`rootfs/lib/firmware/calliope_sram.bin`** (28544 B, sha256 48757e6d…0043d)
- **`rootfs/lib/firmware/calliope_dram.bin`** (2018400 B, sha256 1950f402…3b89)

`scripts/build-initramfs.sh` kopiert `rootfs/.` komplett ins cpio, der Kernel-Build nimmt es über
`INITRAMFS_SOURCE`. Damit findet der Kernel die Firmware beim `request_firmware_nowait` sofort unter
`/lib/firmware`, der Calliope-DSP bootet, und Wiedergabe/Aufnahme funktionieren.

Zusätzlich robust (optional): im Kernel-Build `CONFIG_EXTRA_FIRMWARE="calliope_sram.bin
calliope_dram.bin"` mit `CONFIG_EXTRA_FIRMWARE_DIR` auf den Ordner mit den beiden Dateien setzen –
dann sind sie fest im Kernel-Image, unabhängig von der Initramfs. (LIESMICH-Hinweis „firmware/ vorher
in den Ausgabeordner kopieren" zielt genau darauf.)

### Validierung
Greift beim **nächsten Kernel-Bau + Boot-Flash**. Nach dem Boot prüfen:
`cat /sys/devices/platform/14a50000.abox/calliope_version` (nicht mehr leer) und `reset_count` (≥1),
dann `ultar-audio --tone` → Ton. Ohne Kernel-Neubau ist der DSP am laufenden Gerät nicht bootbar.

## GELÖST ohne Kernel-Neubau (2026-11-19 abends, am Gerät bestätigt)

Der Kernel hat den Userspace-Rückfall (FW_LOADER_USER_HELPER_FALLBACK): nach dem fehlgeschlagenen Direktladen bei
t≈1,2 s meldet ABOX „SRAM firmware downloading is deferred" und wartet 60 s unter `/sys/class/firmware/calliope_sram.bin/`.
`ultar-init` bedient das jetzt selbst (`fwhelper.rs`: loading=1 → data → loading=0, Auslöser uevent):

    [15.17] SRAM firmware loaded / DRAM firmware loaded
    [15.18] AP_AUDIO_SLSI.bin, APBargeIn_AUDIO_SLSI.bin downloaded
    calliope_version = lSI0

`ultar-audio --tone` → „Wiedergabe beendet (kein EIO)", `ultar-shell --beep` → „Ton OK"; dmesg zeigt die DAPM-Kette
ABOX RDMA0 → SIFS1 → UAIF SPK3 → sma1301 Playback. Die Oberfläche ruft für den Lautsprecherweg `ultar-audio --route <lautstärke>`.
