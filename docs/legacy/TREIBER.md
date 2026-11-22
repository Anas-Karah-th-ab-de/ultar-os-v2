# Ultar-Kern – Treiberverzeichnis

KM Code Craft · `kernel/ultar-kern/src/drivers/` · Stand 22.11.2026

Jeder Treiber ist eigener Rust-Code (kein Linux-Code) und meldet sich über `driver::Driver` an
(`name`, `origin`, `matches`, `probe`). Reihenfolge und Liste: `drivers/mod.rs`. In der Kern-Konsole:
`geraete` (Gerät → Treiber), `treiber` (alle Treiber), `werkstatt` (Geräte ohne Treiber als Bauplan).

## Treiber

| Treiber | Datei | Herkunft | Geräte (PCI) | Norm / Unterlage | Leistet | Befehl |
|---|---|---|---|---|---|---|
| `bruecke` | `bruecke.rs` | hand | Klasse 06 (Host-, ISA/LPC-, PCI-PCI-Brücke) | PCI Local Bus 3.0 | übernimmt Brücken, schaltet bei PCI-PCI die Weiterleitung ein | `geraete` |
| `nvme` | `nvme.rs` | hand | Klasse 01.08.02 | NVM Express 2.0 | Platten lesen/schreiben | `platten` |
| `virtio-blk` | `virtio_blk.rs` | hand | 1af4:1001 / 1042 | virtio 1.2 §5.2 | Platten lesen/schreiben | `platten` |
| `virtio-net` | `virtio_net.rs` | hand | 1af4:1000 / 1041 | virtio 1.2 §5.1 | Ethernet | `netz` |
| `virtio-rng` | `virtio_rng.rs` | hand | 1af4:1005 / 1044 | virtio 1.2 §5.4 | Zufall vom Wirt in den Kern-Vorrat (auch für `getrandom`) | `zufall` |
| `ps2kbd`, `ps2maus` | `ps2kbd.rs`, `ps2maus.rs` | hand | i8042 (kein PCI) | IBM PS/2, VMware-vmmouse | Tastatur (de/us/ar), Maus relativ und absolut | `tastatur` |
| `gen-ahci` | `gen_ahci.rs` | Werkstatt | Klasse 01.06.01 | Serial ATA AHCI 1.3.1 | SATA-Platten lesen/schreiben | `platten` |
| `gen-intel-eth` | `gen_intel_eth.rs` | Werkstatt | 8086: e1000/e1000e/I217–I219 | Intel-Datenblätter 8254x/82574 | Ethernet | `netz` |
| `gen-ixgbe` | `gen_ixgbe.rs` | Werkstatt | Intel 10 GbE (E610 u. a.) | Intel-Datenblatt | Ethernet 10 GbE (am Gerät server3 geprüft) | `netz` |
| `gen-xhci` | `gen_xhci.rs` | Werkstatt | Klasse 0c.03.30 | xHCI 1.2, USB HID/Mass Storage | USB-Tastaturen, USB-Sticks/-Platten | `platten` |
| `gen-bochs-vga` | `gen_bochs_vga.rs` | Werkstatt | 1234:1111, 80ee:beef | Bochs VBE-DISPI (MMIO BAR2+0x500) | Grafikkarte erkennen, Auflösung zur Laufzeit umstellen (Konsole und `/dev/fb0` folgen) | `anzeige [BxH]` |
| `gen-hda` | `gen_hda.rs` | Werkstatt | Klasse 04.03 | Intel High Definition Audio 1.0a | Controller, CORB/RIRB, Codec-Weg Pin ← DAC, Ausgabe 48 kHz/16 Bit/Stereo | `ton [hz] [ms]` |
| `gen-smbus` | `gen_smbus.rs` | Werkstatt | 8086, Klasse 0c.05 | SMBus 2.0, Intel ICH9/PCH-Datenblatt | „Byte lesen“, Speicherriegel-Kennungen (SPD) | `smbus [adr reg]` |

## Geprüft (QEMU 8.2, WSL-Rechner aus `scripts/wsl-x11.py`)

```
00:00.0 8086:29c0  Host-Brücke        bruecke
00:01.0 1b36:0010  NVMe-Speicher      nvme
00:02.0 1234:1111  Grafik (VGA)       gen-bochs-vga
00:03.0 8086:10d3  Ethernet           gen-intel-eth
00:04.0 1b36:000d  USB 3 (xHCI)       gen-xhci
00:05.0 8086:293e  Audio (HD Audio)   gen-hda
00:06.0 1af4:1005  unbekannt          virtio-rng
00:1f.0 8086:2918  ISA-Brücke         bruecke
00:1f.2 8086:2922  SATA (AHCI)        gen-ahci
00:1f.3 8086:2930  SMBus              gen-smbus
[werkstatt] alle Geräte haben einen Treiber
```

- **Ton:** `ton 880 500` – QEMU-Aufzeichnung (WAV) ausgewertet: 880 Hz, volle Lautstärke des Prüftons.
- **Grafik:** `anzeige 1920x1080` – Bildschirmfoto hat danach 1920×1080.
- **Zufall:** `zufall` – 16 Byte vom Wirt, Kern-Vorrat gemischt.
- Wiederholen: `scripts/wsl-x11.py geraete` (endet mit Fehler, wenn ein Gerät ohne Treiber bleibt).

## Offen (ehrlich)

- **Ton für Programme:** `gen-hda` hat noch keine Geräteschnittstelle (`/dev/snd/…`, ALSA-ioctl) – Programme wie
  `ultar-shell` hören den Kern noch nicht. Aufnahme (ADC), Lautstärkeregelung und Kopfhörer-Erkennung fehlen.
  Auf echter Hardware ungeprüft (Codec-Wege von Realtek/Conexant sind verzweigter als im QEMU-Codec).
- **Grafik:** nur Bochs/QEMU-Karten umstellbar; echte GPUs (Intel/AMD/Nvidia) bleiben beim Bild der Firmware (GOP).
  Umstellen ist in der Kern-Konsole geprüft, nicht bei laufender Desktop-Oberfläche.
- **AHCI:** CD/DVD-Laufwerke (ATAPI) werden erkannt, aber nicht gelesen.
- **SMBus:** nur „Byte lesen“; Sensoren (Temperatur, Lüfter) brauchen eigene Gerätetreiber darüber.
- **Nicht vorhanden:** WLAN, Realtek-Ethernet, USB-Hubs, virtio-gpu/-input, Bluetooth.

## Neuen Treiber anlegen

1. `werkstatt` in der Kern-Konsole → Bauplan (JSON) des Geräts.
2. `tools/treiber-werkstatt/werkstatt.py erzeugen <art>` → Gerüst `drivers/gen_<art>.rs`, Eintrag in `mod.rs`, Auftrag.
3. Aus der offenen Norm ausschreiben (`ki`), mit `pruefen` in QEMU bestätigen, hier eintragen.
