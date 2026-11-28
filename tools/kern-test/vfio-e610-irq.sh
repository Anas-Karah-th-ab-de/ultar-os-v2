#!/bin/bash
# Wie vfio-e610.sh, aber nach dem Boot (Interrupts an) den Befehl `netz` an die Kern-Shell schicken
# und so die MSI-X-Zustellung belegen: die während des Treiber-Starts (IF=0) im LAPIC anstehende
# Unterbrechung des internen Schleifentests wird nach `sti` zugestellt → `netz` zeigt den Zähler.
# Der Port mit der Server-Verbindung (ad:00.1) wird NICHT angefasst.
set -u
DEV=0000:ad:00.0
IMG=/root/ultar-kern-esp.img
LOG=/root/ultar-vfio-irq.log
if [ "$(cat /sys/bus/pci/devices/$DEV/net/*/operstate 2>/dev/null)" = up ]; then
  echo "Port $DEV ist in Benutzung – Abbruch"; exit 1
fi
modprobe vfio-pci
echo vfio-pci > /sys/bus/pci/devices/$DEV/driver_override
echo $DEV > /sys/bus/pci/devices/$DEV/driver/unbind 2>/dev/null
echo $DEV > /sys/bus/pci/drivers_probe
echo "vfio: $(readlink /sys/bus/pci/devices/$DEV/driver)"
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/uv.fd
# Eingabe: 22 s warten (Boot), dann `netz`, kurz warten, noch einmal, dann schließen.
( sleep 22; printf 'netz\n'; sleep 3; printf 'netz\n'; sleep 3 ) | \
  timeout ${DAUER:-40} qemu-system-x86_64 -enable-kvm -machine q35 -m 2048 -smp 4 -cpu host -display none -no-reboot \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file=/tmp/uv.fd \
    -drive file=$IMG,if=none,id=b,format=raw -device virtio-blk-pci,drive=b \
    -device vfio-pci,host=$DEV -serial stdio -monitor none -nodefaults -vga std > $LOG 2>&1
# zurück an Linux
echo > /sys/bus/pci/devices/$DEV/driver_override
echo $DEV > /sys/bus/pci/devices/$DEV/driver/unbind 2>/dev/null
echo $DEV > /sys/bus/pci/drivers_probe
sleep 2
echo "Treiber danach: $(readlink /sys/bus/pci/devices/$DEV/driver)"
echo ---- LOG
cat -v $LOG
