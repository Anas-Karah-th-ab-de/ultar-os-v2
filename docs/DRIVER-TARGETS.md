# Treiberziele – zwei Testrechner vollständig

Stand 22.11.2026 · Ziel: Ultar-Kern (ohne Linux) läuft auf **beiden** Rechnern vollständig, jeder Treiber als `.utr`
im Installationsmedium.

- **T1** Lenovo ThinkPad P1 Gen 3 – Laptop (Akku, Panel 4K, Touchpad, WLAN) → `devices/thinkpad-p1-gen3/`
- **T2** Beelink GTi15 Ultra, Core Ultra 9 285H, `root@192.168.100.18` – Mini-PC/Server → `devices/beelink-gti15/`

Stand: ✅ vorhanden (in ultar-os) · 🔧 vorhanden, am Gerät noch zu prüfen · ⬜ neu zu schreiben · 🔬 Erkundung nötig (Stufe 4)

**Grundsatz: Es gibt keine „Grenze“.** Jedes Gerät der zwei Rechner bekommt einen Treiber. Wo der Hersteller nichts
offenlegt, wird beobachtet, wie Ubuntu und Windows das Gerät ansprechen, daraus eine eigene Beschreibung geschrieben
und danach der Treiber gebaut – als letzter Bauteil (Stufe 4).

## Stufe 1 – startet, installiert, bedienbar (zuerst)

| Treiber | T1 | T2 | Stand | Unterlage |
|---|---|---|---|---|
| Plattform: ACPI (Tabellen + AML-Interpreter), x2APIC, HPET/TSC, SMP, P-/E-Kerne | ✔ | ✔ | 🔧 Grundlagen da; **AML-Interpreter ⬜** (nötig für Akku, Deckel, Tasten, Thermik, Energie) | ACPI 6.5 |
| NVMe | PM981a | Crucial E100 ×2 | 🔧 am echten Gerät prüfen | NVMe 2.0 |
| xHCI + Hubs + HID (Tastatur/Maus) + Massenspeicher | ✔ | ✔ | ✅ T2 geprüft, T1 🔧 | xHCI 1.2 |
| Anzeige über UEFI-GOP (feste Auflösung) | ✔ | ✔ | ✅ | UEFI |
| Ethernet Intel I219-LM | ✔ | – | 🔧 `gen-intel-eth` deckt I219 ab, am Gerät prüfen | Intel-Datenblatt |
| Ethernet Intel E610 10 GbE | – | ✔ | ✅ geprüft; Verbindung mit Kabel, MSI-X offen | Intel-Datenblatt |
| MSI/MSI-X, IOMMU (DMAR) für DMA-Schutz | ✔ | ✔ | 🔧 MSI/MSI-X: Rahmen `msi.rs` (Vektoren 0x90–0xEF, Tabelle in BAR); E610-10G nutzt es, am echten Gerät (VFIO) belegt. IOMMU `iommu.rs`: VT-d an, RAM 1:1, in QEMU geprüft; echte 2 Einheiten des Beelink: beim Direktstart | PCI 3.0, VT-d |
| PS/2 (Tastatur, TrackPoint) | ✔ | – (kein i8042) | ✅ | |

## Stufe 2 – vollwertiger Desktop

