# Ultar OS unter WSL – Bildschirm über X11

KM Code Craft · `scripts/wsl-x11.py` · Stand 22.11.2026

Ultar OS (Ultar-Kern, ohne Linux) läuft in WSL 2 als virtuelle Maschine (QEMU). Das Bild geht als X11-Fenster
an den X-Server unter Windows (VcXsrv, Xming) – nicht an WSLg. Der Ton geht, wenn WSLg da ist, über dessen PulseAudio.

## Voraussetzungen

- WSL 2 mit Ubuntu 24.04: `sudo apt install qemu-system-x86 qemu-system-gui ovmf mtools gdisk dosfstools x11-utils python3-pil`
- VcXsrv unter Windows gestartet (Display 0, „Disable access control“ an).
- Rust mit Ziel `x86_64-unknown-uefi` (siehe `scripts/build-kern.sh`).

## Ablauf

```bash
scripts/wsl-x11.py bauen                   # Kern + Installationsmedium  (UBUNTU=1: mit Ubuntu-26.04-Programmen)
scripts/wsl-x11.py installieren --auto     # Installation läuft von selbst (im Fenster zu sehen); ohne --auto von Hand
scripts/wsl-x11.py start                   # installiertes System starten (Desktop)
scripts/wsl-x11.py geraete                 # Kern ohne Fenster: Geräte, Treiber, Werkstatt
scripts/wsl-x11.py foto bild.png           # Bildschirmfoto der laufenden VM
```

Unter Windows: `Ultar-OS-WSL.cmd` (ruft `wsl.exe … scripts/wsl-x11.py start`).

| Datei | Inhalt |
|---|---|
| `~/ultar-wsl-vm/ultar-install.img` | Installationsmedium (ULTAR-INSTALL, ULTAR-EFI, ultar-compat) |
| `~/ultar-wsl-vm/ultar.img` | Platte der VM (16 GB, wächst nach Bedarf), NVMe |
| `~/ultar-wsl-vm/seriell.log` | serielle Konsole, wenn ohne Terminal gestartet |
| `~/ultar-wsl-vm/monitor.sock` | QEMU-Monitor (Bildschirmfotos, Tasten) |

Umgebung: `X11=127.0.0.1:0` (X-Server), `RES=1280x800`, `MEM=4096`, `SMP=4`, `PLATTE_GB=16`, `SPRACHE=ar|de|en`
(nur `--auto`), `ULTAR_VM=…`. Neu installieren: `installieren --auto --neu`.

## X-Server finden

Reihenfolge: `X11=…`, `127.0.0.1:0` (WSL mit gespiegeltem Netz), Standard-Router und DNS-Adresse der WSL (NAT-Netz),
`DISPLAY`, zuletzt `:0` (WSLg). GTK wird auf X11 festgelegt (`GDK_BACKEND=x11`), sonst nähme es Wayland von WSLg.

## Die Maschine und ihre Treiber

q35, UEFI (OVMF), 4 Kerne, 4 GB – jedes Gerät hat einen Treiber im Ultar-Kern (`docs/TREIBER.md`):
NVMe (`nvme`), Installationsmedium (`virtio-blk`), Bochs-Grafik (`gen-bochs-vga`), Intel e1000e (`gen-intel-eth`),
xHCI (`gen-xhci`), HD-Audio (`gen-hda`), SMBus (`gen-smbus`), AHCI (`gen-ahci`), `virtio-rng`, Brücken (`bruecke`),
PS/2-Tastatur, absolute Maus (vmmouse).

## Geschwindigkeit

Ohne Zugriff auf `/dev/kvm` rechnet QEMU alles nach (TCG) – der Kern startet trotzdem in ~0,6 s. Mit KVM deutlich
schneller: `sudo usermod -aG kvm $USER`, danach `wsl --shutdown` und neu öffnen. Der Ultar-Kern ist unter KVM
noch nicht geprüft.

## Stand

Geprüft (22.11.2026, WSL 2, QEMU 8.2.2, VcXsrv): Installation `--auto` (2 Partitionen kopiert und zurückgelesen, GPT),
Start von der Platte bis zum Desktop, Fenster „QEMU (Ultar OS)“ auf dem X-Server, alle Geräte mit Treiber.
Nicht geprüft: Ton bis zum Windows-Lautsprecher, KVM, Bedienung der Installation von Hand im Fenster.
