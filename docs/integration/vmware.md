# Ultar OS v2 in VMware Workstation – integration notes

`tools/vm/make-vmware.sh` builds a VMware Workstation machine from the tree, `tools/vm/install-vmware.ps1`
puts it on the Windows PC and starts it, `tools/kern-test/vmware-like.sh` is the kernel gate on the same kind
of hardware. **QEMU is the test bed for all of it; VMware Workstation itself was not started by these tools.**

## 1. What the kernel has for VMware's virtual hardware

| VMware device | Kernel | Checked in QEMU with |
|---|---|---|
| UEFI firmware, GOP frame buffer | `efi.rs`, `grafik::init_gop`, `/dev/fb0` | OVMF, `-vga std` and `-vga vmware` |
| VMware SVGA II (15ad:0405) | **no driver**: the firmware's GOP picture and resolution stay (no mode change, no acceleration) | `-vga vmware` |
| NVMe (`nvme0`) | `drivers/nvme.rs` (polling) | `-device nvme` (start disk, installer target) |
| SATA/AHCI (`sata0`) | `drivers/gen_ahci.rs` (polling; disks only, no CD/DVD) | q35 AHCI (start disk, installation medium) |
| LSI Logic SAS/parallel, BusLogic, PVSCSI, IDE (PIIX4) | **no driver** | – |
| e1000 / e1000e | `drivers/gen_intel_eth.rs` (8086:100e, 10d3, …) | `e1000`, `e1000e` |
| vmxnet3 | **no driver** | – |
| xHCI (`usb_xhci`) | `drivers/gen_xhci.rs`: keyboards, mice, absolute tablets (HID report descriptor), hot-plug; no hubs, no storage | `qemu-xhci` + `usb-kbd`, `usb-tablet` |
| UHCI/EHCI (`usb`, `ehci`) | **no driver**: what VMware attaches there is not seen | – |
| PS/2 keyboard and mouse (i8042), VMware absolute pointer (backdoor) | `drivers/ps2kbd.rs`, `drivers/ps2maus.rs` | i8042, `vmmouse` of QEMU |
| HD audio (`hdaudio`) | `drivers/gen_hda.rs`; the machine is built without sound | – |
| VMCI, PCI bridges, PCIe root ports | bridges: `drivers/bruecke.rs`; VMCI is turned off in the `.vmx` | PCIe root port in `ci.sh` |

PCI is read through the ports `0xCF8/0xCFC` over all 256 buses, so devices behind VMware's PCIe root ports are
found without the PCIe configuration space. Nothing essential is missing for a start: no driver was added.
In VMware the kernel line "every device has a driver" will not appear (SVGA II, PIIX4 IDE, UHCI/EHCI are
listed as driverless) – that is expected and does not stop anything.

## 2. Kernel gate `tools/kern-test/vmware-like.sh`

The programs and markers of the first boot of `ci.sh`, without any virtio device (a bound virtio driver fails
the run): `q35-ahci-usb` (AHCI start disk, e1000e, xHCI keyboard + tablet), `q35-nvme-ps2` (NVMe, e1000 as
the only network card – here geraetetest runs its bind cycle –, i8042 input only), `pc-nvme-svga`
(i440FX/PIIX, NVMe, e1000e, xHCI, VMware SVGA II: only the IDE controller and the display may be driverless
and the GOP picture must be there). `REPEAT=3`, `RUNS=…`, `VL_LOG=<prefix>`; verdict `VMWARE-LIKE: ALLES OK`.
Two changes made it possible: `qtest.py` takes `MACHINE`, and `fenstertest` no longer assumes that no USB
input device is plugged in at the start (it then opens `event2` instead of expecting it to be absent).

## 3. Session start by the kernel: `/etc/ultar/session`

New, in `shell::init_starten` (called from `main.rs` on an installed system): when the system partition holds
`/etc/ultar/session` with the word `init`, the kernel starts `/sbin/ultar-init` (else `/usr/bin/ultar-init`)
as PID 1 with the system language in `ULTAR_LANG`, instead of `ultar-shell`. Without the file, with another
word, or when the program is missing or does not start, everything is as before (`ultar-shell`, then the
start page). No existing medium has the file.

## 4. The machine

`make-vmware.sh` → `out/vmware/UltarOS-v2/{UltarOS-v2.vmx, UltarOS-v2.vmdk}`, `out/vmware/UltarOS-v2.zip`
(+ `.sha256`), logs and screenshots `out/vmware/UltarOS-v2-*.{log,ppm}` (`.png` with Pillow).

* Disk: written by the kernel's own installer (medium on AHCI → empty 4 GiB disk on NVMe, in QEMU):
  `ULTAR-EFI` (loader), `ULTAR-BOOTCTL`, `ultar-kernel-a/b`, `ultar-system-a/b`, `ultar-data`; then
  `qemu-img convert -O vmdk` (monolithicSparse).
* System: `/sbin/ultar-init`, the shipped `init.toml`, `ids.toml`, `bus-policy.toml`, rights and seat files,
  the shipped service files `logd`, `bus`, `settingsd`, `devd`, `seat`, and `compositor` without its line
  `manual = true` (this image has no `ultar-shell` that would drive the same display); plus two windows of
  `ultar-compositor-testclient` (`tools/vm/services/`, image only, as user nobody). `/bin/busybox` when the
  build computer has a static one. Language: `LANG_CODE` (default `ar`) on `ULTAR-EFI` and in the system.
* Start: firmware → `ultar-loader` (slot A) → kernel (start picture in Arabic) → `ultar-init` (PID 1) → logd,
  seat, bus, compositor, devd, settingsd → compositor on `fb0` with the two windows (title bar mirrored for
  Arabic). There is no desktop shell on the compositor yet, so that is all there is to see; `ultar-shell`
  (the old surface) is not in this image.
* `.vmx`: hardware version 16, `firmware = "efi"`, 4 GiB, 4 processors, NVMe disk, e1000e (NAT), UHCI/EHCI +
  xHCI with VMware's USB pointer on the xHCI, PS/2 keyboard, no sound, no VMCI, `serial0` → `serial.log`
  beside the `.vmx` (the kernel's whole log – the first thing to read when the screen stays black).
* Checks (`tools/vm/vmware-vm.py check`, each on a copy-on-write layer, the `.vmdk` is delivered unstarted):
  the raw disk on NVMe, the `.vmdk` on NVMe with the VMware SVGA II adapter, the `.vmdk` on AHCI. Verdict
  `VMWARE-CHECK: OK` needs the `[boot] slot` line of the loader, `ready` of the six services, `start` of
  both windows, `settled`, no refused or ended service, no panic, no virtio device, and at least 1 000 pixels
  of each window colour on the screenshot.

## 5. Not checked, and what to try

* VMware Workstation itself: firmware, processor features, timer calibration, start of the further
  processors, VMware's NVMe/AHCI/xHCI models and its USB pointer were never run with this kernel.
* Screen stays black or the machine restarts: read `serial.log`. Disk not found: `install-vmware.ps1 -Disk
  sata`. Pointer does not move: `install-vmware.ps1 -Ps2Mouse` (then click into the window; Ctrl+Alt frees
  the pointer).
* Input in the compositor was not driven in the check (only that the devices get their event nodes); network
  only as far as the kernel's DHCP line.
