# A30 – SIM / Mobilfunk (Shannon-CP-Boot)

Quelle: A30-Kernelbaum `/home/claude/a30k`. **Aktiver Treiber: `drivers/misc/modem_v1`**
(CONFIG_UMTS_MODEM_SS310AP=y, CONFIG_LINK_DEVICE_SHMEM=y, **CONFIG_CP_SECURE_BOOT=y**). Modem =
Samsung Shannon **SS310AP** (im Exynos 7904). Steuergerät `/dev/umts_boot0`, IPC `/dev/umts_ipc0`,
Daten über `rmnet0..7`.

> **Korrektur gegenüber erster Fassung:** Das Gerät nutzt **modem_v1**, nicht modem_if. Deshalb andere
> ioctls (XMIT_BOOT 0x40 + SECURITY_REQ 0x53 statt XMIT_BIN 0x63 + SEC_CP_INIT 0x61) und eine andere
> Reihenfolge. Live bestätigt: `ss310ap_on` + `ss310ap_boot_on` in dmesg.

> **Sicherer Boot ist an.** `SECURITY_REQ` ruft `exynos_smc(SMC_ID, mode, size_boot, size_main)` und
> prüft die **Signatur**. Nur die **originale, signierte** Samsung-CP-Firmware bootet. Modifizierte
> Modem-Firmware ist unmöglich – für SIM/Mobilfunk egal (wir booten die Stock-Firmware).

## 1. Warum der erste Versuch scheiterte (EACCES)
`ss310ap_boot_on` (modem_ctrl_ss310ap.c) wartet in einer Schleife 100×~15 ms auf `cp_status != 0` und
gibt sonst **-EACCES** zurück. Den `cp_status` setzt erst der **CP**, nachdem er seinen Bootloader
ausführt. Unser alter Ablauf rief BOOT_ON direkt nach MODEM_ON – der CP hatte nichts zu starten, also
blieb cp_status 0 → EACCES. **Fix:** Boot-Image laden + SECURITY_REQ + DL_START VOR BOOT_ON.

## 2. ioctls auf /dev/umts_boot0 (modem_v1)
Alle `_IO('o', nr)` → Typ `'o'`=0x6f, Größe 0 → Nummer = `(0x6f<<8)|nr`:

| Name | nr | Wert | Zweck |
|---|---|---|---|
| IOCTL_MODEM_ON | 0x19 | 0x6f19 | CP einschalten (pmu power on + 300 ms) |
| IOCTL_MODEM_OFF | 0x20 | 0x6f20 | CP aus |
| IOCTL_MODEM_RESET | 0x21 | 0x6f21 | CP reset |
| IOCTL_MODEM_BOOT_ON | 0x22 | 0x6f22 | wartet auf cp_status, setzt ap_status |
| IOCTL_MODEM_BOOT_OFF | 0x23 | 0x6f23 | Boot-Modus aus |
| IOCTL_MODEM_BOOT_DONE | 0x24 | 0x6f24 | Download fertig |
| IOCTL_MODEM_STATUS | 0x27 | 0x6f27 | phone_state lesen (Rückgabe: 0=OFFLINE,3=BOOTING,4=ONLINE) |
| IOCTL_MODEM_DL_START | 0x28 | 0x6f28 | setzt MEM_BOOT_MAGIC + ap_status=1 → CP startet PBL |
| IOCTL_MODEM_XMIT_BOOT | 0x40 | 0x6f40 | CP-Teil-Image laden (`struct modem_firmware`) |
| IOCTL_SECURITY_REQ | 0x53 | 0x6f53 | exynos_smc Signaturprüfung (`struct modem_sec_req`) |

