#!/usr/bin/env bash
# Durable writes on ext4 (kernel ext4 writer): boots the kernel with a second disk holding an ext4 named
# ultar-data (/data), runs `ext4test write` (free space fragmented, a 64 MiB and an 18 MiB file with fsync,
# a file larger than the free space must fail with ENOSPC), stops the machine hard, boots again,
# runs `ext4test verify` (both files byte for byte), and checks on the host with e2fsck and debugfs.
# EXT4_BLOCK=<bytes>: block size of the test file system (default: what mke2fs chooses for 160 MiB, 1 KiB).
# Needs: mke2fs, debugfs, e2fsck, python3, the image from `IMG=1 scripts/build-kern.sh`.
set -euo pipefail
cd "$(dirname "$0")/../.."
D="$(mktemp)"; DISK="$(mktemp)"; L1="$(mktemp)"; L2="$(mktemp)"; trap 'rm -f "$D" "$DISK" "$L1" "$L2"' EXIT
# an indexed (HTree) directory of 600 entries: mkdir inside it must reach the disk or fail, never stay in memory
SRC="$(mktemp -d)"; mkdir "$SRC/bigdir"; for i in $(seq 1 600); do : > "$SRC/bigdir/eintrag-mit-langem-namen-$i"; done
mke2fs -q -t ext4 ${EXT4_BLOCK:+-b "$EXT4_BLOCK"} -d "$SRC" "$D" 160M
rm -rf "$SRC"
e2fsck -fyD "$D" >/dev/null 2>&1 || true
debugfs -R "htree_dump /bigdir" "$D" 2>&1 | grep -q "Root node dump" || echo "note  /bigdir carries no HTree index"
python3 tools/kern-test/gptimg.py "$DISK" "ultar-data:linux:$D"
run() { # log, command
  QARGS="-drive file=$DISK,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP="${SMP:-2}" MEM=1536 WAIT="${WAIT:-600}" \
    timeout 900 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "$2" > "$1" 2>&1 || true
}
fail=0
run "$L1" "/bin/ext4test write /data"
grep -aq "EXT4TEST: GESCHRIEBEN" "$L1" || { echo "FAIL  EXT4TEST (write)"; grep -a "FEHLER\|ok  " "$L1" | tail -12; fail=1; }
run "$L2" "/bin/ext4test verify /data"
grep -aq "EXT4TEST: ALLES OK" "$L2" || { echo "FAIL  EXT4TEST (verify after the hard stop)"; grep -a "FEHLER\|ok  " "$L2" | tail -6; fail=1; }
cat "$L1" "$L2" | grep -aqiE "kernel panic|PANIK" && { echo "FAIL  kernel panic"; fail=1; }
dd if="$DISK" of="$D" bs=512 skip=2048 count=$(( $(stat -c %s "$D") / 512 )) conv=notrunc status=none
e2fsck -fn "$D" >/dev/null 2>&1 || { echo "FAIL  e2fsck after the hard stop"; e2fsck -fn "$D" 2>&1 | tail -12; fail=1; }
depth=$(debugfs -R "stat /big64" "$D" 2>/dev/null | grep -c "(ETB" || true)
[ "$depth" -ge 1 ] || { echo "FAIL  /big64 has no extent tree (depth 0)"; fail=1; }
debugfs -R "stat /too-big" "$D" 2>/dev/null | grep -q "Size: 62914560" && { echo "FAIL  the refused file is on disk with its full size"; fail=1; }
[ "$fail" = 0 ] || { tail -20 "$L1"; exit 1; }
echo "ok    EXT4TEST: ALLES OK (64 MiB + 18 MiB through a hard stop, extent tree, ENOSPC honest, e2fsck clean)"