| Treiber | T1 | T2 | Stand | Unterlage / Aufwand |
|---|---|---|---|---|
| **Intel-Grafik Anzeige** (Modus setzen, mehrere Ausgänge, Hotplug, Helligkeit, Schlafmodus) | UHD Gen 9.5 | Arc 140T (Xe) | ⬜ zwei Generationen, ein Treiber mit zwei Plattform-Teilen | Intel PRM (öffentlich) · groß |
| **Intel-Grafik 3D/Rechnen** (für Compositor, 4K flüssig, lokale Modelle) | ✔ | ✔ | 🔧 T2: Stufe G0/G1a am Gerät geprüft (VFIO, 24.11.2026: Erkennung, Forcewake, Engines, GuC-Firmware geladen, GuC meldet „bereit“ und beantwortet Anfragen – Hardware-Tabelle gelesen, schneller Nachrichtenkanal CTB läuft; erste Rechenaufträge auf Kopier-, Render- und Rechen-Engine mit eigenen Seitentabellen schreiben Werte in den Speicher, 25.11.2026); Schnittstelle für Programme wie Linux `xe` auf `/dev/dri/renderD128`: Puffer, Adressräume, Aufträge, Synchronisationsobjekte – ein Testprogramm rechnet auf allen drei Engines; Mesa noch nicht gelaufen; Stufen bis Vulkan: `docs/integration/intel-gt.md` · T1 ⬜ | Linux `xe` (MIT), Mesa genxml · sehr groß |
| HD Audio: Codec-Wege echter Laptops, Aufnahme, Kopfhörer-Erkennung, Schnittstelle für Programme | ALC-Codec | ALC-Codec | 🔧 `gen-hda` erweitern | HDA 1.0a |
| I2C (LPSS) + HID-über-I2C (Touchpad, Mehrfinger-Gesten) | ✔ | – | ⬜ | HID over I2C |
| Akku, Netzteil, Deckel, Tasten (Helligkeit/Lautstärke), Thermik, Lüfter – über ACPI/EC | ✔ | Thermik/Lüfter | ⬜ (braucht AML) | ACPI, ThinkPad-EC |
| Energie: Frequenz (HWP), C-Zustände, Schlafen S3/s2idle, TDP-Grenzen (RAPL) | ✔ | ✔ | ⬜ | Intel SDM |
| USB-Klassen: Audio (Headset), UVC (Kamera), CDC/ECM | ✔ | ✔ | ⬜ | USB-Klassen-Normen |
| SD-Kartenleser | Genesys | O2 Micro | 🔧 `gen-sdhci` (SDHCI): Hochfahren am echten O2 Micro (VFIO) belegt – Reset, Fähigkeiten (v5, 200 MHz), Karten-Erkennung; Karten-Init + PIO-Lesen codiert, endgültig mit eingelegter Karte zu prüfen; Schreiben offen; Genesys (T1) prüfen | SD Host Spec |
| SMBus/SPD, Sensoren | ✔ | ✔ | 🔧 | SMBus 2.0 |
| TPM 2.0 (für Verschlüsselung im Installer) | ✔ | ✔ | ⬜ | TCG TIS/CRB |
| Thunderbolt/USB4 (PCIe-Tunnel, Hotplug) | Titan Ridge | integriert | ⬜ spät; USB-C-Anzeige läuft über die Grafik | USB4-Norm |

## Stufe 3 – Funk und Spezialchips

| Treiber | T1 | T2 | Stand | Hinweis |
|---|---|---|---|---|
| **WLAN Intel** | AX201 | BE200 | ⬜ | Braucht die **geschlossene Intel-Firmware** (frei weitergebbar, wird nur geladen). Die Firmware-Schnittstelle ist nur über den Linux-Quelltext beschrieben → eigener Treiber nach dieser Beschreibung + eigener 802.11-Stapel (WPA2/WPA3). Größtes Einzelpaket. |
| Bluetooth Intel (USB-HCI + Firmware) | ✔ | ✔ | ⬜ | HCI ist Norm; Firmware geschlossen; `ultar-bt`-Host wiederverwenden |
| NPU (Intel AI Boost) | – | ✔ | 🔧 Stufe N0 am Gerät geprüft (VFIO, 24.11.2026: Takt, Einschalten bis zum laufenden IP-Block, Ausschalten); noch keine Inferenz; Stufen: `docs/integration/intel-npu.md` | keine offene Norm, aber offener Linux-Treiber als Beschreibung; Firmware geschlossen |

## Stufe 4 – Erkundung: Geräte ohne offene Beschreibung (letzter Bauteil)

| Gerät | Was fehlt | Weg |
|---|---|---|
| 🔬 **NVIDIA Quadro RTX 4000** (T1) | Registerbeschreibung; Start der Karte braucht signierte Hersteller-Firmware (GSP) | Offene Linux-Treiber als Beschreibung (nouveau, Nova, NVIDIAs offene Kernmodule) + eigene Mitschnitte. Hersteller-Firmware wird geladen, nicht ersetzt. Erst Anzeige/Ausgänge, dann 3D, dann Rechnen (für LLM). Sehr groß. |
| 🔬 **DisplayLink-Dock Dell D3100** (T1, 3 Monitore) | Protokoll geschlossen | USB-Mitschnitt unter Windows und Ubuntu (Hersteller-Treiber), vorhandene Community-Analysen der DL-3xxx-Reihe; eigener Treiber: Bild komprimieren → USB. Ethernet/Ton des Docks sind Normklassen (früher, Stufe 2). |
| 🔬 Fingerabdruck Synaptics 06cb:00bd (T1), 1c7a:0577 (T2) | Protokoll geschlossen, teils verschlüsselter Kanal zum Sensor | libfprint und dessen Analysen als Ausgang + USB-Mitschnitt; Abgleich im Sensor (Match-on-Chip) nutzen |
| 🔬 Intel NPU (T2) | keine Norm | offener Linux-Treiber (ivpu) als Beschreibung, Firmware des Herstellers laden |
| 🔬 Thunderbolt-Sonderfunktionen, Genesys-Kartenleser, Thermal/EC-Sonderregister (T1) | lückenhafte Doku | Mitschnitt + ACPI-Tabellen |
| Intel ME/AMT | Beschreibung teils offen | bewusst **ohne** Treiber als Voreinstellung (Angriffsfläche); auf Wunsch später |