## 3. Strukturen (1:1 aus modem_v1/modem_prj.h)
```c
struct modem_firmware {   // __packed, Argument von IOCTL_MODEM_XMIT_BOOT
    u64 binary;           // Userspace-Zeiger auf die Nutzdaten
    u32 size;             // Größe der Stufe
    u32 m_offset;         // Ziel-Offset im CP-Speicher (aus TOC)
    u32 b_offset;         // Quell-Offset (0, wenn der Puffer direkt übergeben wird)
    u32 mode;             // 0 = BOOT_MODE (boot_base/boot_size), 1 = DUMP
    u32 len;              // Länge dieses Stücks
};
struct modem_sec_req {    // __packed, Argument von IOCTL_SECURITY_REQ
    u32 mode;             // cp_boot_mode: 0 = NORMAL
    u32 size_boot;
    u32 size_main;
    u32 dummy;
};
enum phone_state { OFFLINE=0, CRASH_RESET=1, CRASH_EXIT=2, BOOTING=3, ONLINE=4 };
```
Kernel (`shmem_xmit_boot`): kopiert `len` Bytes von `binary` nach `boot_base + m_offset` (mode 0),
prüft Grenzen gegen `boot_size`. `DL_START` (`shmem_start_download`) setzt MEM_BOOT_MAGIC und ruft
`modem_boot_confirm` (ap_status=1) → CP startet. `SECURITY_REQ` (`shmem_security_request`) macht den SMC.

## 4. Verifizierte Boot-Reihenfolge
1. `MODEM_ON` → 300 ms warten
2. `XMIT_BOOT` je BOOT-Stufe (mode=0, m_offset/size aus TOC)
3. `SECURITY_REQ {mode=0, size_boot, size_main}` → Signatur prüfen/aktivieren (Rückgabe 0 = ok)
4. `DL_START` → Magic setzen, CP startet PBL
5. `BOOT_ON` → wartet jetzt erfolgreich auf cp_status
6. `XMIT_BOOT` je MAIN/VSS/NV-Stufe
7. `BOOT_DONE`, `BOOT_OFF`
8. `STATUS` pollen bis 4 (ONLINE)

## 5. CP-Firmware-Aufbau (TOC)
Firmware beginnt mit **TOC**: 32-Byte-Einträge `{char name[12]; u32 b_offset; u32 m_offset; u32 size;
u32 crc; u32 toc_count;}`. Erster Eintrag `"TOC"` (toc_count=Anzahl), danach BOOT/MAIN/VSS/NV. `b_offset`
= Lage in der Datei, `m_offset` = Ziel im CP-RAM, `size`. VERIFY am echten Image (Feldbreiten/Namen).

## 6. SIPC5-IPC (nach dem Boot, über /dev/umts_ipc0)
Rahmen: `config(1) | ch_id(1) | len(2 LE) | [ctrl] | payload`. Kanäle: FMT=235 (RIL/SIM), RFS=245,
PDP 10..24 (Daten→rmnet). sipc_fmt_hdr: `u16 len; u8 msg_seq; u8 ack_seq; u8 main_cmd; ...`. Darauf
setzt der (noch zu bauende) RIL auf.

## 7. Skelett-Programm
`crates/ultar-modem` (statisch, aarch64, modem_v1-korrekt):
- `ultar-modem --fw <cp.bin>` — TOC-Trockentest.
- `ultar-modem --fw <cp.bin> --arm` — scharfer Boot in der Reihenfolge aus §4.
- `ultar-modem --reset` — nach Fehlversuch: MODEM_OFF + MODEM_RESET (CP aus STATE_BOOTING holen).
Binary: `/mnt/user-data/outputs/ultar-modem`.

## 8. Testanleitung (über die Tür)
**Schritt 1 – Modem-Partition finden** (CP-Firmware ~40–50 MB; Kandidaten p16≈53 MB, p19≈45 MB, p15≈36 MB):
```
for p in 15 16 19; do echo "== p$p =="; dd if=/dev/block/mmcblk0p$p bs=512 count=1 2>/dev/null | od -c | head -4; done
```
Die Partition mit lesbarem `TOC`/`BOOT`/`MAIN` am Anfang ist die CP-Firmware.

**Schritt 2 – herausziehen + TOC prüfen** (z.B. p16):
```
dd if=/dev/block/mmcblk0p16 of=/data/cp.bin bs=1M 2>/dev/null; wc -c /data/cp.bin
/data/ultar-modem --fw /data/cp.bin
```
Erwartung: TOC mit BOOT, MAIN, VSS, NV und plausiblen m_offsets. Müll → falsche Partition (anderer Kandidat).

