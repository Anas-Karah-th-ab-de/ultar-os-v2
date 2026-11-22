# Ultar OS auf dem Samsung Galaxy A30 (SM-A305F) – Boot & Display

Stand: Ultar OS **bootet und läuft** auf echter A30-Hardware, Bildschirm zeigt Bild. Kein Exploit, kein eigener Kernel.

## 1. Boot-Weg: system-als-Wurzel (der Schlüssel)

Der Samsung-Bootloader nimmt beim Normalstart unser Boot-Abbild **nicht** (er hängt `skip_initramfs` an, der Stock-Kernel überspringt jedes eigene Ramdisk). Lösung: Ultar kommt **nicht** ins Boot-Abbild, sondern als **Wurzel-Dateisystem in die system-Partition (mmcblk0p25)**.

Die Kernel-cmdline (vom Bootloader) lautet:
```
skip_initramfs rootwait ro init=/init root=/dev/mmcblk0p25 ... androidboot.selinux=enforcing
```
Der Stock-Kernel mountet also mmcblk0p25 direkt als `/` und startet `/init`. Genau dorthin legen wir Ultar.

Wichtig: Die Wurzel wird **direkt als ext4 ohne dm-verity** eingehängt (nur /product, /vendor laufen über verity). Mit einem **leeren vbmeta** (flags=2, Prüfung aus) akzeptiert der Bootloader das modifizierte System.

### Abbild bauen (system.img)
- ext4 mit konservativen Features (4.4-Kernel): `mke2fs -t ext4 -b 4096 -O ^metadata_csum,^64bit,^orphan_file,^huge_file,^metadata_csum_seed,^fast_commit -I 256 -d <rootfs> system.img 256M`
- Inhalt (`rootfs`): Samsungs Recovery-Ramdisk (entpackt) als Basis (hat /system/bin/sh, toybox, Geräteknoten-Umgebung), darüber Ultar: `/sbin/init` = ultar-init, `/usr/bin/*` = ultar-ctl/compat/net/shell, `/usr/bin/uctl`, `/etc/ultar/*`. `/init` = testinit (Vibrations-/Wachhund-Vorlauf, dann exec /sbin/init). Beim `kill`-sicheren Testen chaint testinit zu ultar-init.
- Ins Samsung-Sparse-Format wandeln (sonst lehnt Loke das Schreiben ab, "Typ 4294967295"): 1 Raw-Chunk, Kopf `0xed26ff3a,1,0,28,12,blk=4096,total_blks,1,0` + Chunk `0xCAC1,0,n,12+len`.
- Mit Ultar Flash schreiben: `UltarFlash6.exe --cli flash <pkgdir> <backupdir> --testdevice --yes SM-A305F`. Paket: `device=a30`, `image.system=system.img`, `image.vbmeta=vbmeta.img` (leeres vbmeta aus e1).

## 2. Display: der DECON-Treiber (das zweite harte Problem)

Panel = **EA8076** AMOLED, 1080x2340, über **DECON/DSIM** (Exynos 7904), Treiber `drivers/video/fbdev/exynos/dpu_7885`.

**Der klassische fbdev-Weg (write + FBIOPAN_DISPLAY) zeigt KEIN Bild.** Gründe im Treiber:
- `decon_enable` nimmt aus dem Bootloader-Zustand `DECON_STATE_INIT` eine Abkürzung (setzt nur ON, Panel-Einschaltsequenz läuft nicht). Abhilfe: erst `FBIOBLANK POWERDOWN` (→OFF), dann `UNBLANK` erzwingt die volle Sequenz (Panel + Hintergrundlicht gehen dann an – im Log `ea8076_displayon`, `dsim_enable`).
- Aber selbst dann schiebt `decon_pan_display` (Legacy) kein Bild aufs Panel. Android nutzt es nie.

