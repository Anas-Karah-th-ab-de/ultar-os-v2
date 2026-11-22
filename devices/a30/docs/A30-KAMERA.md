# A30 Kamera (V4L2 / Exynos FIMC-IS)

## Was gebaut wurde
- **crates/ultar-shell/src/camera.rs** – V4L2-Aufnahme-Modul (als `mod camera;` in main.rs eingebunden).
  `Camera::open("/dev/videoN")` → `capture_frame()` liefert `Vec<u32>` als RGB `0x00RRGGBB`
  (Breite/Höhe in `cam.width`/`cam.height`), `close()` / `Drop` räumt auf.
  Unterstützt **Single-Plane und MPLANE** (Exynos nutzt meist MPLANE), Formate **YUYV/NV21/NV12/RGB24**.
  `camera::find_capture_node()` sucht die erste Node mit `V4L2_CAP_VIDEO_CAPTURE(_MPLANE)`.
  Struct-Größen (Capability 104, Format 208, RequestBuffers 20, Plane 64, Buffer 88) per `const assert` abgesichert.
- **tools/camctl.c** – eigenständiges statisches Testwerkzeug (gebaut, Binary: `/mnt/user-data/outputs/camctl`).
  - `camctl list` – alle `/dev/video*`: QUERYCAP-Treiber/Karte, caps (CAPTURE / CAPTURE_MPLANE / STREAMING) und
    unterstützte Formate (ENUM_FMT).
  - `camctl grab <dev|/pfad> <out.ppm>` – einen Frame holen (verwirft 5 Frames für die Belichtung),
    nach RGB wandeln, als **PPM (P6)** speichern. `<dev>` ist die Nummer (z. B. `0`) oder ein voller Pfad.

## Status
- camera.rs kompiliert statisch im Workspace (ultar-shell), qemu-geprüft über den Gesamt-Build.
- camctl läuft unter qemu-aarch64 ohne Absturz (ohne Gerät: `list` leer, `grab` sauberer Open-Fehler).
- **Noch nicht am Gerät getestet** – Exynos-Pipeline braucht evtl. media-ctl und/oder ISP-Firmware (siehe unten).

## Exynos-Besonderheit (wichtig)
Die A30 hat eine FIMC-IS-Pipeline: **Sensor → MIPI-CSIS → ISP (FIMC-IS) → MCSC (Scaler) → Capture-Node**.
Nur die **MCSC-Ausgabe** ist eine echte Capture-`/dev/video`-Node; viele andere `/dev/video*` sind Subdev-/Interna.
Zwei mögliche Hürden am Gerät:
1. **Verschaltung (media-ctl):** Wenn `grab` mit STREAMON-Fehler oder „kein Frame" scheitert, muss die Pipeline
   erst verschaltet werden (Links Sensor→CSIS→ISP→MCSC aktivieren, Formate auf den Subdevs setzen).
2. **ISP-Firmware:** FIMC-IS lädt eine Firmware + „setfile" aus `/vendor/firmware` (z. B. `fimc_is_lib.bin`,
   `setfile_*.bin`, `fimc_is_rta_*.bin`). Ohne sie streamt der Sensor nicht – analog zum WLAN-Fall (Firmware lag
   auf /vendor). `/vendor` muss also gemountet sein (mmcblk0p26) und ggf. der Firmware-Pfad gesetzt sein.

## Testanleitung (über die Tür, Schritt für Schritt)
1. **camctl aufs Gerät** (vom PC-Ordner UltarFlash):
   `upush.ps1 <...>\camctl /data/camctl` (vorher `camctl` aus /mnt/user-data/outputs dorthin committen).
2. **/vendor mounten** (für mögliche ISP-Firmware):
   `ucmd.ps1 "mkdir -p /vendor; mount -t ext4 -o ro /dev/mmcblk0p26 /vendor 2>&1; ls /vendor/firmware | grep -iE 'fimc|is_|setfile|rta'"`
