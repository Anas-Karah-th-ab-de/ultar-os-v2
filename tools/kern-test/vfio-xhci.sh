#!/bin/bash
# Ultar-Kern mit dem echten PCH-xHCI (00:14.0, IOMMU-Gruppe mit 00:14.2) per VFIO prüfen, danach an Linux zurück.
# An diesem Controller hängen nur Bluetooth, Fingerabdruck, USB-Audio und die ASMedia-Hubs – nichts für den Serverbetrieb.
IMG=/root/ultar-kern-esp.img
LOG=/root/ultar-vfio-usb.log
modprobe vfio-pci
for D in 0000:00:14.2 0000:00:14.0; do
  echo vfio-pci > /sys/bus/pci/devices/$D/driver_override
  [ -e /sys/bus/pci/devices/$D/driver ] && echo $D > /sys/bus/pci/devices/$D/driver/unbind
  echo $D > /sys/bus/pci/drivers_probe
  echo "$D -> $(readlink /sys/bus/pci/devices/$D/driver)"
done
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/uv.fd
timeout ${DAUER:-40} qemu-system-x86_64 -enable-kvm -machine q35 -m 2048 -smp 4 -cpu host -display none -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive if=pflash,format=raw,file=/tmp/uv.fd \
  -drive file=$IMG,if=none,id=b,format=raw -device virtio-blk-pci,drive=b \
  -device vfio-pci,host=0000:00:14.0 -serial file:$LOG -monitor none -vga std
for D in 0000:00:14.0 0000:00:14.2; do
  echo > /sys/bus/pci/devices/$D/driver_override
  echo $D > /sys/bus/pci/devices/$D/driver/unbind
  echo $D > /sys/bus/pci/drivers_probe
done
sleep 3
for D in 0000:00:14.0 0000:00:14.2; do echo "$D zurück: $(readlink /sys/bus/pci/devices/$D/driver)"; done
lsusb | wc -l
echo ---- LOG
grep -aE 'xhci|usb|treiber.*00:0|Fehler|FEHLER|panic|bereit' $LOG | head -40
