#!/usr/bin/env bash
# Intel NPU driver test on the Beelink GTi15 Ultra without touching its boot: the NPU (8086:7d1d,
# usually 00:0b.0) is taken from Linux' intel_vpu, passed to the Ultar kernel under KVM with VFIO,
# and ALWAYS given back when the script ends (also on errors and Ctrl-C). The kernel runs its
# self test; its verdict line is printed at the end (NPUTEST: ALLES OK).
#
# Usage (on the target, as root):  npu-passthrough.sh <ultar-kern.efi>
#   LIMIT=<seconds>  hard time limit of the VM (default 180)
#   OPTIN=0          boot without the driver's opt-in: it must only read (verdict READ-ONLY OK)
# Needs: qemu-system-x86_64 (KVM), OVMF 4M, mtools, dosfstools.
#
# What it does NOT do: stop any service, change the boot, or touch other PCI devices. It refuses to
# start when a process holds the NPU's accel node (an inference job is running).
# Not run on hardware yet (written with the driver, stage N0): docs/integration/intel-npu.md.
set -euo pipefail
EFI="${1:?usage: npu-passthrough.sh <ultar-kern.efi>}"
[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }
[ -r "$EFI" ] || { echo "kernel image $EFI missing"; exit 1; }
for c in qemu-system-x86_64 mcopy mkfs.vfat; do command -v "$c" >/dev/null || { echo "$c missing"; exit 1; }; done
[ -e /dev/kvm ] || { echo "/dev/kvm missing"; exit 1; }
DEV=""
for d in /sys/bus/pci/devices/*; do
  [ "$(cat "$d/vendor")" = 0x8086 ] && [ "$(cat "$d/device")" = 0x7d1d ] && { DEV=$(basename "$d"); break; }
done
[ -n "$DEV" ] || { echo "no Intel NPU 8086:7d1d on this machine"; exit 1; }
SYS=/sys/bus/pci/devices/$DEV
W="$(mktemp -d /tmp/npu-pt.XXXXXX)"
others=$(ls "$SYS/iommu_group/devices" | grep -v "^$DEV$" || true)
[ -z "$others" ] || { echo "IOMMU group of $DEV has other devices ($others) - refusing"; rm -rf "$W"; exit 1; }
drv_of() { basename "$(readlink -f "$SYS/driver" 2>/dev/null)" 2>/dev/null || echo none; }
ORIG=$(drv_of)
echo "NPU $DEV, driver before: $ORIG"
for n in "$SYS"/accel/accel*; do
  [ -e "$n" ] || continue
  node=/dev/accel/$(basename "$n")
  for fd in /proc/[0-9]*/fd/*; do
    [ "$(readlink "$fd" 2>/dev/null)" = "$node" ] && { echo "a process uses $node (pid $(echo "$fd" | cut -d/ -f3)) - nothing was changed"; rm -rf "$W"; exit 1; }
  done
done
N0=$(dmesg | wc -l)

give_back() {
  set +e
  pkill -f "qemu-system-x86_64.*$DEV" 2>/dev/null
  sleep 1
  if [ "$(drv_of)" != "$ORIG" ]; then
    [ "$(drv_of)" = none ] || echo "$DEV" > "$SYS/driver/unbind"
    echo > "$SYS/driver_override"
    if [ "$ORIG" != none ]; then
      echo "$DEV" > /sys/bus/pci/drivers_probe
      sleep 2
      [ "$(drv_of)" = "$ORIG" ] || { modprobe "$ORIG"; echo "$DEV" > "/sys/bus/pci/drivers/$ORIG/bind" 2>/dev/null; sleep 2; }
    fi
  fi
  echo "driver after the test: $(drv_of) (was $ORIG)"
  dmesg | tail -n +"$((N0 + 1))" | grep -iE "intel_vpu|ivpu" | tail -8
  rm -rf "$W"
}
trap give_back EXIT INT TERM

truncate -s 64M "$W/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$W/esp.img" >/dev/null
mmd -i "$W/esp.img" ::/EFI ::/EFI/BOOT ::/bin ::/ultar-kern ::/ultar
mcopy -i "$W/esp.img" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
if [ "${OPTIN:-1}" != 0 ]; then
  printf 'on\n' > "$W/intel-npu"; mcopy -i "$W/esp.img" "$W/intel-npu" ::/ultar/intel-npu
fi
head -c 262144 /dev/zero | tr '\0' ' ' > "$W/log.txt"; mcopy -i "$W/esp.img" "$W/log.txt" ::/ultar-kern/selbsttest.txt
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd"

modprobe vfio-pci
[ "$ORIG" = none ] || echo "$DEV" > "$SYS/driver/unbind"
echo vfio-pci > "$SYS/driver_override"
echo "$DEV" > /sys/bus/pci/drivers_probe
[ "$(drv_of)" = vfio-pci ] || { echo "vfio-pci did not take $DEV"; exit 1; }

: > "$W/serial.log"
timeout "${LIMIT:-180}" qemu-system-x86_64 -enable-kvm -machine q35 -cpu host -smp 4 -m 2048 \
  -display none -vga none -nodefaults -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive "if=pflash,format=raw,file=$W/vars.fd" \
  -drive "file=$W/esp.img,if=none,id=b,format=raw" -device virtio-blk-pci,drive=b,addr=05.0 \
  -device "vfio-pci,host=$DEV,id=npu,addr=0b.0" \
  -serial "file:$W/serial.log" -monitor none 2> "$W/qemu.err" &
qp=$!
while kill -0 "$qp" 2>/dev/null; do
  if grep -aqE "NPUTEST: (ALLES OK|FEHLER|no Intel)|AUSNAHME|PANIK" "$W/serial.log" 2>/dev/null; then sleep 2; kill "$qp" 2>/dev/null; break; fi
  sleep 1
done
wait "$qp" 2>/dev/null || true
echo "--- QEMU: $(head -3 "$W/qemu.err" | tr '\n' ' ')"
echo "--- kernel (intel-npu, NPUTEST)"
grep -aE "intel-npu|NPUTEST|treiber\] 00:0b|panic|PANIK|Ausnahme" "$W/serial.log" | head -60 || true
if [ "${OPTIN:-1}" = 0 ]; then
  if grep -aq "not driven, read only" "$W/serial.log" && grep -aq "NPUTEST: no Intel NPU bound" "$W/serial.log"; then
    echo "VERDICT: NPUTEST: READ-ONLY OK"
  else
    echo "VERDICT: read-only boot not confirmed"; tail -30 "$W/serial.log"
  fi
elif grep -aq "NPUTEST: ALLES OK" "$W/serial.log"; then
  echo "VERDICT: NPUTEST: ALLES OK"
else
  echo "VERDICT: $(grep -aE 'NPUTEST: (FEHLER|no Intel)' "$W/serial.log" | tail -1 || echo 'no verdict (see serial tail below)')"
  echo "--- serial tail"; tail -30 "$W/serial.log"
fi
