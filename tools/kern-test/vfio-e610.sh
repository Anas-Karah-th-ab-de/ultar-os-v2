#!/bin/bash
# Ultar-Kern mit echtem E610-Port (ad:00.0, frei) per VFIO in QEMU/KVM prüfen; danach Port an Linux zurückgeben.
# Der Port mit der Server-Verbindung (ad:00.1) wird NICHT angefasst.
DEV=0000:ad:00.0
IMG=/root/ultar-kern-esp.img
LOG=/root/ultar-vfio.log
[ "$(cat /sys/bus/pci/devices/$DEV/net/*/operstate 2>/dev/null)" = up ] && { echo "Port $DEV ist in Benutzung – Abbruch"; exit 1; }
modprobe vfio-pci
echo vfio-pci > /sys/bus/pci/devices/$DEV/driver_override
echo $DEV > /sys/bus/pci/devices/$DEV/driver/unbind
echo $DEV > /sys/bus/pci/drivers_probe
readlink /sys/bus/pci/devices/$DEV/driver
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/uv.fd
timeout ${DAUER:-45} qemu-system-x86_64 -enable-kvm -machine q35 -m 2048 -smp 4 -cpu host -display none -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive if=pflash,format=raw,file=/tmp/uv.fd \
  -drive file=$IMG,if=none,id=b,format=raw -device virtio-blk-pci,drive=b \
  -device vfio-pci,host=$DEV -serial file:$LOG -monitor none -nodefaults -vga std
# zurück an Linux
echo > /sys/bus/pci/devices/$DEV/driver_override
echo $DEV > /sys/bus/pci/devices/$DEV/driver/unbind
echo $DEV > /sys/bus/pci/drivers_probe
sleep 2
echo "Treiber danach: $(readlink /sys/bus/pci/devices/$DEV/driver)"
ip -br link | grep enp173
echo ---- LOG
grep -aE 'treiber|ixgbe|pci|smp|apic|Fehler|FEHLER|panic|bereit' $LOG | head -40