**Lösung = der Android-Weg: `S3CFB_WIN_CONFIG` mit ION-Puffer.** Bewiesen: Farbfenster (grün) und ION-Bildpuffer (Verlauf) erscheinen.
- ION (Legacy, Kernel 4.4): `/dev/ion`, `ION_IOC_ALLOC` (System-Heap, `heap_id_mask=1`, `EXYNOS_ION_HEAP_SYSTEM_ID=0`) → handle, `ION_IOC_SHARE` → dmabuf-fd, `mmap` → zeichnen.
- `S3CFB_WIN_CONFIG = _IOW('F',209,struct decon_win_config_data)`. Vollbild-Puffer-Fenster: **Standard-Fenster 3**, `idma_type=IDMA_G0 (0)`, `format=XRGB_8888 (4)` (Speicher B,G,R,X = Zeichenfläche 0x00RRGGBB), `fd_idma[0]=dmabuf`, `fence_fd=-1`, `plane_alpha=255`, `src`/`dst` = voll. Rückgabe `d.fence` (Freigabe-Fence) schließen.
- Strukturen bytegenau (CONFIG_DPU_20=y, CONFIG_SUPPORT_MASK_LAYER=y): `sizeof(decon_win_config)=200`, `sizeof(decon_win_config_data)=1040`, `sizeof(ion_allocation_data)=32`. Die C-Union ist 8-byte-ausgerichtet (wegen u64-Adressen in dpp_parm) – im Rust-Modul über eine eigene `WinBuf`-Struktur nachgebildet; Größen per `const assert` abgesichert.

Rust-Umsetzung: `crates/ultar-shell/src/decon.rs` (Modul `Decon`), eingebunden in `display.rs` als Backend `decon:` – wird vor fbdev versucht, wenn `/dev/ion` existiert. Zeichnen → `flip()` (= WIN_CONFIG).

## 3. Live-Tür (Entwicklung ohne Neu-Flashen)

`ultar-init` (diag.rs) startet einen **Befehls-Server über USB** (`/dev/ttyGS0`, ACM): beim Verbinden einmal die Diagnose, danach Zeile für Zeile Shell-Befehle ausführen und Ausgabe zurückschicken (gerahmt mit STX/ETX + `RC=`). Am PC: COM-Port (beim A30 COM13), Helfer `ucmd.ps1` (Befehl senden/lesen) und `upush.ps1` (Binärdatei in Stücken per base64 pushen – serielle Zeilen > ~4 KB brechen sonst ab). Werkzeuge: `uctl` (fbinfo/fbblank/fbfill), `windec`/`windec2` (WIN_CONFIG-Farbfenster / ION-Bildpuffer-Test). So wurde der DECON-Weg live gefunden.

Kernel-Log lesen: `/dev/kmsg` und `dmesg` sind bei `console=ram` teils leer; der `syslog`-Syscall (klogctl, Typ 3) liefert den Ringpuffer – in diag.rs eingebaut, Abschnitt "kernel: anzeige" filtert decon/dsim/panel.

## 4. Offen / nächste Schritte (Optimierung)
- **Rust-DECON-Weg auf dem Gerät testen** (ist gebaut, noch nicht geflasht): system.img neu bauen mit der neuen ultar-shell, flashen, prüfen dass die Oberfläche erscheint.
- **Farbgenauigkeit/Qualität** (Nutzer-Wunsch "besser als Android"): Samsungs mdnie-Farb-Engine konfigurieren (Standard/Basis-Modus für korrekte Farben), Gamma, native 1080x2340 1:1 (kein Skalieren), ggf. 10-bit prüfen. Hintergrundlicht über `/sys/class/backlight/panel/brightness` (max 365) statt Software-Dimmen.
- **Doppelpufferung** (zwei ION-Puffer, abwechselnd) gegen Tearing; auf den Freigabe-Fence warten.
- WLAN-Aktivierung scheitert noch (os error 22) – Treiber/Firmware.
- Kein /compat (Ubuntu-Chroot) und /data mitgeflasht → Browser/Apps-Chroot und dauerhafte Daten brauchen eigene Partitionen/Labels.