### Arbeitsweise „beobachten – verstehen – selbst bauen“ (`treiber/werkstatt/erkundung/`)

1. **Beobachten:** dasselbe Gerät unter Ubuntu und Windows betreiben und mitschneiden –
   USB: `usbmon`/Wireshark, USBPcap · PCI-Register: `mmiotrace`, VFIO-Durchreichung an QEMU mit Zugriffsprotokoll ·
   Windows: ETW-Spuren · dazu ACPI-Tabellen, Firmware-Dateien, offene Linux-Treiber lesen.
2. **Verstehen:** aus den Mitschnitten eine **eigene Beschreibung** schreiben (Register, Befehle, Abläufe:
   Einschalten, Modus setzen, Daten senden, Schlafen) – als Bauplan der Werkstatt, mit Wiedergabe-Tests:
   der Mitschnitt wird gegen den neuen Treiber abgespielt und verglichen.
3. **Selbst bauen:** Treiber in Rust nach dieser Beschreibung – eigener Code, kein kopierter Fremdcode
   (Lizenz und Unabhängigkeit bleiben sauber). Hersteller-Firmware wird nur geladen; ob sie mitgeliefert werden darf,
   wird je Datei geprüft, sonst holt der Installer sie vom Gerät/aus dem Netz.
4. **Prüfen** in VFIO und am echten Gerät, dann `.utr`.

Der Rechner T2 (VFIO, IOMMU an) und dein PC (WSL/Windows-Mitschnitt) sind dafür die Messplätze. Ehrlich bleibt:
diese Teile sind die aufwendigsten und haben keinen festen Termin – sie blockieren aber nichts davor, weil Bild
(Intel-Grafik), Netz und Bedienung ohne sie laufen.

## Reihenfolge und Prüfung

1. Stufe 1 auf beiden Rechnern vom USB-Stick (Selbsttest schreibt Protokoll auf den Stick) – T2 braucht dafür einen
   Neustart des Servers, **nur nach Absprache**; bis dahin Einzelgeräte per VFIO wie bisher.
2. AML-Interpreter → Akku/Thermik/Energie.
3. Intel-Grafik Anzeige (T1 Gen 9.5 zuerst, da Laptop-Panel; dann T2 Xe).
4. Ton, Touchpad, USB-Klassen, TPM.
5. WLAN/Bluetooth, dann 3D-Beschleunigung, dann Thunderbolt.
6. Stufe 4 (Erkundung): NPU → Fingerabdruck → DisplayLink → NVIDIA.

Jeder Treiber gilt erst als fertig mit: Manifest `.utr`, Prüfung in QEMU (wo emulierbar) **und** am echten Gerät,
Eintrag in der Paritätstabelle, arabischen Anzeigenamen.

## Prüfung auf dem echten Rechner aus der Ferne (über SSH)

Drei Stufen, jede erst nach bestandener vorheriger:

1. **Einzelgeräte per VFIO** (wie bisher, `tools/kern-test/vfio-*.sh`) – der Server läuft weiter.
2. **Kern unter KVM auf dem Zielrechner** mit echtem Prozessor, allen Kernen und den echten SMBIOS-Tabellen:
   `tools/kern-test/kvm-on-target.sh <kern.efi> <bin-ordner>` – ändert nichts am Rechner.
3. **Einmaliger Start direkt auf der Hardware:** `tools/kern-test/remote-boot.sh check | prepare | go | collect`.
   Der Kern wird mit `--features remote-test` gebaut: Selbsttest, Protokoll auf die EFI-Partition, danach Neustart von
   selbst; hängt er, startet eine Frist (180 s) den Rechner neu. Gestartet wird über „BootNext“ – jeder weitere Start
   führt wieder ins normale System. In QEMU geprüft: Normalfall (Neustart nach 9 s) und absichtlicher Hänger
   (Neustart durch die Frist).
   **Restrisiko:** Hängt der Kern, bevor der Zeitgeber läuft, oder mit gesperrten Unterbrechungen, greift die Frist
   nicht – dann muss einmal jemand den Netzschalter drücken (danach startet wieder das normale System).