**Schritt 3 – scharfer Boot** (nur bei sauberer TOC):
```
/data/ultar-modem --fw /data/cp.bin --arm 2>&1 | tee /data/modem.log
dmesg | tail -40
```
Erwartung in dmesg: `ss310ap_boot_on: +++` … **`---`** (kein EACCES mehr), `cp_status=…`, dann Richtung
ONLINE. Danach `ifconfig rmnet0` und `/dev/umts_ipc0` prüfen.

**Reset vor erneutem Versuch:** Nach einem Fehlversuch steht der CP in STATE_BOOTING. Vor dem nächsten
`--arm` einmal `/data/ultar-modem --reset` (MODEM_OFF 0x6f20 + MODEM_RESET 0x6f21).

**Risiken:** Ein falscher `m_offset` kann den Kernel zum `BUG()` bringen – m_offset nur aus der TOC.
`SECURITY_REQ != 0` = Signatur abgelehnt (CP-Version passt nicht zum geflashten Stock → passende CP-FW
nutzen). Fehlgeschlagener CP-Boot ist ungefährlich fürs AP (Ultar läuft weiter).

## 9. Offen
- VERIFY am Gerät: Partition, TOC-Feldbreiten, exakte m_offsets, ob MAIN vor oder nach BOOT_ON geladen wird.
- Nach erfolgreichem Boot: schlanker **RIL** auf SIPC5 (SIM-Status, PIN, Netzregistrierung, PDP) – großes eigenes Paket.

## XMIT_BOOT-Fix für MAIN/VSS/NV (aus Kernel-Analyse, 2026-11-19)

Quelle: `drivers/misc/modem_v1/link_device_shmem.c::shmem_xmit_boot` +
`drivers/misc/mcu_ipc/shm_ipc.c`.

`shmem_xmit_boot` prüft und kopiert so:
```
valid_space = mf.mode ? mld->size   : mld->boot_size;   // ipc_size : ipc_off
v_base      = mf.mode ? mld->base   : mld->boot_base;    // (p_addr+ipc_off) : p_addr
if (mf.size > valid_space || mf.len > valid_space || mf.m_offset > valid_space - mf.len)
        return -EINVAL;                                  // = „Invalid args"
dst = v_base + mf.m_offset;                              // m_offset ist REGION-RELATIV
```
Speicherlayout (phys, `p_addr` = Reserved-Memory-Basis des CP):
- `p_addr` … Boot-Region, Größe `ipc_off`  (mode=0)
- `p_addr + ipc_off` … IPC/MAIN-Region, Größe `ipc_size` (mode=1)
- `p_addr + cp_size` … VSS-Region (eigene Region, **nicht** über mode=1 erreichbar)

### Fehler im jetzigen `ultar-modem`
`xmit_boot()` sendet IMMER `mode=0` und die **rohen TOC-`m_offset`** (absolute CP-Adressen,
z. B. VSS 0x46900000). Damit landet alles im kleinen Boot-Fenster und die großen Offsets sprengen
`valid_space` → `-EINVAL`. BOOT passt zufällig (kleiner Offset), VSS/NV nicht.

### Korrekte Sequenz
1. **BOOT** → `mode=0`, `m_offset = toc.m_offset - p_addr` (relativ zur Boot-Region).
2. **MAIN** → `mode=1`, `m_offset = toc.m_offset - (p_addr + ipc_off)` (relativ zur IPC-Region).
3. **VSS** → gehört in die VSS-Region (`p_addr + cp_size`), die `xmit_boot` NICHT abbildet. Der
   reale `cbd` lädt VSS i. d. R. nicht per XMIT_BOOT; der CP zieht es selbst. → VSS aus der
   XMIT-Schleife nehmen.
4. **NV** → wird vom CP aus der NV-Partition (EFS) geladen, nicht per XMIT_BOOT. → ebenfalls entfernen.

Also: nur **BOOT (mode0)** und **MAIN (mode1)** mit region-relativen Offsets senden; VSS/NV weglassen.