## 5. Farbformat (gelöst)
Die Zeichenfläche ist 0x00RRGGBB (Speicher B,G,R,X). Das korrekte DECON-Format ist **BGRX_8888 (7)**, nicht XRGB –
am A30 per Kamera verifiziert (reine Rot/Grün/Blau-Vollbilder: nur Format 7 zeigt alle drei korrekt; XRGB zeigte
Rot als Grün → Grünstich). In `decon.rs` gesetzt. Kamera-Abgleich: ffmpeg (dshow, "HD Pro Webcam C920"),
`ffmpeg -f dshow -i video="HD Pro Webcam C920" -frames:v 1 -update 1 -y cam.jpg`, dann in die Sandbox stagen und ansehen.

## 6. WLAN/BT/Mobilfunk (Chip läuft, Supplicant offen)
Der A30-WLAN/BT-Chip ist **Samsung SCSC/Maxwell** (wlbt). Treiber im Kernel geladen: scsc_wlan, scsc_mx, scsc_bt,
scsc_platform_mif. Schnittstellen wlan0, swlan0, p2p0 existieren.
- **Firmware:** /vendor/etc/wifi/mx140.bin (+ *.hcf). Der Treiber lädt sie beim ersten `wlan0 up` (slsi_start →
  mxman_start → mx140_file_download_fw). Scheiterte zuvor, weil **/vendor nicht gemountet** war
  (`mx140_request_file: vfs_stat() failed for /vendor/etc/wifi`, Error -11). **Fix:** /vendor (A30: mmcblk0p26) und
  /efs (mmcblk0p3) ro in init.toml gemountet. Danach: `SM_MSG_START_CFM from the firmware`, HW Version 0xA300, MAC gesetzt.
- **Echte MAC** in /efs/wifi/.mac.info (A4:D9:90:87:48:AF); ohne /efs nimmt der Treiber eine Default-MAC.
- **Supplicant (offen):** Samsungs /vendor/bin/hw/wpa_supplicant ist ein Android-Binary und läuft nicht
  (libssl.so u. a. lagen auf der ersetzten system-Partition). Nächster Schritt: **statisches wpa_supplicant**
  (CONFIG_TLS=internal, CONFIG_CRYPTO=internal) oder nl80211 direkt in ultar-net, dann Scan/Assoziation + DHCP.
  `iw` fehlt ebenfalls und müsste statisch mit.
- **Mobilfunk/BT:** eigener, größerer Schritt (Modem-RIL bzw. BT-Stack).

## 7. Werkzeuge/Abläufe dieser Sitzung
- Live-Tür: `ucmd.ps1 "<cmd>"` (Shell über COM13), `upush.ps1 <lokal> <dev>` (Binär in Stücken pushen).
- Display-Diagnose: `uctl fbinfo/fbblank/fbfill`, `windec`/`windec2` (WIN_CONFIG Farb-/ION-Puffer, Format/Farbe als Argument).
- vendor mounten zum Prüfen: `mount -o ro /dev/mmcblk0p26 /vendor`; efs: `/dev/mmcblk0p3 /efs`.
- WLAN-Chip hochfahren (Test): `/vendor` mounten, dann `ifconfig wlan0 up` → dmesg zeigt Firmware-Load.