3. **Nodes + Formate auflisten:**
   `ucmd.ps1 "/data/camctl list"`
   → Nodes mit `CAPTURE`/`CAPTURE_MPLANE` + `STREAMING` sind die Kandidaten. Namen merken (card-Feld nennt oft
   „MCSC"/„scaler"/„vout"). Front/Back/Ultraweit sind verschiedene Sensoren am selben ISP.
4. **Frame holen** (jede Kandidaten-Node durchprobieren):
   `ucmd.ps1 "/data/camctl grab 0 /data/cam.ppm; ls -la /data/cam.ppm"`
   (dev-Nummer je Kandidat tauschen). Erwartung: „gespeichert: … PPM". Fehlerbilder:
   - `STREAMON: ...`/`kein Frame` → Pipeline/Firmware fehlt (siehe oben) → mit media-ctl verschalten.
   - `REQBUFS`/`S_FMT`-Fehler → Node ist kein echter Capture-Ausgang, nächste probieren.
5. **PPM abholen + ansehen:** `upull.ps1 /data/cam.ppm <...>\cam.ppm`, dann in WSL
   `pnmtopng cam.ppm > cam.png` (oder mit PIL `Image.open('cam.ppm').save('cam.png')`), in die Sandbox stagen,
   unter 2000px skalieren und ansehen. Byte-genau prüfen via md5 wie beim Screenshot.
6. **Falls media-ctl nötig:** prüfen ob `media-ctl` am Gerät liegt (`which media-ctl`); sonst statisch bauen
   (v4l-utils) wie bei wpa_supplicant. Typischer Ablauf: `media-ctl -d /dev/media0 -p` (Topologie zeigen),
   Links aktivieren, Formate auf Sensor-/CSIS-/ISP-Pads setzen, dann erneut `camctl grab`.

## Integration in die Oberfläche (für später)
Eine Kamera-App/Vorschau: `let mut cam = camera::Camera::open(&camera::find_capture_node()?)?;`
pro Frame `cam.capture_frame()?` → die `Vec<u32>` (Breite×Höhe) in `cv.px` skaliert blitten.
Front/Back-Umschaltung = andere Node bzw. Sensor-Auswahl über media-ctl/Subdev.

## Fix: Kamera-App fror das ganze System ein (2026-11-19)

Symptom (Nutzer): „Kamera-App starten → System hängt, kein Ausweg außer Neustart."

Ursache: `app_camera.rs` rief `find_capture_node()` + `Camera::open()` + Capture **im UI-Thread**.
Auf Exynos/fimc-is blockiert der Kernel beim `open()`/`S_FMT`/`STREAMON` im `request_firmware`
(ISP-Firmware `fimc_is_lib.bin`/`fimc_is_rta.bin`/`setfile_*`), wenn `/vendor` nicht gemountet ist –
bis ~60 s oder dauerhaft. Dadurch fror die komplette Oberfläche ein. (Gleiche Klasse wie der
Setup-Hänger über die MFC-Videonode.)

Fix: Öffnen und Aufnahme laufen jetzt in `camera::CameraWorker` in einem **eigenen Thread**:
- UI-Thread pollt nur das zuletzt empfangene Bild (nicht blockierend) und zeichnet weiter.
- Status `Opening/Live/Failed`; nach ~8 s ohne Bild zeigt die App „Kamera nicht verfügbar", bleibt
  aber voll bedienbar.
- Beim Stop wird der Worker **nicht gejoint** (könnte im Kernel hängen), nur per Flag abgelöst; der
  fd schließt, sobald der Kernelaufruf zurückkehrt.
- Funktionierender Knoten wird in `/data/ultar/camera.node` gemerkt.

Damit friert die Oberfläche selbst bei hart hängendem Kamera-Node nicht mehr ein. Dateien:
`crates/ultar-shell/src/camera.rs` (Worker), `crates/ultar-shell/src/app_camera.rs` (nur noch Polling).

Damit die Kamera auch wirklich **läuft**, muss die ISP-Firmware beim Öffnen da sein → `/vendor`
gemountet (ultar-init `mount_vendor_firmware()`, ab nächstem Flash) bzw. live gemountet. Firmware
liegt in `/vendor/firmware/` (fimc_is_lib.bin, fimc_is_rta.bin, setfile_2p6/3p8sp/5e9.bin).

## Eigener Kameraweg ohne Android-HAL – Befunde am Gerät (2026-11-19)

Werkzeuge: `tools/a30-cam/`. Geprüft am Stock-Kern `4.4.177-22602103` (Quelltext = universal7904,
Zeilennummern der Fehlermeldungen stimmen überein; Treiberdatei ist `fimc-is-device-sensor_v2.c`,
Sensorgruppe aktiv).

| Punkt | Befund |
|---|---|
| S_FMT/G_FMT → ENOTTY | Kern hat `VIDEO_MAX_PLANES 17` → `struct v4l2_format` 384 statt 208 Bytes, andere ioctl-Nummer. Mit angepasster Kopfdatei geht S_FMT. |
| Sensorgröße | Kommt **nicht** aus S_FMT, sondern aus `S_CTRL V4L2_CID_IS_S_SENSOR_SIZE` (Basis+69, Wert `breite<<16 \| höhe`), vor S_FMT. |
| Knotennummern | `/dev/videoN` = interne Nummer + 100. In den Metadaten und im S_INPUT (vindex) zählt die **interne** Nummer: ss0 = 1, ss0vc0 = 110, 30s = 10, i0s = 30, m0p = 70. |
| Leitknoten ss0 | Typ OUTPUT_MPLANE (10); S_PARM aber mit CAPTURE_MPLANE (9). |
| Metadaten | `camera2_shot_ext` ist am Gerät **38368** Bytes (Quelltext ergibt 38064); `magicNumber` liegt bei Versatz **38360**. Die 304 Bytes mehr liegen vor der Magic-Zahl; `node_group` (Versatz 56, Aufnahme[0].vid bei 100) ist unverändert. |
| Sensor-Modus | Mit obigem: `sensor mode(4608x3456@30) = 0 (lane:4)`, CSI wird eingerichtet, Gruppe startet. |
| Strom | Stromfolge (AVDD/DVDD/VDDIO, Reset, MCLK) gibt es nur für Szenario NORMAL (0). READ_ROM (4) schaltet nur VDDIO → Sensor antwortet nicht (I2C −6). |
| „Sensor allein" | Jedes Szenario außer NORMAL/STANDBY ist „stand-alone" (läuft ohne Bildprozessor), hat aber keinen Strom. NORMAL verlangt den gebundenen Bildprozessor (`BUG_ON(!group->device->sensor)` in `fimc_is_group_start`). |
| Absturz | Stream-Start mit stromlosem Sensor → `kernel BUG at fimc-is-device-csi_v3.c:313` (Gerät startet nach 5 s neu). Deshalb nie S_STREAM ohne Strom. Absturzprotokoll: `/proc/last_kmsg`. |

Folgerung: Ein Bild gibt es am Stock-Kern nur über den **vollen Weg**: Sensor (NORMAL) + 3AA-Gruppe
(lädt Samsungs Bibliothek `fimc_is_lib.bin`/`fimc_is_rta.bin` + `setfile_2p6.bin` in den Kern). Rohbild dann
am Knoten ss0vc0 oder 30c; Farbbild über ISP/MCSC (m0p).

### Voller Weg (Sensor + 3AA + ISP) – Stand 2026-11-19 abends

Mit `CAM3AA=1 camraw 0 4608 3456 aus.raw 0 10 0 2 1` läuft am Stock-Kern jetzt:
Bibliothek geladen (`DDK version: 5E7885 …F16LLNJ02PA`, RTA, `setfile_2p6.bin`), Sensor an Strom und im
Modus 4608x3456@30, CSI-Empfang (`[VC0][F1] DMA on`), 3AA und ISP bekommen Bildanfang-Unterbrechungen.
Danach stürzt Samsungs Bibliothek im Kern ab (`lib_task_work`, Zugriff auf 0x22a4) → Neustart.
**Ein Rohbild kommt an** (`Bild 0: Puffer 0 fcount 1 … Mittel 35`, DQBUF am Knoten ss0vc0 klappt), der Absturz
kommt aber, bevor es auf den Speicher geschrieben ist. Die Daten sind größer als 10 Bit gepackt
(5760 Bytes/Zeile): auch hinter 19,9 MB stehen Werte → Zeilenmaß noch bestimmen.

Was dafür nötig war (in dieser Reihenfolge):

1. Einmal je Start: `cat /sys/class/camera/rear/fw_update` → „HW init done." Ohne das ist ein Mutex
   nicht angelegt und das Öffnen des Bildprozessors stürzt ab (`fimc_is_load_bin`).
2. Der Kern liest die Bibliothek fest von `/system/vendor/firmware/` → `/system/vendor` muss auf `/vendor` zeigen.
3. Knoten öffnen: ss0 (101), ss0vc0 (210), 30s (110), 30p (112), i0s (130).
4. `S_CTRL V4L2_CID_IS_END_OF_STREAM` (Basis+58) auf 30s = Bildprozessor wirklich öffnen.
5. S_INPUT: Sensor mit Szenario NORMAL und **vindex = 1** (bei 0 wird die Sensorgruppe nicht verkettet →
   `BUG` in `csi_dma_tag`); 30s mit Quelle 1 (OTF); i0s mit Quelle 12 (30p, OTF). Die 3AA-Gruppe darf nicht
   die letzte sein (sonst `junction == NULL` → Absturz in `fimc_is_ischain_3aa_shot`).
6. Sensorgröße per S_CTRL, dann S_FMT/S_PARM, S_FMT auf 30s/30p/i0s, REQBUFS, STREAMON (i0s, 30s, ss0vc0, ss0).
7. Aufnahmepuffer **größer** als das gepackte Maß anlegen (hier Breite×Höhe×2): sonst schreibt die CSI-DMA
   über das Pufferende → `SysMMU WRITE PAGE FAULT` → Neustart.
8. Metadaten: Magic-Zahl bei Versatz 38360; `capture[0]` = 110 (Anfrage 1), `capture[1]` = 12 (Anfrage 0, Größe gesetzt).

Offen: Absturz in der Bibliothek. Wahrscheinlichste Ursachen: (a) die 304 Bytes Unterschied im
Metadatenaufbau liegen an unbekannter Stelle – die Bibliothek liest dann falsche Felder; (b) Kette ohne
MCSC/VRA-Knoten, wie sie die Android-HAL immer öffnet. Nächster Schritt: passende Kopfdatei zur
Firmware `F16LLNJ02PA` finden bzw. Kette um MCSC (m0s/m0p) ergänzen; Bild zuerst am Knoten ss0vc0.


## Stand 2026-11-20: Kamera und Blitzlicht laufen

**Kette, die am Stock-Kern ohne Absturz läuft** (Helfer `ultar-cam`, Quelle `tools/a30-cam/`):

    Sensor ss0 (101) -> 3AA 30s (110) => ISP i0s (130) -> MCSC m0s (160) -> VRA (180)
    Rohbild am Knoten ss0vc0 (210); Verbindungsknoten 30p (112) nur geöffnet

Die ISP-, MCSC- und VRA-Gruppen werden gestartet, bekommen aber keine Bilder. Sie müssen trotzdem dabei sein:

| Fehlt … | Folge |
|---|---|
| Nachfolger der 3AA-Gruppe | `junction == NULL` → Absturz in `fimc_is_ischain_3aa_shot` |
| VRA | Bibliothek ruft nach dem 1. Bild eine VRA-Funktion ohne Kontext auf → Absturz (`lib_task_work`, Adresse 0x22a4) |
| MCSC | Zuordnung der Einstellungsblöcke verrutscht → `VRA_CopyTuneSet size … should be equal` → `BUG` |
| Sensorformat vor dem letzten S_INPUT | Frontkamera: Nullzeiger in `get_initial_exposure_gain_of_sensor` |

Weitere Punkte:

- `camera2_shot_ext` hat am Stock-Kern 38368 Bytes, weil dort `CAPTURE_NODE_MAX 12` gilt (Quelltext: 5).
  Mit 5 gebaut liegt der ganze `shot`-Teil 304 Bytes falsch.
- Die CSI-DMA liefert **12 Bit gepackt** (2 Pixel in 3 Bytes, 6912 Bytes je Zeile bei 4608 Breite), Werte 10 Bit, Schwarzwert 64.
- Belichtung und Weißabgleich macht Samsungs Bibliothek; die Verstärkungen stehen in `dm.color.gains` (R, G, G, B).
- Frontkamera: Position 1, Knoten ss1 (102) und ss1vc0 (214), gleiche Größe 4608x3456.
- Blitzlicht/Taschenlampe: `echo 1|0 > /sys/class/camera/flash/rear_flash`.
  **Nie** `/sys/class/leds/fled-s2mu106/brightness` beschreiben – der Kern springt dort auf einen Nullzeiger.

**Helfer `ultar-cam`**

    ultar-cam stream [front]     Bildstrom auf stdout: "UCF1", Breite, Höhe, Nr (u32 LE), dann Pixel 0x00RRGGBB
                                 Eingabe 'p' = nächstes Bild in Fotogröße 2304x1728, 'q' oder Ende der Eingabe = beenden
    UCAM_STEP=n                  Vorschau: jedes n-te Bayerfeld (3 → 768x576)
    UCAM_BAYER=0..3              Farbanordnung 0 GRBG (Vorgabe), 1 RGGB, 2 BGGR, 3 GBRG

Die Oberfläche (`camera.rs`, `samsung_main`) startet den Helfer, sobald `/sys/class/camera/rear/fw_update` und
`/usr/bin/ultar-cam` vorhanden sind. Vorgabe-Drehung: hinten 90°, vorn 270°.

**Geprüft am Gerät:** Vorschau in der Kamera-App (Bildschirmfoto), 30 Bilder/s im Helfer, Fotogröße im Strom,
Frontkamera läuft ohne Absturz, Lampe an/aus (Bildhelligkeit steigt von ~5 auf ~58).
**Noch nicht geprüft:** Farben bei Tageslicht (Farbanordnung), Auslöser in der App, Videoaufnahme.

## Kamera-App: Dokumentenscanner und Autofokus (2026-11-20)

- **Modus „Scan"** (vierter Modus neben Foto/Quadrat/Video), Code: `crates/ultar-shell/src/scan.rs` + `app_camera.rs`.
  - Vorschau: das Blatt wird laufend gesucht (helle zusammenhängende Fläche vor dunklerem Grund, Otsu-Schwelle,
    Ecken = Extrempunkte der Diagonalen) und golden eingerahmt.
  - Auslöser: Bild in Fotogröße, Blatt neu suchen, **Kontrollansicht** – die vier Ecken lassen sich ziehen,
    Filter Farbe / Grau / S/W wählbar, verwerfen (✕) oder übernehmen (✓).
  - Übernehmen: perspektivisch entzerren (längere Kante bis 2200 Pixel), Beleuchtung ausgleichen
    (Papierweiß je Kachel, Schatten verschwinden), als `DCIM/Camera/SCAN_<zeit>.jpg` speichern.
  - Mehrere Seiten sammeln sich; Knopf „PDF · n Seiten" schreibt ein mehrseitiges A4-PDF nach `Documents/SCAN_<zeit>.pdf`.
- **Autofokus:** Hauptkamera stellt dauernd scharf (`AA_AFMODE_CONTINUOUS_PICTURE`); abschaltbar mit `UCAM_NOAF=1`.
- **Fernbedienung zum Prüfen (Tür):** `/run/ultar/shell.goto` = `scan`, `scanshot` (auslösen), `scanok` (übernehmen),
  `photoshot`; danach `kill -USR1 $(pidof ultar-shell)`.
- Tests: `cargo test -p ultar-shell scan::` (Blatt finden, entzerren, aufbereiten, PDF-Aufbau).

## Fix 2026-11-20 (Mittag): „ultar-cam: Too many open files (os error 24)"

Am Gerät lief noch eine ältere Oberfläche (Hilfe-Stand). Deren DECON-Ausgabe schloss die Sync-Fences des
Kernels nicht: nach einigen Stunden waren 995 von 1024 Dateinummern belegt (`anon_inode:sync_fence`), dann
konnte die Kamera-App den Helfer `ultar-cam` nicht mehr starten. Behoben in `decon.rs` (Fences werden nach
jedem Bild geschlossen, seit k13); neue Oberfläche eingespielt → Kamera-App zeigt wieder das Live-Bild,
die Oberfläche hält ~31 offene Dateien. Prüfen: `ls /proc/$(pidof ultar-shell)/fd | wc -l`.
