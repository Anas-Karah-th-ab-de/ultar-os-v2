# Ultar OS v2 – alles Offene, priorisiert, mit Abarbeitungsplan

Stand 28.11.2026 · Quelle: `main` 840ae8f und Projekt-Dokumente (Kopie im Projekt: `claude/OFFEN-PLAN.md`).
Seit den älteren Listen erledigt: AVX/XSAVE im Kern, Desktop-Hülle/Sperre/Systemdialoge (S1), App-Starter (A1),
Installation A–Z in QEMU, NVMe-Flush bei fsync; Sicherheit S-4, S-5, S-7, S-8, S-9, S-10, S-11, S-12, S-13, S-15,
R2-2…R2-5, `mprotect`, `CLONE_THREAD` ohne `CLONE_VM`.

**Installation erledigt und auf `main` (28.11.2026):**
- Erststart übernimmt alle Antworten (Konto, Sprache, Tastatur, Zeitzone, Barrierefreiheit, Gerätename, Komponenten);
  der Store markiert die bei der Installation gewählten, fehlenden Komponenten.
- **VMware-Installationsmaschine** `UltarOS-v2-installer.zip` (Medium auf SATA, leere NVMe-Platte, README) +
  GitHub-Workflow „Installer image“ (Abbild für USB und VMware; in QEMU bis zum ersten Installer-Schritt grün).