## 8. KRITISCH: Ultar-Binaries MÜSSEN statisch gelinkt sein
Die aarch64-Rootfs hat keinen passenden dynamischen Linker/glibc für frisch gebaute Binaries.
Ein dynamisch gelinktes Binary scheitert am Gerät mit "No such file or directory" (fehlender
Interpreter /lib/ld-linux-aarch64.so.1):
- dynamische **/sbin/init** -> Kernel kann PID1 nicht starten -> Kernel-Panik -> Reboot-Schleife (Dauervibration)
- dynamische **ultar-shell** -> UI startet nicht -> hängt am Samsung-Logo
Deshalb ALLE Ultar-Binaries statisch bauen:
`RUSTFLAGS="-C target-feature=+crt-static" RUSTC_BOOTSTRAP=1 cargo build --target aarch64-unknown-linux-gnu --release -Z build-std=std,panic_abort`
Statische init ~1,32 MB (wie das bewährte s3-Original), dynamisch nur ~0,66 MB -> Größe als schneller Check.
Kontrolle: `file …/ultar-shell` muss "statically linked" zeigen; `readelf -l` darf KEINEN Interpreter listen.
(Sandbox ohne aarch64-std: std aus /usr/src/rustc-*/library per build-std bauen, da static.rust-lang.org gesperrt ist.)

## 9. /data (userdata) erst nach dem Watchdog mounten
Der Auto-Mount der großen userdata-ext4 (A30: mmcblk0p32, ~51 GB) darf NICHT im kritischen Boot-Pfad
vor dem Watchdog laufen (Reset-Risiko). ultar-init ruft `mount_userdata_bg()` erst nach `diag::start()`
(Watchdog aktiv) in einem eigenen Thread auf. Dateisystem ist ext4 (nach Werksreset), nicht f2fs.

## 10. Live-Entwicklung ohne Flashen (Werkzeugkette PC)
- PC-Image-Bau in WSL (Ubuntu): `simg2img` s3 -> raw, `resize2fs raw 400M`, loop-mounten,
  Binaries ersetzen, `img2simg` -> Samsung-Sparse; sha256 in ultar-package.txt aktualisieren.
  (Scripts: UltarFlash/build_image*.sh)
- Flashen: `UltarFlash6.exe --cli flash <pkgdir> a30-RR8M6060BMP --testdevice --yes SM-A305F` (Download-Modus, COM12).
- Tür (COM13): ucmd.ps1 "<cmd>" / upush.ps1 <lokal> <dev>. PowerShell frisst $-Variablen und $( ) -> in
  Türbefehlen keine Shell-Variablen/Substitutionen verwenden; längere Logik als .sh-Datei ablegen und ausführen.

## 11. Ton/Audio A30 (Exynos ABOX + Cod3035) – Stand
Codec **Exynos7885-COD3035**, Wiedergabe über /dev/snd/pcmC0D0p (SPUS0/RDMA0). Mixer-Werkzeug: tools/audctl.c
(statisch, Modi dump/set/setname/setenum/tone). WICHTIG beim Enum-Lesen: items auf <=128 begrenzen, sonst
Absturz (der [min..max]-Wert im INFO ist bei Enums kein Bereich).
Lautsprecher-Route aus /vendor/etc/mixer_paths.xml: **media-speaker = route-spkamp-playback + dev-speaker**:
`ABOX NSRC3=UAIF3, ABOX SPUS OUT7=SIFS1, ABOX SIFS1=SPUS OUT7, ABOX UAIF SPK3=SIFS1, HP HP On=1`
plus `Speaker Mute Switch=0` (war stumm!), `DAC Soft Mute=0`, `Speaker Volume ~150`.
Defaults: `ABOX UAIF SPK0=SIFS0, UAIF SPK2=SIFS2` (vom Maschinentreiber getaktete Codec-Links).
FORTSCHRITT: Route per audctl setzbar, PCM PREPARE gelingt (vorher EBADFD). ABER PCM-Write -> **EIO (os err 5)**.
URSACHE: Rohe Mixer-Schreibzugriffe setzen nur die MUX-Auswahl, lösen aber die ASoC-DAPM-Stromsequenz des
Codecs nicht aus (UAIF-Takt/PLL, DAC, SPK-Verstärker bleiben aus). 
NÄCHSTER SCHRITT: eigener Audio-Pfad, der die DAPM-Widgets des Cod3035 (DAC/AIF/SPK) korrekt schaltet bzw. die
komplette HAL-Init-Sequenz (Default-ctls der mixer_paths.xml) nachfährt; ggf. tinyalsa-ASoC statt roher ctl-Writes.

