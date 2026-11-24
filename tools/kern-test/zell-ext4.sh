#!/usr/bin/env bash
# Escape test for symbolic links on ext4 (isolation stage 1): boots the kernel with a second disk that
# holds a plain ext4 file system (becomes "/", read-only) and runs `zelltest --ext4` from the FAT disk.
# Needs: mke2fs (e2fsprogs), the image from `IMG=1 scripts/build-kern.sh`.
set -euo pipefail
cd "$(dirname "$0")/../.."
R="$(mktemp -d)"; LOG="$(mktemp)"; trap 'rm -rf "$R"' EXIT
mkdir -p "$R/etc" "$R/zelle" "$R/tmp"
printf 'GEHEIM-EXT4\n' > "$R/etc/geheim"
printf 'drinnen\n' > "$R/zelle/drin.txt"
ln -s /drin.txt "$R/zelle/innen"
ln -s /etc/geheim "$R/zelle/flucht"
ln -s ../etc/geheim "$R/zelle/flucht2"
ln -s /etc "$R/zelle/ordner"
rm -f out/zell-ext4.img
mke2fs -q -t ext4 -d "$R" out/zell-ext4.img 16M
QARGS="-drive file=out/zell-ext4.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP="${SMP:-2}" \
  timeout 200 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "/boot/efi/bin/zelltest --ext4" > "$LOG" 2>&1 || true
if grep -q "ZELLTEST-EXT4: ALLES OK" "$LOG" && ! grep -qiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    ZELLTEST-EXT4: ALLES OK"
else
  echo "FAIL  ZELLTEST-EXT4"; tail -40 "$LOG"; exit 1
fi
[ "${VERBOSE:-0}" = 1 ] && sed -n '/ZELLTEST: ext4/,/ZELLTEST-EXT4/p' "$LOG"
exit 0
