#!/usr/bin/env bash
# Power off and restart from a process (reboot(2), like ultar-init at its end) must end the virtual machine:
# ACPI S5 / ACPI reset first, the firmware's ResetSystem as fallback (forced here with the file
# ultar-kern/aus-firmware, 256 MiB RAM: the firmware's runtime code then lies in the process's user window).
# q35 with 1 and 4 processors, pc (PIIX4 ACPI), VMware-like (pc, NVMe, e1000e, SVGA). -no-reboot turns a
# restart into the end of QEMU. Never an exception screen. Verdict line: "AUSSCHALTEN: ALLES OK".
# IMG=<esp image> (default out/ultar-kern-esp.img, built by scripts/build-kern.sh with /bin/ausschalttest).
set -uo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-out/ultar-kern-esp.img}"
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
cp "$IMG" "$WORK/fw.img"
mmd -D s -i "$WORK/fw.img" ::/ultar-kern 2>/dev/null || true
echo 1 > "$WORK/aus-firmware"; mcopy -o -i "$WORK/fw.img" "$WORK/aus-firmware" ::/ultar-kern/aus-firmware
fail=0
fall() { # name what image env...
  local name=$1 was=$2 img=$3; shift 3
  local log="$WORK/$name-$was.log"
  env "$@" WAIT=60 timeout 300 python3 tools/kern-test/qtest.py "$img" "/bin/ausschalttest $was" > "$log" 2>&1
  if grep -aq "AUSSCHALTTEST: $was" "$log" && grep -aq "QEMU-ENDE: selbst" "$log" \
     && ! grep -aqE "AUSNAHME|KERNEL EXCEPTION|خطأ في النواة|kehrte zurück|PANIK|kernel panic" "$log"; then
    echo "ok    $was: $name"
  else
    echo "FAIL  $was: $name"; grep -aE "AUSSCHALT|kern\]|QEMU-ENDE|AUSNAHME|EXCEPTION|خطأ" "$log" | tail -12; fail=1
  fi
}
for was in aus neustart; do
  fall q35-smp1 "$was" "$IMG" MACHINE=q35 SMP=1
  fall q35-smp4 "$was" "$IMG" MACHINE=q35 SMP=4
  fall pc-piix4 "$was" "$IMG" MACHINE=pc SMP=2
  fall vmware-like-smp4 "$was" "$IMG" MACHINE=pc SMP=4 DEV="nvme,serial=ultar" NIC=e1000e "QARGS=-vga vmware"
  fall uefi-fallback-256m "$was" "$WORK/fw.img" MACHINE=q35 SMP=4 MEM=256
done
[ "$fail" = 0 ] && echo "AUSSCHALTEN: ALLES OK"
exit "$fail"
