# Ultar-Kern auf server3 (Beelink GTi15 Ultra) – Treiber-Stand

KM Code Craft · Stand 22.11.2026 · Rechner `root@192.168.100.18` (server3) · Inventur: `tools/kern-test/hw-inventur.sh`

## Gerät

| Teil | Wert |
|---|---|
| Rechner | Beelink GTi15 Ultra (AZW, BIOS AMI, Version GTi15T203), UEFI, Secure Boot aus |
| Prozessor | Intel Core Ultra 9 285H (Arrow Lake-H), 16 Kerne (6 P + 8 E + 2 LP-E), x2APIC vom BIOS eingeschaltet, APIC-Kennungen 0…66 (Lücken) |
| Speicher | 91 GB |
| Zeitgeber | TSC 3686,4 MHz (CPUID 0x15), HPET 19,2 MHz, **kein i8042** (FADT IAPC_BOOT_ARCH = 0), kein COM1 (nur LPSS-UART per MMIO) |
| ACPI | FADT Rev. 6, PM1a_CNT 0x1804 (Ausschalten über S5), Reset-Register 0xCF9 ← 6, MADT mit 16 aktiven x2APIC-Einträgen + 32 leeren, DMAR (2 IOMMU) |

## Geräte und Treiber im Ultar-Kern

| PCI | Gerät | Treiber | Stand |
|---|---|---|---|
| ad:00.0 / ad:00.1 | Intel E610-XT2, 2 × 10GBASE-T (8086:57b0) | **gen-ixgbe** (neu, Werkstatt) | **am echten Gerät geprüft** (VFIO): Rücksetzen, MAC aus NVM, Firmware-Schnittstelle ACI (FW 7.8.18, API 1.7), Verbindungsstatus, Senden + Empfangen über interne Schleife ✅. Verbindungsaufbau mit Gegenstelle: noch mit Kabel prüfen |
| 01:00.0 / 02:00.0 | 2 × Crucial E100 NVMe 1 TB (c0a9:560b, ohne DRAM) | nvme (hand) | Norm-Treiber (Klasse 01:08:02); am Gerät nicht geprüft, weil beide Platten Linux tragen |
| 00:14.0 | Intel PCH-xHCI (8086:777d) | gen-xhci | **am echten Gerät geprüft** (VFIO): 14 Anschlüsse, alle Geräte erkannt, beide ASMedia-Hubs (USB 2: 174c:2174, USB 3: 174c:3274) angemeldet |
| 00:0d.0 | Intel Typ-C-xHCI (8086:7ec0) | gen-xhci | gleiche Norm wie 00:14.0 |
| – | USB-Hubs (USB 2.0 Kap. 11, USB 3.2 Kap. 10) | gen-xhci (neu) | bis 5 Ebenen, TT für Low/Full-Speed hinter High-Speed-Hubs, Rückstellung nach STALL; in QEMU (Hub im Hub, Tastatur + Stick) und am echten Gerät geprüft |
| – | Tastatur (PS/2) | ps2kbd/ps2maus | **Fehler behoben**: ohne i8042 (Port 0x64 liest 0xFF) hing der Kern in der Maus-Einrichtung; jetzt Erkennung und Abbruch – Tastatur/Maus nur über USB |
| – | Zeitmessung | arch | **Fehler behoben**: TSC kommt jetzt aus CPUID 0x15/0x16 (Arrow Lake schaltet den PIT oft ab); PIT nur noch als Rückfall, mit Grundtakt, falls er nicht zählt. Am Gerät: 3687 MHz ✅ |
| 00:02.0 | Intel Arc 140T (8086:7d51) | gop (Firmware) über die Anzeige-Schicht | Bild über den UEFI-Bildspeicher, feste Auflösung; eigener Anzeigetreiber: Bauplan `intel-xe-anzeige` (Intel PRM, offen). **Am echten Gerät noch nicht geprüft** (braucht Start per Stick) |
| 00:1f.3 | HD-Audio (8086:7728) | – | Bauplan „hda“; für den Server nicht nötig |
| ab:00.0 | SD-Kartenleser O2 Micro (1217:9863) | – | Bauplan „sdhci“ (SD Host Controller Simplified Spec) – offen |
| ac:00.0 | WLAN Intel BE200 (8086:272b) | – | Grenze: braucht Herstellerfirmware + Registerbeschreibung |
| 00:0b.0 | NPU (8086:7d1d) | – | Grenze (keine offene Norm) |
| 00:0d.2/.3 | Thunderbolt DMA | – | für den Server nicht nötig |
| USB | Fingerabdruck 1c7a:0577, Bluetooth 8087:0036, USB-Audio 1d6b:a4a7 | – | erkannt (Kennung), ohne Klassentreiber |

## Geräteart

SMBIOS meldet Gehäuse 35 „Mini-PC“, die ACPI-FADT dagegen „mobil“ (Laptop-Chip). Der Kern nimmt SMBIOS → **PC, Ausgabe desktop**.
Geprüft in QEMU mit der echten SMBIOS-Tabelle von server3 (`-smbios file=`) sowie Varianten Laptop/Server.
Für den Betrieb als Server: `server` in `/etc/ultar/edition` eintragen.

## Prüfung am echten Gerät ohne Neustart des Servers

Der Server läuft weiter unter Linux. Einzelne, für den Betrieb unwichtige Geräte werden kurz an eine
QEMU/KVM-Maschine mit dem Ultar-Kern durchgereicht (VFIO, IOMMU ist an) und danach an Linux zurückgegeben:

```bash
# freier 10G-Port (ad:00.0) – der Port mit der Serververbindung (ad:00.1) wird nie angefasst
ssh root@192.168.100.18 'bash -s' < tools/kern-test/vfio-e610.sh
# PCH-xHCI (00:14.0 + 00:14.2, eine IOMMU-Gruppe)
ssh root@192.168.100.18 'bash -s' < tools/kern-test/vfio-xhci.sh
```

Ergebnis 22.11.2026:

```
[ixgbe] eth0: 57b0 E610 MAC 78:55:36:04:b2:94 · Firmware 7.8.18 API 1.7 · Kabel steckt (Rahmen bis 1518 Byte)
        · Verbindung fehlt – Schleifentest: ok (Senden und Empfangen geprüft)
[xhci] Controller 8086:777d: 14 Port(s), 32 Slots, Kontext 32 Byte
[xhci] Port 4: Hub mit 4 Anschlüssen 174c:2174
[xhci] Port 14: Hub mit 4 Anschlüssen 174c:3274
```

## Hinweise

- **10G-Port nur mit 100 Mbit/s:** Linux meldet am genutzten Port enp173s0f1 „Link is Up 100 Mbps“. Kabel (Cat 6/6A)
  oder Switch-Port prüfen – der E610 kann 10/5/2,5/1 Gbit/s.
- Vollständiger Start des Ultar-Kerns direkt auf server3 (USB-Stick, `scripts/make-kern-usb.sh`, Selbsttest schreibt
  `ultar-kern/selbsttest.txt`) braucht einen Neustart des Servers – erst nach Absprache.
- Offen für den Serverbetrieb: NVMe am echten Gerät, Verbindungsaufbau E610 mit Kabel, Neu-Erkennung bei
  nachträglich eingesteckten USB-Geräten (Hub-/Port-Ereignisse), MSI-X statt Abfragebetrieb.
