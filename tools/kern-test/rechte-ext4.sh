#!/usr/bin/env bash
# File modes and owners on a writable ext4 (R3-K1): boots the kernel with a second disk that holds an
# ext4 labelled ultar-data (mounted writable as /data), runs `rechtetest --ext4 /data` (write, fsync,
# chmod 0640, chown 1000:100, mkdir 0750 for 2000:200; as user 1000 a private file written with 0600 and
# renamed, then chmod by its owner), stops the machine while the file is still open
# and checks on the host that data, modes and owners are on the disk and the file system is clean.
# Needs: mke2fs, debugfs, e2fsck (e2fsprogs), python3, the image from `IMG=1 scripts/build-kern.sh`.
set -euo pipefail
cd "$(dirname "$0")/../.."
D="$(mktemp)"; DISK="$(mktemp)"; L="$(mktemp)"; LOG="${ELOG:-$L}"; trap 'rm -f "$D" "$DISK" "$L"' EXIT
mke2fs -q -t ext4 "$D" 16M
# the kernel finds ultar-data by its GPT partition name: one partition at 1 MiB (sgdisk is not needed)
python3 - "$D" "$DISK" <<'PY'
import struct, sys, uuid, zlib
part, disk = open(sys.argv[1], "rb").read(), sys.argv[2]
first, n = 2048, len(part) // 512
last_usable = first + n - 1; total = last_usable + 1 + 33
ent = struct.pack("<16s16sQQQ72s", uuid.UUID("0fc63daf-8483-4772-8e79-3d69d8477de4").bytes_le, uuid.uuid4().bytes_le,
                  first, last_usable, 0, "ultar-data".encode("utf-16-le")).ljust(128, b"\0")
table = ent.ljust(128 * 128, b"\0")
def header(cur, alt, tbl):
    h = struct.pack("<8sIIIIQQQQ16sQIII", b"EFI PART", 0x10000, 92, 0, 0, cur, alt, 34, last_usable,
                    uuid.uuid4().bytes_le, tbl, 128, 128, zlib.crc32(table))
    return (h[:16] + struct.pack("<I", zlib.crc32(h)) + h[20:]).ljust(512, b"\0")
mbr = bytearray(512); mbr[446:462] = struct.pack("<BBBBBBBBII", 0, 0, 2, 0, 0xEE, 0xFF, 0xFF, 0xFF, 1, min(total - 1, 0xFFFFFFFF)); mbr[510:] = b"\x55\xaa"
with open(disk, "wb") as f:
    f.write(bytes(mbr) + header(1, total - 1, 2) + table)
    f.seek(first * 512); f.write(part)
    f.seek((total - 33) * 512); f.write(table + header(total - 1, 1, total - 33))
PY
QARGS="-drive file=$DISK,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP="${SMP:-2}" WAIT="${WAIT:-60}" \
  timeout 200 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "/bin/rechtetest --ext4 /data" > "$LOG" 2>&1 || true
fail=0
grep -aq "RECHTETEST-EXT4: GESCHRIEBEN" "$LOG" || { echo "FAIL  RECHTETEST-EXT4 (in the machine)"; grep -a "FEHLER" "$LOG"; fail=1; }
if grep -aqiE "kernel panic|PANIK" "$LOG"; then echo "FAIL  kernel panic"; fail=1; fi
# what the disk holds after the hard stop
dd if="$DISK" of="$D" bs=512 skip=2048 count=$(( $(stat -c %s "$D") / 512 )) conv=notrunc status=none
st() { debugfs -R "stat $1" "$D" 2>/dev/null | tr '\n' ' '; }
f="$(st /rt-dauer)"; s="$(st /rt-ordner)"; k="$(st /rt-eigen/schluessel)"
# a private file written as .name.new (0600) and renamed keeps mode and owner (here 0400 after its owner's chmod)
[[ "$k" =~ Mode:\ +0400 && "$k" =~ User:\ +1000\  && "$k" =~ Group:\ +100\  ]] || { echo "FAIL  renamed private file on disk: ${k:0:160}"; fail=1; }
[[ "$f" =~ Mode:\ +0640 && "$f" =~ User:\ +1000\  && "$f" =~ Group:\ +100\  && "$f" =~ Size:\ +8192 ]] || { echo "FAIL  file on disk: ${f:0:160}"; fail=1; }
[[ "$s" =~ Type:\ directory && "$s" =~ Mode:\ +0750 && "$s" =~ User:\ +2000\  && "$s" =~ Group:\ +200\  ]] || { echo "FAIL  directory on disk: ${s:0:160}"; fail=1; }
debugfs -R "cat /rt-dauer" "$D" 2>/dev/null | cmp -s - <(python3 -c "import sys; sys.stdout.write(''.join(chr(65 + i % 23) for i in range(8192)))") || { echo "FAIL  file content on disk"; fail=1; }
e2fsck -fn "$D" >/dev/null 2>&1 || { echo "FAIL  e2fsck after the hard stop"; e2fsck -fn "$D" 2>&1 | tail -8; fail=1; }
[ "$fail" = 0 ] || { tail -20 "$LOG"; exit 1; }
echo "ok    RECHTETEST-EXT4: ALLES OK"
