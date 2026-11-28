# IOMMU (Intel VT-d) – DMA-Schutz im Ultar-Kern

Stand 28.11.2026 · `kernel/ultar-kern/src/iommu.rs` · eingeschaltet in `main.rs` nach `pci::scan`, vor `bind_all`.

## 0. Stand und offene Punkte

| Stufe | Inhalt | Stand |
|---|---|---|
| V1 | DMAR lesen, jede Remapping-Einheit einschalten; eine gemeinsame zweite Übersetzungsebene bildet **nur den nutzbaren RAM** 1:1 ab (2-MiB-Großseiten), sonst nichts; Standard-Domäne für alle Bus/Gerät/Funktion | **fertig, in QEMU geprüft** (`-device intel-iommu`): „VT-d 1.0: 1/1 Einheit(en) an", alle Geräte (virtio-blk, virtio-net) arbeiten durch die eingeschaltete IOMMU, DHCP holt eine Adresse; keine vIOMMU-Fehler |
| V2 | eine eigene Domäne je Gerät mit **nur seinen** DMA-Puffern (statt des ganzen RAM) | offen – braucht, dass der DMA-Zuteiler (`mem::alloc_pages` für Ringe/Puffer) je Treiber eine Domäne kennt |
| V3 | Fehler-Unterbrechung (FECTL/Fault-Event) statt nur FSTS-Abfrage; Protokoll je Fehler (Gerät, Adresse) | offen |
| V4 | Prüfung auf echter Hardware (Beelink, 2 Einheiten) | offen – braucht den Start direkt auf dem Gerät (nicht über VFIO; die Gast-VT-d über die echte DMAR zu schachteln geht so nicht) |

## 1. Warum

Ohne IOMMU darf jedes Gerät per Bus-Master in **jeden** physischen Speicher schreiben – der klassische
Angriff eines bösartigen oder fehlerhaften Geräts, auch über Modem-/Chip-Firmware (Sicherheitsziel des
Projekts: kein Datenverkehr am System vorbei). VT-d übersetzt jede Geräte-Adresse über Seitentabellen;
was dort nicht abgebildet ist, löst einen Fehler aus statt eines Zugriffs.

Stufe 1 bildet den ganzen RAM 1:1 ab: Geräte erreichen weiter ihre Puffer (die im RAM liegen), aber **kein
Gerät mehr Firmware, SMRAM, die MMIO-Register anderer Geräte oder Speicherlöcher**. Das ist der erste
Riegel und der Nachweis, dass die Übersetzung erzwungen wird. Die starke Trennung „jedes Gerät nur seine
Puffer" kommt in V2.

## 2. Aufbau

- **DMAR** (ACPI): nennt die Remapping-Einheiten (DRHD) mit Registerbasis. Die erste CAP bestimmt die
  Adressbreite (SAGAW: Bit 9 = 39 Bit/3 Ebenen, Bit 10 = 48 Bit/4 Ebenen).
- **Zweite Ebene** (`build_slpt`): `[0, RAM-Spitze)` mit 2-MiB-Großseiten (PS-Bit), Lesen+Schreiben, 1:1.
- **Wurzel-/Kontexttabelle** (`build_root`): eine Kontexttabelle, von allen 256 Bussen geteilt; jeder
  Eintrag zeigt auf die zweite Ebene (Domäne 1, TT=00 nur zweite Ebene).
- **Einschalten** (`enable_unit`): RTADDR setzen → GCMD.SRTP (warten auf GSTS.RTPS), Kontext- und
  IOTLB-Zwischenspeicher global leeren (CCMD CIRG=01 in Bit 62:61; IOTLB IIRG=01 in Bit 61:60),
  GCMD.TE (warten auf GSTS.TES).

Unterbrechungs-Umleitung bleibt **aus** – die MSI (`msi`) behalten das einfache Format.

## 3. Prüfen

```bash
# QEMU mit emulierter IOMMU: Geräte müssen durch VT-d weiter funktionieren
qemu-system-x86_64 -machine q35 -device intel-iommu,intremap=off,caching-mode=on ... \
  -device virtio-blk-pci,... -device virtio-net-pci,... -serial file:log
grep iommu log   # [iommu] VT-d 1.0: 1/1 Einheit(en) an ...
grep DHCP  log   # Adresse geholt -> RX/TX-DMA läuft durch die IOMMU
```

`sicherheit` in der Kern-Shell zeigt `dma-schutz (vt-d) an` (und „DMA-Fehler gemeldet", falls FSTS Bit 1).

## 4. Quellen

Intel® Virtualization Technology for Directed I/O, Architecture Specification (öffentlich): Kapitel
Root/Context-Entry, Second-Level Translation, Register (CAP/ECAP/GCMD/GSTS/RTADDR/CCMD/IOTLB).
