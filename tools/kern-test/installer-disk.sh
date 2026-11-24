#!/usr/bin/env bash
# Installer mode of the Ultar kernel (rawpart.rs): whole-disk nodes for the graphical installer.
#   1. normal start with two disks: no /dev/ultar/disk, no /dev/ultar/medium (installtest normal)
#   2. start from an installation medium (GPT, partition ULTAR-INSTALL with the marker \ultar-installer) with an
#      empty second disk, VMware-like (q35, AHCI, no virtio disk): exactly one target, the medium is the boot disk
#      and never a target, only the raw-device right plus the installer's user or group (840) opens the nodes,
#      the medium is read only (installtest installer 1)
# Verdict line: "INSTALLER-DISK: ALLES OK". IMG=<esp image> (else scripts/build-kern.sh builds one).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
cp "$IMG" "$W/plain.img"
mcopy -o -i "$W/plain.img" kernel/ultar-kern/user/installtest ::/bin/installtest
truncate -s 128M "$W/second.img"
fail=0
SATA="-device ahci,id=ahci2 -drive file=$W/second.img,if=none,id=d2,format=raw -device ide-hd,drive=d2,bus=ahci2.0"
LOG="$W/normal.log"
MACHINE=q35 DEV="ide-hd,bus=ide.0" NIC=e1000e QARGS="$SATA" SMP="${SMP:-2}" WAIT=60 timeout 300 \
  python3 tools/kern-test/qtest.py "$W/plain.img" "/bin/installtest normal" >"$LOG" 2>&1 || true
if grep -aq "INSTALLTEST: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    INSTALLER-DISK normal start: no installer nodes"
else
  echo "FAIL  INSTALLER-DISK normal start"; { grep -aE "FEHLER|INSTALLTEST|installtest" "$LOG" || true; } | tail -20; fail=1
fi
# the medium: the test image as partition ULTAR-INSTALL, with the installer marker
cp "$W/plain.img" "$W/install.fat"
printf 'Ultar OS installation medium (installer-disk test)\nsession=console\n' >"$W/marker"
mcopy -o -i "$W/install.fat" "$W/marker" ::/ultar-installer
python3 tools/kern-test/gptimg.py "$W/medium.img" "ULTAR-INSTALL:esp:$W/install.fat" >/dev/null
rm -f "$W/install.fat"
truncate -s 128M "$W/second.img"
LOG="$W/installer.log"
MACHINE=q35 DEV="ide-hd,bus=ide.0" NIC=e1000e QARGS="$SATA" SMP="${SMP:-2}" WAIT=60 timeout 300 \
  python3 tools/kern-test/qtest.py "$W/medium.img" "/bin/installtest installer 1" >"$LOG" 2>&1 || true
if grep -aq "INSTALLTEST: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    INSTALLER-DISK installer mode: $(grep -ac '^ok ' "$LOG") checks"
else
  echo "FAIL  INSTALLER-DISK installer mode"; { grep -aE "FEHLER|INSTALLTEST|\[installer\]" "$LOG" || true; } | tail -20; tail -5 "$LOG"; fail=1
fi
[ -n "${KEEP_LOG:-}" ] && cp "$W"/*.log "$KEEP_LOG"/ 2>/dev/null
[ "$fail" = 0 ] && echo "INSTALLER-DISK: ALLES OK" || { echo "INSTALLER-DISK: FEHLER"; exit 1; }