### Benötigte Laufzeitwerte (am Gerät auslesen, NICHT raten)
- `p_addr`, `ipc_off`, `ipc_size`, `cp_size` aus dem Reserved-Memory/DT bzw. den Treiber-Logs
  (`dmesg | grep -i cp_base` / `shm_get_phys_base`), oder `cpmem_info->start` aus `/proc/iomem`.
- Die echten TOC-`m_offset`/`size` (ultar-modem gibt die TOC aus).
Ohne diese Werte bleibt der Fix ungetestet; ein geratener Offset kann den CP-Pfad aufhängen.
Nächste Live-Session: TOC + Region-Basen auslesen, Offsets berechnen, dann die beiden XMIT-Calls
anpassen und auf `STATUS=ONLINE` prüfen.

## GELÖST am Gerät (2026-11-19 nachts): Modem ONLINE, Funkdienst spricht mit dem Modem

### Bootablauf (so macht es der Original-Lader; am A30 bestätigt)
`ultar-modem --auto` (Dienst `modem`, einmal beim Systemstart):

1. `IOCTL_MODEM_STATUS` – läuft das Modem schon, nichts tun.
2. Firmware direkt aus der Partition **radio** (mmcblk0p19) lesen; TOC: BOOT, MAIN, VSS, NV.
3. `IOCTL_MODEM_RESET`, `IOCTL_SECURITY_REQ {mode 2}` (Rückgabe 11 „CP_NOT_WORKING" ist hier normal).
4. Alle Stufen mit `IOCTL_MODEM_XMIT_BOOT` senden: **Stücke von 0xf800 Byte**, `mode = 0`,
   `m_offset = TOC-m_offset − 0x4000_0000` (die TOC nennt CP-Adressen; der Kernel will den Abstand zum
   Anfang des CP-Speichers – roher Wert → „Invalid args"). BOOT → 0x0, MAIN → 0x10000, VSS → 0x6900000,
   NV → 0x6300000. **NV kommt aus nv_data.bin**, nicht aus der Firmware.
5. `IOCTL_SECURITY_REQ {mode 0, Größe BOOT, Größe MAIN}` → 0 (Signatur ok; nur Original-Firmware besteht).
6. `IOCTL_MODEM_ON` → `IOCTL_MODEM_BOOT_ON` → `IOCTL_MODEM_DL_START`.
7. Handshake auf /dev/umts_boot0 (je 4 Byte schreiben/lesen): `0x900d → 0xa00d`, `0x9f00 → 0xaf00`.
8. `IOCTL_MODEM_BOOT_OFF` → Kernel meldet `BOOTING -> ONLINE` (≈ 2 s).

**EFS-Schutz:** nv_data.bin wird einmalig aus der EFS-Partition kopiert (nur lesend, `noload`) nach
`/data/ultar/modem/nv_data.bin`. Alle Schreibwünsche des Modems landen in dieser Kopie – die EFS-Partition
(IMEI, Kalibrierung) wird nie beschrieben.

**Schutz vor Neustart-Schleifen:** `--auto` legt `/data/ultar/modem/boot.try` an und löscht es nach dem
Versuch. Bleibt die Marke liegen (Absturz beim Modemstart), lässt der nächste Systemstart das Modem aus.

### Funkdienst (`ultar-ril --daemon`, Dienst `ril`)
- Öffnet **/dev/umts_ipc0 und /dev/umts_rfs0** – erst dann meldet der Kernel dem Modem „Funkdienst bereit".
- Ereignisgesteuert (`crates/ultar-ril/src/engine.rs`): das Modem meldet von selbst; Kopf 7 Byte
  `länge:u16 mseq aseq gruppe index typ`, Antworttypen INDI 1 / RESP 2 / NOTI 3.
- **RFS-Server** (`rfs.rs`, Kanal 245, Kopf `länge:u32 befehl id`): NV lesen 0x01 / schreiben 0x02 (bestätigt),
  Datei öffnen 0x11 (gesehen: `CP_AUDIO_SLSI_OMC_AUDIODATA.bin`), weitere Dateibefehle nach Annahme (// VERIFY).
- Mitschrift aller Meldungen: `/run/ultar/radio/ipc.log`. Zustand für die Oberfläche: `/run/ultar/radio/state`.
- Entwicklerbefehl: `echo "raw <befehl hex> <typ+daten hex>" >> /run/ultar/radio/cmd`.

### Am Gerät gemessen
| Meldung | Inhalt |
|---|---|
| `0107` PHONE_STATE | 2 = Normalbetrieb (Funk an); Setzen mit EXEC `02 02`, Flugmodus `01 00` |
| `0a03` ME_SN | IMEI lesbar |
| `0501` PIN_STATUS | `80 00` = **keine Karte erkannt** |
| `0805` REGIST (leitungsvermittelt) | UMTS, Status 4 = **nur Notrufe**, LAC/Zelle vorhanden |
| `0803` SERVING_NETWORK | Netz **41701** |

Das Modem funkt also und findet ein Netz. Offen ist die SIM: Status „keine Karte".

### Offen
- SIM: Karte eingelegt? Wenn ja: SIM-Erkennung prüfen (`/sys/devices/platform/10000.mif_pdata/sim/ds_detect`,
  Dual-SIM-Zähler; der Original-Lader schreibt dort die SIM-Anzahl vor MODEM_ON).
- Danach: PIN, Anmeldung, mobile Daten (PDP → rmnet0), Anrufe/SMS an die Apps anbinden.
- Dateibefehle des RFS am Gerät bestätigen; Signalstärke-Meldungen (`07xx`) deuten.

## Abgleich mit dem Original-Treiber (cbd + rild), 20.11.2026

Untersucht: `/vendor/bin/cbd` (aus dem Gerät, disassembliert), `init.baseband.rc`, `init.vendor.rilchip.rc`,
`init.vendor.rilcommon.rc` (Sicherung `a30-RR8M6060BMP/drivers/vendor_etc_init`) und der Kernel-Treiber
`modem_v1/modem_ctrl_ss310ap.c`.

### Was Android anders macht als wir (vorher)
| Schritt | Original | Ultar vorher | Ultar jetzt |
|---|---|---|---|
| CP-Lader | `cbd -d -tss310 -bm -mm -P …/by-name/radio` | `ultar-modem --auto` (gleicher Ablauf) | unverändert |
| **SIM-Steckplätze** | cbd `set_sim_configuration`: liest `/mnt/vendor/efs/factory.prop`, nimmt die Ziffer nach dem ersten `=`, schreibt sie nach `/sys/devices/platform/10000.mif_pdata/sim/ds_detect` – **vor** dem CP-Start; init schreibt zusätzlich `ro.vendor.multisim.simslotcount` nach `/sys/module/modem_ctrl_ss310ap/parameters/ds_detect` | **fehlte** – Kernel-Vorgabe `ds_detect=1` | `ultar-modem` kopiert `factory.prop` (nur lesend) aus EFS nach `/data/ultar/modem/`, schreibt die Anzahl vor dem Boot nach `ds_detect`; Hand-Vorgabe `/data/ultar/modem/simslots` hat Vorrang |
| Warum wichtig | Der Kernel legt `ds_detect−1` bei `MODEM_ON` (`init_mailbox_regs`) in das AP→CP-Statusregister (`sbi_ds_det`); danach richtet der CP seine SIM-Erkennung aus | | |
| NV | `/mnt/vendor/efs/nv_data.bin` | Kopie in `/data/ultar/modem` | unverändert |
| CP-EFS | eigene Partition `cpefs` → `/mnt/vendor/cpefs` | nicht eingebunden | offen (siehe unten) |
| **Mobile Daten** | rild: `GPRS_DEFINE_PDP_CONTEXT` (0x0D01 SET) → `GPRS_PDP_CONTEXT` (0x0D04 SET) → Modem meldet `IP_CONFIGURATION` (0x0D09) / `CALL_STATUS` (0x0D10) → rmnet0 einrichten | Code in `pdp.rs` vorhanden, aber **nirgends aufgerufen**; Schalter setzte nur `ip link rmnet0` | Funkdienst baut den Kontext auf, sobald Schalter an + SIM bereit + Paketdienst angemeldet; richtet rmnet0 (IP/Route/DNS) ein; Schalter aus = Kontext ab + rmnet0 runter |

### Bedienung
- APN: `/data/ultar/apn.conf` mit `apn=…`, optional `user=…`, `pass=…`. Fehlt sie, wird ein leerer APN gesendet
  (Netz-Vorgabe; bei LTE meist ausreichend).
- Mobile Daten an: Schnelleinstellungen (`/run/ultar/mobiledata.on`).
- Ohne SIM zeigt die Oberfläche jetzt „Keine SIM erkannt (Modem kennt N Steckplatz/-plätze)".
- Läuft das Modem bereits, greift eine neue Steckplatz-Zahl erst nach CP-Neustart:
  `ultar-modem --reset` und danach `ultar-modem --auto --force`.

### Ehrlich offen
- **Am Gerät noch nicht getestet** (Tür COM13 war während der Arbeit belegt). Erwartung im Log:
  `factory.prop aus EFS kopiert: …` und `SIM-Konfiguration: N Steckplatz/-plätze -> …/ds_detect`, in dmesg
  `set ds_detect: N` und `sys_rev:…, ds_det:…`.
- `PIN_STATUS 80 00` = „keine Karte" ist ohne eingelegte SIM das **richtige** Ergebnis – zum Prüfen muss eine SIM rein.
- `GPRS_CALL_STATUS`-Aufbau (cid, status, fail_cause) nach libsamsung-ipc, `// VERIFY` am Gerät.
- `cpefs` (CP-eigene Dateien) wird noch nicht bereitgestellt; RFS-Dateizugriffe des CP landen in
  `/data/ultar/modem/rfs/`. Falls der CP dort Dateien vermisst (ipc.log: `RFS … FEHLER`), cpefs nur lesend
  einbinden und eine Kopie anbieten.
- Zweite SIM (zweiter FMT-Kanal) wird weiterhin nicht abgefragt.

## Dual-SIM / SIM 2 – am Gerät bestätigt (20.11.2026, nachmittags)
- `factory.prop` des A30: `ro.multisim.simslotcount=2`, `persist.radio.multisim.config=dsds`. `ultar-modem` schreibt
  jetzt `ds_detect=2`; dmesg: `set ds_detect: 2`, `init_mailbox_regs: sys_rev:3, ds_det:1` → Modem ONLINE.
- **SIM 2 läuft über einen eigenen FMT-Kanal** (236 = `/dev/umts_ipc1`), wie Androids zweiter RIL (`rild -c 2`).
  Der Funkdienst führt jetzt je Steckplatz eine eigene Engine (SIM 1: ipc0 + RFS, SIM 2: ipc1); Log-Zeilen `[SIM2] …`,
  Entwicklerbefehl `raw2 <cmd> <typ+daten>`, PIN mit `pin 2 <code>`.
- Gemessen mit SIM in Steckplatz 2: `PIN_STATUS 00 00` (= bereit), `SERVING_NETWORK` → **41701 Syriatel**, Zugangsart
  `0x21` (LTE), Zelle vorhanden. Oberfläche: `sim=slot:2;state:ready`, `operator=Syriatel`, `rat=LTE`.
- SIM-Statuswerte korrigiert: `0x00`/`0x83` = bereit (vorher fälschlich „unbekannt“).

### Mobile Daten – Stand
- `GPRS_DEFINE_PDP_CONTEXT` (APN `net.syriatel.com`, magic 0x02) → Antwort `0x8000` (angenommen); mit leerem APN
  `0x8001` (abgelehnt). `GPRS_PDP_CONTEXT` aktivieren → `0x8000`, aber `CALL_STATUS 01 03` (Kontext bleibt aus,
  Grund 0). `PDP_CONTEXT` GET → `01 18 00`.
- Noch nicht gedeutet: Anmeldestatus in `NET_REGIST` (CS `21 02 01…`, PS `21 03 07…`) – Shannon nutzt hier andere
  Werte als libsamsung-ipc. Vermutung für den fehlenden Datenkontext: im Dual-SIM-Betrieb muss dem CP die
  **Daten-SIM** (Android: ALLOW_DATA / DDS) mitgeteilt werden; der Befehl dafür steht in `libsec-ril.so`
  (nächster Schritt: aus /vendor/lib64 holen und auswerten). Alternativ: SIM ohne Datenguthaben.
- APN-Datei am Gerät: `/data/ultar/apn.conf` = `apn=net.syriatel.com`.

## Anrufe/SMS: Ursache „Nummer nicht im Netz" (20.11.2026, abends – am Gerät gemessen)
- `NET_REGIST` (0x0805) gedeutet: Byte 0 = Zugangsart (0x21 LTE, 0x03 EDGE), Byte 1 = Domäne (02 CS, 03 PS),
  Byte 2 = Status: **01 nicht angemeldet, 04 nur Notrufe, 07 angemeldet**; Byte 10 = Ablehnungsgrund.
- SIM 2 (Syriatel, LTE): **PS 07 = angemeldet, CS 01 = nicht angemeldet** → das Netz kennt die Nummer für Sprache/SMS
  nicht (Anrufer hört „nicht im Netz", SMS kommt nicht an). Die Daten-Anmeldung (EPS) steht, die Sprach-Anmeldung
  (CS über CSFB/SGs) fehlt.
- Dienstbereich `NET_SERVICE_DOMAIN_CONFIG` (0x0808) = 01. Laut libsec-ril (`BuildIpcNetSetServiceDomain`):
  Wert 1 = CS+PS kombiniert (Standard), 2 = nur PS, 3 = nur CS. Mit 3 meldet sich das Modem ganz ab → auf 1 lassen.
- Test nur 2G/3G (`NET_MODE_SEL` 0x080A = 01): das Modem findet hier **nur MTN (41702) auf EDGE**, Status 04 (nur Notrufe),
  Grund 13 → **Syriatel hat an diesem Standort kein 2G/3G**. Sprache/SMS gehen dort nur über LTE: CSFB (kombinierte
  Anmeldung) oder **VoLTE/IMS**. Zurückgestellt auf 0x0B (LTE/3G/2G).
- Nach den Versuchen hing die Anmeldung → Wiederherstellung: Modem zurücksetzen, `nv_data.bin` frisch aus EFS kopieren
  (`ultar-modem --reset`, alte Kopie wegschieben, `ultar-modem --auto`). Danach wieder PS 07, Syriatel LTE.
- libsec-ril (`/vendor/lib64/libsec-ril-dsds.so`) exportiert C++-Symbole (`Ipc41PacketBuilder::BuildIpc…`,
  `IpcProtocol41…::IpcTx…`) → Befehle lassen sich gezielt nachschlagen. Nächste Kandidaten: `SetLteAttachtype`,
  `SetLteAttachProfile` (Anmelde-APN), IMS-SDM-Einstellungen (`BuildIpcModemSdmIms…`). Wenn das Netz nur VoLTE
  anbietet, braucht Ultar einen eigenen IMS-Teil (SIP + AKA über die SIM + IPsec + AMR-WB) – großes eigenes Paket.

## USSD (*111# usw.) und weitere Befunde aus libsec-ril (20.11.2026, abends)
- **USSD eingebaut** (`tel.rs`, Format aus libsec-ril `IpcTxSsSendUssd`): `SS_USSD` 0x0C08 EXEC, Nutzdaten 185 Byte =
  Zustand (1 neu, 2 Antwort in Sitzung, 3 Abbruch) + DCS 0x0F + Länge + GSM-7 gepackt. Antwort des Netzes (NOTI 0x0C08):
  Zustand, DCS (GSM-7 oder UCS-2 für Arabisch), Text → `/run/ultar/radio/ussd` + Benachrichtigung.
  Telefon-App: Code wie `*111#` wählen → USSD statt Anruf; Antwortkarte; bei Menüs (Zustand 2) Antwort im Wählfeld
  eingeben und Anruftaste; „Schließen“ beendet die Sitzung. Befehle: `ussd <slot> <hex>`, `ussd_cancel`.
- **Weitere IPC-Befehle aus libsec-ril** (C++-Symbole):
  - `0x0808` Dienstbereich SET: Wert 1 = CS+PS, 2 = nur PS, 3 = nur CS (`BuildIpcNetSetServiceDomain`).
  - `0x0816` Daten-SIM (DDS, `IpcTxSetDualStandbyPrefModem`): SET `[steckplatz 0/1, 1]` – am A30 angenommen (0x8000).
  - `0x1608` IMS-Anmeldestatus an das Modem (`IpcTxImsReqSetRegState`): SET 6 Byte – angenommen.
  - `0x0811` Zellinfo-Liste (`IpcTxNetCellInfoList`) – für die Nachbar-Türme (noch einzubauen).
- **Stand Anrufe/SMS/USSD:** alle drei brauchen die Sprach-Anmeldung (CS). Sie bleibt bei SIM 2 auch nach DDS, IMS-Status
  „nicht angemeldet“ und Funk aus/an auf `01`. Ein normales Handy zeigt beim Anruf „H“ → Syriatel nutzt hier **CSFB**
  (LTE → 3G). Die kombinierte Anmeldung (EPS + IMSI über SGs) gelingt unserem Modem noch nicht; der Datenkontext
  (`0x0D04`) scheitert ebenfalls. Vermutung: der Original-RIL schickt beim SIM-Start weitere Einstellungen
  (z. B. LTE-Anmeldeprofil `SetLteAttachProfile`, Anmeldetyp `SetLteAttachtype`).
- **Empfehlung:** Samsungs eigenen RIL (`/vendor/bin/hw/rild` + `libsec-ril-dsds.so`) in der abgeschotteten
  Android-Umgebung laufen lassen und Ultar über die RIL-Schnittstelle anbinden (wie Halium/Ubuntu Touch mit oFono).
  Dann gelten Anrufe, SMS, USSD, Daten und VoLTE sofort wie im Original; der eigene Rust-RIL bleibt als Rückfall.

## Stand 20.11.2026, 19:30 – am Gerät mit Syriatel-SIM (Steckplatz 2) getestet
- **USSD funktioniert:** `*1111#` liefert das arabische Syriatel-Menü (UCS-2), Antworten im Menü über Wählfeld + Anruftaste.
  Ansicht: Karte oben, Antwortzeile und Tastenfeld darunter; „Schließen“ beendet die Sitzung; klare Texte bei
  Fehler/Zeitüberschreitung. Neue Antwort holt das Telefon aus jeder App nach vorne.
- **SMS-Empfang funktioniert:** z. B. Syriatel-Hinweis von 1957 (Arabisch, UCS-2) kam an → `/data/ultar/sms-in.tsv`,
  Benachrichtigung, Nachrichten-App.
- **NET_REGIST endgültig gedeutet** (libsamsung-ipc-Werte stimmen): Status 01 nicht angemeldet, 02 angemeldet (Heimnetz),
  03 suche, 04 nur Notrufe, 06 Roaming; Byte 0 Zugangsart 0x21 = LTE. Gemessen: CS 02 (Sprache angemeldet), PS 03
  (Daten sucht) – die frühere „07“ trat nur kurz auf. Die Statusleiste zeigt jetzt „LTE“ statt leer.
- **Anrufe abgehend:** `CALL_OUTGOING` war im alten Format (Fehler 0x0064) → jetzt Format aus libsec-ril
  (`IpcTxCallOriginate`, 92 Byte Nutzdaten). Am Gerät noch nicht bestätigt. Sprachweg (Ton im Gespräch) fehlt noch.
- **Offen:** mobile Daten (Kontext bleibt aus), Ton im Gespräch, Nachbarzellen (`0x0811`), Daten-Anmeldung PS.
- **Nächster Test:** ohne SIM geht nichts davon – SIM wieder in Steckplatz 2, dann Anruf ab-/ankommend prüfen.
