# Treiberziele – zwei Testrechner vollständig

Stand 22.11.2026 · Ziel: Ultar-Kern (ohne Linux) läuft auf **beiden** Rechnern vollständig, jeder Treiber als `.utr`
im Installationsmedium.

- **T1** Lenovo ThinkPad P1 Gen 3 – Laptop (Akku, Panel 4K, Touchpad, WLAN) → `devices/thinkpad-p1-gen3/`
- **T2** Beelink GTi15 Ultra, Core Ultra 9 285H, `root@192.168.100.18` – Mini-PC/Server → `devices/beelink-gti15/`

Stand: ✅ vorhanden (in ultar-os) · 🔧 vorhanden, am Gerät noch zu prüfen · ⬜ neu zu schreiben · ⛔ Grenze

## Stufe 1 – startet, installiert, bedienbar (zuerst)

| Treiber | T1 | T2 | Stand | Unterlage |
|---|---|---|---|---|
| Plattform: ACPI (Tabellen + AML-Interpreter), x2APIC, HPET/TSC, SMP, P-/E-Kerne | ✔ | ✔ | 🔧 Grundlagen da; **AML-Interpreter ⬜** (nötig für Akku, Deckel, Tasten, Thermik, Energie) | ACPI 6.5 |
| NVMe | PM981a | Crucial E100 ×2 | 🔧 am echten Gerät prüfen | NVMe 2.0 |
| xHCI + Hubs + HID (Tastatur/Maus) + Massenspeicher | ✔ | ✔ | ✅ T2 geprüft, T1 🔧 | xHCI 1.2 |
| Anzeige über UEFI-GOP (feste Auflösung) | ✔ | ✔ | ✅ | UEFI |
| Ethernet Intel I219-LM | ✔ | – | 🔧 `gen-intel-eth` deckt I219 ab, am Gerät prüfen | Intel-Datenblatt |
| Ethernet Intel E610 10 GbE | – | ✔ | ✅ geprüft; Verbindung mit Kabel, MSI-X offen | Intel-Datenblatt |
| MSI/MSI-X, IOMMU (DMAR) für DMA-Schutz | ✔ | ✔ | ⬜ | PCI 3.0, VT-d |
| PS/2 (Tastatur, TrackPoint) | ✔ | – (kein i8042) | ✅ | |

## Stufe 2 – vollwertiger Desktop

| Treiber | T1 | T2 | Stand | Unterlage / Aufwand |
|---|---|---|---|---|
| **Intel-Grafik Anzeige** (Modus setzen, mehrere Ausgänge, Hotplug, Helligkeit, Schlafmodus) | UHD Gen 9.5 | Arc 140T (Xe) | ⬜ zwei Generationen, ein Treiber mit zwei Plattform-Teilen | Intel PRM (öffentlich) · groß |
| **Intel-Grafik 3D/Video-Beschleunigung** (für Compositor, 4K flüssig) | ✔ | ✔ | ⬜ nach der Anzeige; ohne sie bleibt 4K Software-Zeichnen | Intel PRM · sehr groß |
| HD Audio: Codec-Wege echter Laptops, Aufnahme, Kopfhörer-Erkennung, Schnittstelle für Programme | ALC-Codec | ALC-Codec | 🔧 `gen-hda` erweitern | HDA 1.0a |
| I2C (LPSS) + HID-über-I2C (Touchpad, Mehrfinger-Gesten) | ✔ | – | ⬜ | HID over I2C |
| Akku, Netzteil, Deckel, Tasten (Helligkeit/Lautstärke), Thermik, Lüfter – über ACPI/EC | ✔ | Thermik/Lüfter | ⬜ (braucht AML) | ACPI, ThinkPad-EC |
| Energie: Frequenz (HWP), C-Zustände, Schlafen S3/s2idle, TDP-Grenzen (RAPL) | ✔ | ✔ | ⬜ | Intel SDM |
| USB-Klassen: Audio (Headset), UVC (Kamera), CDC/ECM | ✔ | ✔ | ⬜ | USB-Klassen-Normen |
| SD-Kartenleser | Genesys | O2 Micro | ⬜ SDHCI für T2; Genesys prüfen | SD Host Spec |
| SMBus/SPD, Sensoren | ✔ | ✔ | 🔧 | SMBus 2.0 |
| TPM 2.0 (für Verschlüsselung im Installer) | ✔ | ✔ | ⬜ | TCG TIS/CRB |
| Thunderbolt/USB4 (PCIe-Tunnel, Hotplug) | Titan Ridge | integriert | ⬜ spät; USB-C-Anzeige läuft über die Grafik | USB4-Norm |

## Stufe 3 – Funk und Spezialchips

| Treiber | T1 | T2 | Stand | Hinweis |
|---|---|---|---|---|
| **WLAN Intel** | AX201 | BE200 | ⬜ | Braucht die **geschlossene Intel-Firmware** (frei weitergebbar, wird nur geladen). Die Firmware-Schnittstelle ist nur über den Linux-Quelltext beschrieben → eigener Treiber nach dieser Beschreibung + eigener 802.11-Stapel (WPA2/WPA3). Größtes Einzelpaket. |
| Bluetooth Intel (USB-HCI + Firmware) | ✔ | ✔ | ⬜ | HCI ist Norm; Firmware geschlossen; `ultar-bt`-Host wiederverwenden |
| NPU (Intel AI Boost) | – | ✔ | ⬜ | keine offene Norm, aber offener Linux-Treiber als Beschreibung; Firmware geschlossen |

## Grenzen – ehrlich

| Gerät | Grund | Ausweg |
|---|---|---|
| ⛔ **NVIDIA Quadro RTX 4000** (T1) | keine öffentliche Registerbeschreibung; Beschleunigung nur mit signierter Hersteller-Firmware | Das Laptop-Panel hängt an der Intel-Grafik → Bild und Desktop laufen ohne NVIDIA. Die NVIDIA-Karte wird abgeschaltet (Strom sparen). Externe Ausgänge, die fest an der NVIDIA hängen, bleiben dunkel. |
| ⛔ **DisplayLink-Dock Dell D3100** (T1, deine 3 Monitore) | Protokoll geschlossen und verschlüsselt | Monitor direkt per USB-C/Thunderbolt/HDMI anschließen oder Dock mit DisplayPort-Alt-Mode. Ethernet/Ton des Docks sind USB-Normklassen und machbar. |
| ⛔ Fingerabdruck Synaptics (T1), Fingerabdruck 1c7a:0577 (T2) | geschlossenes Protokoll | später, falls Hersteller-Doku auftaucht |
| ⛔ Intel ME/AMT | bewusst kein Treiber (Angriffsfläche) | – |

## Reihenfolge und Prüfung

1. Stufe 1 auf beiden Rechnern vom USB-Stick (Selbsttest schreibt Protokoll auf den Stick) – T2 braucht dafür einen
   Neustart des Servers, **nur nach Absprache**; bis dahin Einzelgeräte per VFIO wie bisher.
2. AML-Interpreter → Akku/Thermik/Energie.
3. Intel-Grafik Anzeige (T1 Gen 9.5 zuerst, da Laptop-Panel; dann T2 Xe).
4. Ton, Touchpad, USB-Klassen, TPM.
5. WLAN/Bluetooth, dann 3D-Beschleunigung, zuletzt NPU und Thunderbolt.

Jeder Treiber gilt erst als fertig mit: Manifest `.utr`, Prüfung in QEMU (wo emulierbar) **und** am echten Gerät,
Eintrag in der Paritätstabelle, arabischen Anzeigenamen.
