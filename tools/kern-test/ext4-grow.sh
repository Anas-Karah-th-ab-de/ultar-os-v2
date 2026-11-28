#!/usr/bin/env bash
# A grown data file system in the Ultar kernel: mke2fs makes a 32 MiB ext4 with only 256 inodes,
# ultar-ext4grow grows it to 1 GiB (new groups BLOCK_UNINIT/INODE_UNINIT, descriptors and resize
# inode rewritten), the kernel mounts it as /data and `growtest` writes 400 files and a 60 MiB
# file into the new groups. Afterwards e2fsck must be clean and the files complete.
# Needs: mke2fs, debugfs, e2fsck, python3, the image from `IMG=1 scripts/build-kern.sh`.
set -euo pipefail
cd "$(dirname "$0")/../.."
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
cargo build --release -q -p ultar-ext4grow
mkdir "$W/tree"; echo "ultar grow probe" > "$W/tree/probe.txt"
mke2fs -q -t ext4 -N 256 -d "$W/tree" "$W/fs.img" 32M
truncate -s 1G "$W/fs.img"
target/release/ultar-ext4grow "$W/fs.img" | grep -q "^grown" || { echo "FAIL  ultar-ext4grow did not grow"; exit 1; }
e2fsck -fn "$W/fs.img" >/dev/null 2>&1 || { echo "FAIL  e2fsck after the growth"; exit 1; }
python3 tools/kern-test/gptimg.py "$W/disk.img" "ultar-data:linux:$W/fs.img"
QARGS="-drive file=$W/disk.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP=2 MEM=1024 WAIT=600 \
  timeout 900 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "/bin/growtest /data" > "$W/log" 2>&1 || true
fail=0
grep -aq "GROWTEST: ALLES OK" "$W/log" || { echo "FAIL  GROWTEST"; grep -a "FEHLER\|ok  " "$W/log" | tail -8; fail=1; }
grep -aqiE "kernel panic|PANIK" "$W/log" && { echo "FAIL  kernel panic"; fail=1; }
dd if="$W/disk.img" of="$W/fs.img" bs=512 skip=2048 count=$(( $(stat -c %s "$W/fs.img") / 512 )) conv=notrunc status=none
e2fsck -fn "$W/fs.img" >/dev/null 2>&1 || { echo "FAIL  e2fsck after the kernel wrote"; e2fsck -fn "$W/fs.img" 2>&1 | tail -12; fail=1; }
debugfs -R "stat /gross" "$W/fs.img" 2>/dev/null | grep -q "Size: 62914560" || { echo "FAIL  /gross incomplete"; fail=1; }
debugfs -R "cat /n399" "$W/fs.img" 2>/dev/null | grep -q "datei 399" || { echo "FAIL  /n399 missing"; fail=1; }
[ "$fail" = 0 ] || { tail -20 "$W/log"; exit 1; }
echo "ok    GROWTEST: ALLES OK (grown ext4: inodes and blocks from new groups, e2fsck clean)"