**Welle A erledigt und auf `main` (28.11.2026):**
- **S1 MSI/MSI-X** (`kernel/ultar-kern/src/msi.rs`): gemeinsamer Rahmen, Vektoren 0x90–0xEF, Handler je Vektor,
  Einzel-MSI + MSI-X-Tabelle im BAR. **Auf echtem E610 (VFIO) belegt** (`netz` → „10G MSI-X-Unterbrechungen: 1").
- **S3 E610-MSI-X**: RxQ0-Unterbrechung mit Auto-Maske + EITR-Drossel (kein Sturm); Firmware/Kabel erkannt.
- **S2 IOMMU/VT-d** (`iommu.rs`): DMA-Schutz Stufe 1, RAM 1:1, in QEMU (`-device intel-iommu`) geprüft
  (virtio-blk/-net arbeiten durch die IOMMU, DHCP ok). Doku `docs/integration/iommu.md`.
- **S3b USB-Netz (RNDIS)** (`drivers/gen_xhci/rndis.rs`): `NetDev` über USB-Bulk, bewaffnete Bulk-IN-Puffer.
  TX ok; RX in QEMU unbestätigt (QEMU-`usb-net` ist Full-Speed, bulk 64 → dci 5 schließt nie ab) – am echten
  Dongle zu prüfen. Doku `docs/integration/usb-net.md`.
- **S12 SD (SDHCI)** (`drivers/gen_sdhci.rs`): Controller-Reset/Takt/Strom, Karten-Init (CMD0/8/ACMD41/2/3/9/7/16),
  CSD-Kapazität (v1/v2), PIO-Lesen, `BlockDev`. **Bring-up auf echtem O2 Micro (VFIO) belegt**; Lesen mit Karte offen.
- **S12 BT (Intel Read Version)** (`drivers/gen_xhci/bt.rs`): HCI 0xFC05, TLV-Auswertung (CNVi/CNVr, Image-Typ),
  Firmware-Dateiname `ibt-<cnvi>-<cnvr>.sfi`. **Auf echtem BE200 belegt** (operational). Doku `docs/integration/bt-intel.md`.

**Live-Vollendung vorbereitet und auf `main` (28.11.2026):**
- `docs/LIVE-VOLLENDUNG.md` – Handbuch für die nur am laufenden Ultar-Server prüfbaren Teile: BT-Firmware-Download
  (Secure-Send-Offsets/Opcodes, Intel-Reset, DDC), WLAN W1–W4, Xe X1–X3, plus S9-Direktstart, NVMe-Schreibtest,
  VT-d auf echter HW, SD-Lesen mit Karte, iGPU-beim-Start. Reihenfolge §1→§4.
- `scripts/build-pc-image.sh` – entpackt Intel-BT-Firmware (`ibt-*.sfi/.ddc`) nach `/lib/firmware/intel`
  (Kern liest sie über `fs::read_all`).

Priorität: **P0** sperrt Einsatz/Sicherheit · **P1** nötig für den Betrieb · **P2** Ausbau · **P3** Erkundung/später.

## 0. Sofort (Pflege)

| # | Prio | Was | Stand |
|---|---|---|---|
| Q1 | P0 | `ultar-check` auf main | **erledigt 28.11.2026**: `gen_sdhci.rs`, `gen_xhci/rndis.rs`, `gen_xhci/bt.rs` – Kern-Texte englisch, `SAFETY:` an jedem `unsafe`, `attach`/`intel_version` geteilt. Vor jedem Merge `ultar-check all` |
| Q2 | P2 | alte Branches löschen (`claude/installer-erststart-einstellungen`, `claude/installer-store-auswahl` u. a.) | aus der Sitzung nicht erlaubt → auf GitHub |

## 1. Treiber Server – Beelink GTi15 Ultra (`.18`)

| # | Prio | Was | Stand |
|---|---|---|---|
| S1 | P0 | MSI/MSI-X allgemein | **erledigt** (Rahmen; E610 nutzt es, VFIO-belegt); NVMe/xHCI noch auf Abfragen |
| S2 | P0 | IOMMU (VT-d) DMA-Schutz | **erledigt Stufe 1** (QEMU); eigene Domäne je Gerät + echte 2 Einheiten offen (Live §4) |
| S3 | P0 | E610 10 GbE | **MSI-X erledigt**; offen: Verbindung mit Gegenstelle + Durchsatz (braucht Kabel/Gegenstelle) |
| S3b | P0 | **USB-Netz (RNDIS)** für SSH | **Treiber auf `main`**; RX am echten Dongle bestätigen; CDC-ECM/NCM + Vendor bei anderer HW offen |
| S4 | P0 | NVMe am echten Gerät | Norm-Treiber ok (QEMU); Schreib-/Lesetest nur beim Direktstart (Platten tragen Linux) → Live §4 |
| S5 | P1 | ACPI-AML-Interpreter | ⬜ (nötig für Akku/Thermik/Energie) |
| S6 | P1 | Energie (HWP, C-Zustände, RAPL) | ⬜ |
| S7 | P1 | Hardware-Wachhund + TPM 2.0 | ⬜ |
| S8 | P1 | SMBus/SPD + Sensoren | 🔧 |
| S9 | P1 | **Start direkt auf dem Beelink** (ohne KVM) | nie gemacht – braucht Freigabe; schaltet VT-d/NVMe/iGPU/BT/WLAN/Xe auf echter HW frei (Live §0) |
| S10 | P2 | iGPU beim Start | begonnen; echte Prüfung erst nach S9 (Live §4) |
| S11 | P3 | NPU | begonnen |
| S12 | P2 | WLAN/BT/SD/HDA/Xe | **SD + BT-Erkennung belegt**; BT-Firmware/WLAN/Xe am Live-Server (Live §1–§3); HDA offen |

## 1b. USB-Netz + SSH – was noch fehlt (Prio P0 laut Nutzer)

Ziel: „über USB-C und SSH mit dem Server auf Ultar OS verbinden und dort weiterarbeiten." Stand der drei Teile:
1. **USB-Netz-Treiber** – **RNDIS auf `main`** (TX ok, RX am echten Dongle zu bestätigen).
   - USB-C-zu-Ethernet-Dongle: oft **CDC-ECM/NCM** (Norm) oder Vendor (AX88179/RTL8153) → je nach Hardware noch ein Treiber.
   - Handy-USB-Tethering: **RNDIS** (vorhanden) oder CDC-ECM/NCM.
2. **TCP** – **schon vorhanden** (`kernel/ultar-kern/src/socket.rs`): Sockets für Programme (Linux-ABI) AF_INET mit
   UDP **und TCP, Client und Server** – Drei-Wege-Handshake, Folgenummern, Bestätigungen, Wiederholung, FIN/RST,
   Annahme-Warteschlange. Kein neuer Stapel nötig; offen ist nur die Prüfung über RNDIS.
3. **SSH-Server** – **schon vorhanden**: `system/services/ultar-sshd` (russh, nur Schlüssel, nur bei eingeschaltetem
   Entwickler-Zugang, SFTP/scp, Weiterleitung nur zu 127.0.0.1), läuft laut Quelltext auf dem Ultar-Kern.
→ Offen für „auf Ultar OS weiterarbeiten": RNDIS-Empfang am echten Gerät, dann Ende-zu-Ende „ssh über USB-C"
  (sshd im Abbild + Entwickler-Zugang an); CDC-ECM/NCM, falls der Dongle kein RNDIS spricht.

## 2. Sicherheit – Rest (P0)

**Erledigt 28.11.2026** (Details `docs/review/OFFENE-BEFUNDE.md`): R2-2…R2-5, S-9, `mprotect` (neuer `schutztest`),
S-11 (keine HCI-Herstellerbefehle), S-13 (`--input-ack` nur in Testbauten), S-15, `CLONE_THREAD` ohne `CLONE_VM`.

Offen (größere Arbeiten):
- **R1** – 19 Dienstdateien ohne eigenen Benutzer. Davon by design Verwalter: `shell`, `entwickler`; nur Test:
  `selftest`, `tontest`, `webtest`, `wifitest`; alte Oberfläche: `ui`, `verbund` (hängt an `ui`); Handy-Hersteller-HAL
  (Android-Binärdateien erwarten root): `gnss`, `fingerabdruck`, `modem`, `ril`; Einmal-Läufe: `drivers`, `compat-bind`,
  `firstboot`, `netzregeln`; `seat`, `net`, `llm-compat`. Echt aufzuteilen: `net` (Adressen/Routen-Helfer hinter dem
  Bus, DHCP/WLAN als Benutzer), `netzregeln`/`drivers`/`seat` (eigener Benutzer + nur ihr Kern-Recht), `ril`.
- ✅ **LUKS2 im Installer** (28.11.2026): `ultar-luks` (nur RustCrypto), XTS im Kern (`crypt.rs`), Entsperren beim
  Start durch ultar-init; geprüft gegen cryptsetup/dm-crypt (`luks-interop.sh`, `crypt-data.sh`). Rest: grafische
  Abfrage, ext4 im verschlüsselten Bereich wachsen lassen, TPM (S7).
- Anruf-Bildschirm an „klingelt" von `calld` binden · BT-Tastaturen über feste Erzeuger-Kennung zulassen ·
  Einstellungen mit URI-Schema `settings://apps/<id>` · `mprotect`/`munmap` ohne Abgleich anderer Prozessorkerne
  (TLB-Shootdown).
- leicht: SCM_CREDENTIALS, devd-Index ohne Seriennummer, Bus-Grenzen je Kennung, Einstellungsdienst-Reste,
  Server-Härtung (Namen normalisieren, Geräte je Konto, Signal-Ratenbegrenzung), S-14.

## 2b. Installation – Rest

| # | Prio | Was | Stand |
|---|---|---|---|
| I1 | P1 | **Test in VMware Workstation** mit `UltarOS-v2-installer.zip` (Actions → „Installer image") | bereit – Test des Inhabers; bei Problemen `serial.log` |
| I2 | P0 | Verschlüsselung | ✅ Passphrase (LUKS2); TPM offen |
| I3 | P2 | Netzwerk im Installer (Medium ohne Netzdienst; „Updates während der Installation" wirkt erst danach) | ⬜ |
| I4 | P2 | Nach „Neu starten" kann VMware wieder vom Medium starten → SATA-Platte von Hand entfernen; Startreihenfolge automatisch | ⬜ |

## 3. Desktop/PC (P1)

Intel-Grafik (Modus, mehrere Bildschirme, Hotplug) + Rendern/PRIME; virtio-gpu für VMware/QEMU; Ton/Touchpad/BT auf
echter HW; ThinkPad (T1) erstmals. Einstellungen „In dieser Version nicht verfügbar": Dienste je Person als nächstes.
Hinweis: Desktop-E2E (`DESKTOP: no desktop bar`) scheitert auf den headless-Servern vorbestehend (gleich auf `main`).

## 4. Verbund und Helfer (P2) · 5. Apps/Handy/Rest (P2/P3)

A5/A6, B3–B7, C1/C2/C4/C5/C8/C13 · Büro-Apps, Servo-Browser, ext4-Umbenennen, A30-Abbild,
i18n-Umstellung, Bezeichner englisch, ARM64, arabisch gegenlesen; Stufe 4 NVIDIA/DisplayLink/Fingerabdruck.

## Abarbeitung (Reihenfolge)

1. **Q1** Repo-Prüfung grün halten (vor jedem Merge `ultar-check all`).
2. **USB-C + SSH Ende zu Ende** (TCP und `ultar-sshd` sind da): RNDIS-Empfang am echten Gerät, sshd im Abbild mit
   Entwickler-Zugang, `ssh` über USB-C prüfen; CDC-ECM/NCM + Vendor (AX88179/RTL8153), falls der Dongle kein RNDIS spricht.
3. **I1** VMware-Installationstest durch den Inhaber; Befunde beheben.
4. Sicherheit Rest: TLB-Shootdown → R1 (`net`, `netzregeln`, `drivers`, `seat`, `ril`); LUKS2 ✅.
5. Welle Server B: S5 AML → S6 Energie → S8 Sensoren → S7 Wachhund + TPM.
6. **Live-Session am Beelink (S9, mit Freigabe):** `docs/LIVE-VOLLENDUNG.md` §1→§4 abarbeiten – BT-Firmware,
   WLAN, Xe, dann VT-d/NVMe/SD/iGPU auf echter HW + E610-Durchsatz.
7. Desktop; Verbund/Helfer; Apps; NPU; Handy.

Offene Entscheidungen des Inhabers: S9-Freigabe, USB-Netz-Hardware, B3, C5.

Regeln: Clean Code, kleine Schritte, gemeinsame Kern-Dateien zentral (nicht parallel), nichts an iGPU/NPU-Arbeit
anderer Chats stören, Messungen am Beelink nur mit angehaltenen Störern, nie auf dem VPS bauen.
