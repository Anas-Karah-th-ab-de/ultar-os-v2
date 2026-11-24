#!/usr/bin/env bash
# Startet Ultar OS in QEMU (x86_64).
#   KERNEL=/pfad/vmlinuz scripts/run-qemu.sh            Oberfläche im Fenster (Maus = Touch)
#   KERNEL=… SELFTEST=1 scripts/run-qemu.sh             Selbsttest auf der Konsole, schaltet danach ab
#   Für die Oberfläche mit Ubuntu-Kernel vorher bauen mit:
#     MODULES="virtio-gpu virtio_input" MODULES_DIR=… scripts/build-initramfs.sh
set -euo pipefail
cd "$(dirname "$0")/.."
KERNEL=${KERNEL:?KERNEL=/pfad/vmlinuz setzen}
ACCEL=tcg; [ -w /dev/kvm ] && ACCEL=kvm
[ -f out/data.ext4 ] || { truncate -s 64M out/data.ext4; mkfs.ext4 -q -L ultar-data out/data.ext4; }
APPEND="console=ttyS0 quiet loglevel=3 rdinit=/sbin/init panic=-1"
GFX=(-display "${DISPLAY_MODE:-gtk}" -vga none -device virtio-gpu-pci,xres=${XRES:-720},yres=${YRES:-1280} -device virtio-tablet-pci -device virtio-keyboard-pci -serial stdio)
[ -n "${ULTAR_START:-}" ] && APPEND="$APPEND ultar.start=$ULTAR_START"
if [ "${SELFTEST:-0}" = 1 ]; then
  APPEND="$APPEND ultar.start=heartbeat,selftest"
  GFX=(-nographic)
fi
# Name der Maschine (Fenstertitel, Prozessname): aus der einen Quelle der Marke (docs/BRAND.md)
VMNAME=$(. rootfs/etc/os-release; echo "$NAME")
exec qemu-system-x86_64 -name "$VMNAME" -accel $ACCEL -m ${MEM:-512} -smp 2 -no-reboot \
  -kernel "$KERNEL" -initrd out/ultar-initramfs-x86_64.cpio.zst \
  -drive file=out/compat-amd64.ext4,if=virtio,format=raw \
  -drive file=out/data.ext4,if=virtio,format=raw \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -append "$APPEND" "${GFX[@]}" "$@"