## 12. Screenshot ohne Kamera (autonome Sicht) – FUNKTIONIERT
ultar-shell speichert auf **SIGUSR1** den aktuellen Bildschirm als PNG nach /data/ultar/screen.png
(main.rs: SHOT-Flag + save_screenshot aus cv.px 0x00RRGGBB; Handler weckt den Poll über die Wake-Pipe).
Aufruf über die Tür: `kill -USR1 <pid von ultar-shell>` (pidof geht nicht über PowerShell-$()).
Abholen: **tools/pc/upull.ps1** holt die Datei in sauberen 2046-Byte-Blöcken (durch 3 teilbar -> base64
ohne Padding in der Mitte, Konkatenation korrekt). Ein einzelner 230-KB-Transfer verliert sonst Bytes
(115200 Baud ohne Flusskontrolle, Pufferüberlauf). md5 stimmte byte-genau. Dann in WSL base64 -d,
in die Sandbox stagen, mit PIL unter 2000px skalieren und ansehen.

## 13. Live-Shell-Update ohne Flashen
Root ist rw (ext4, kein dm-verity dank leerem vbmeta). Neue ultar-shell:
`mount -o remount,rw /; mv /usr/bin/ultar-shell /usr/bin/ultar-shell.old; cp /data/us /usr/bin/ultar-shell; kill <pid>`
Der Supervisor (ultar-init) startet die neue Shell neu. Grosse Binaries vorher gzippen (~6 MB -> 2,7 MB)
und auf dem Geraet mit `gzip -d` entpacken; "Text file busy" beim cp -> erst die laufende per mv beiseite.

## 11b. Ton – Schlussfolgerung (DAPM-Grenze)
Getestet: Routen über SIFS1/SPK3, SIFS2/SPK2 und den default-getakteten SIFS0/SPK0 — alle setzbar,
alle enden beim PCM-Write mit **EIO**. Damit steht fest: das Problem ist NICHT die Routenwahl, sondern
der **Codec/UAIF-Takt**. Rohe Mixer-Writes setzen nur die MUX-Auswahl; sie lösen die ASoC-**DPCM**-Stromsequenz
nicht aus: das Backend-DAI (UAIF) wird nicht gestartet und der Cod3035 (DAC/AIF/SPK-Amp, Masterclock) bleibt aus.
Richtiger Ton braucht den vollen ASoC-Weg: tinyalsa + komplette HAL-Mixer-Sequenz INKL. der Default-ctls am
Dateianfang (die die UAIF-Takte/PLL aufsetzen) und der Codec-DAPM-Widgets, damit beim Öffnen der FE (RDMA0)
das BE (UAIF) über DPCM mit Takt gestartet wird. Das ist eine eigene, größere Audio-Implementierung,
kein ioctl-Quickfix. (Mixer-Mute ist aus, Lautstärke/Gain gesetzt — nur der Takt fehlt.)

## Sync-Dateien je Bild schließen (Leck behoben)

`S3CFB_WIN_CONFIG` gibt je Bild Sync-Dateien zurück: `fence` fürs ganze Bild und eine `rel_fence` je aktivem
Fenster. Die Oberfläche schloss nur `fence`; die Freigabe-Fences blieben offen (etwa 4 je Sekunde bei
bewegtem Bild). Nach 1024 offenen Dateien meldet der Kernel bei jedem Bild
`decon: decon_create_release_fences: failed to get unused fd`, und alles, was in der Oberfläche noch eine
Datei öffnen will, schlägt fehl. `decon.rs::flip` schließt jetzt alle zurückgegebenen Nummern.
Gemessen am A30: vorher 20 → 43 offene `sync_fence` in 6 s Bewegung, danach dauerhaft 0.
Prüfen: `ls -l /proc/$(pidof ultar-shell)/fd | grep -c sync_fence`.
