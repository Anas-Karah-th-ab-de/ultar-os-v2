#!/usr/bin/env bash
# Encrypted data partition in the Ultar kernel (crypt.rs): cryptsetup formats a LUKS2 volume with a
# known master key and an ext4 inside (the host is the reference), the kernel boots with it as
# ultar-data, `crypttest` unlocks it with that key, reads and writes /data. Afterwards the host opens
# the partition with cryptsetup again: e2fsck clean, the file the kernel wrote is there – so the
# kernel's aes-xts-plain64 matches dm-crypt in both directions.
# Needs: cryptsetup, sudo, mke2fs, debugfs, e2fsck, python3, the image from `IMG=1 scripts/build-kern.sh`.
set -euo pipefail
cd "$(dirname "$0")/../.."
W="$(mktemp -d)"
trap 'sudo -n cryptsetup close ultar-crypt-t 2>/dev/null || true; rm -rf "$W"' EXIT
PASS="kern-test-passwort"
head -c 64 /dev/urandom > "$W/key"
HEX=$(od -An -tx1 -v "$W/key" | tr -d ' \n')
mkdir "$W/tree"; echo "ultar luks probe" > "$W/tree/probe.txt"
mke2fs -q -t ext4 -d "$W/tree" "$W/plain.img" 48M
truncate -s 64M "$W/luks.img"
printf '%s' "$PASS" | cryptsetup luksFormat --batch-mode --type luks2 --cipher aes-xts-plain64 --key-size 512 \
  --sector-size 512 --pbkdf argon2id --pbkdf-memory 1024 --pbkdf-force-iterations 4 \
  --volume-key-file "$W/key" --key-file=- "$W/luks.img"
printf '%s' "$PASS" | sudo -n cryptsetup open --key-file=- "$W/luks.img" ultar-crypt-t
sudo -n dd if="$W/plain.img" of=/dev/mapper/ultar-crypt-t bs=1M conv=fsync status=none
sudo -n cryptsetup close ultar-crypt-t
python3 tools/kern-test/gptimg.py "$W/disk.img" "ultar-data:linux:$W/luks.img"
QARGS="-drive file=$W/disk.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP=2 MEM=1024 WAIT=300 \
  timeout 600 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "/bin/crypttest $HEX" > "$W/log" 2>&1 || true
fail=0
grep -aq "CRYPTTEST: ALLES OK" "$W/log" || { echo "FAIL  CRYPTTEST"; grep -a "FEHLER\|ok  \|crypt" "$W/log" | tail -25; fail=1; }
grep -aqiE "kernel panic|PANIK" "$W/log" && { echo "FAIL  kernel panic"; fail=1; }
dd if="$W/disk.img" of="$W/luks.img" bs=512 skip=2048 count=$(( $(stat -c %s "$W/luks.img") / 512 )) conv=notrunc status=none
printf '%s' "$PASS" | sudo -n cryptsetup open --key-file=- "$W/luks.img" ultar-crypt-t
sudo -n e2fsck -fn /dev/mapper/ultar-crypt-t >/dev/null 2>&1 || { echo "FAIL  e2fsck in the volume"; sudo -n e2fsck -fn /dev/mapper/ultar-crypt-t 2>&1 | tail -8; fail=1; }
sudo -n debugfs -R "cat /neu.txt" /dev/mapper/ultar-crypt-t 2>/dev/null | grep -q "verschluesselt" || { echo "FAIL  the kernel's file is not in the volume"; fail=1; }
sudo -n cryptsetup close ultar-crypt-t
grep -aq "geschrieben im Ultar-Kern" "$W/disk.img" && { echo "FAIL  plain text on the disk"; fail=1; }
[ "$fail" = 0 ] || { tail -30 "$W/log"; exit 1; }
echo "ok    CRYPTTEST: ALLES OK (LUKS2 data partition: locked, wrong key refused, read and write, host reads it back)"

# 2) the start path: ultar-luks (static, as in the image) formats on the host, in the kernel
# `ultar-luks unlock` asks on the console – first a wrong passphrase, then the right one.
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -q --target x86_64-unknown-linux-gnu -p ultar-luks
TOOL=target/x86_64-unknown-linux-gnu/release/ultar-luks
truncate -s 64M "$W/luks2.img"
echo "$PASS" | "$TOOL" format "$W/luks2.img" "$W/plain.img" --test
python3 tools/kern-test/gptimg.py "$W/disk2.img" "ultar-data:linux:$W/luks2.img"
cp out/ultar-kern-esp.img "$W/esp.img"
mcopy -i "$W/esp.img" "$TOOL" ::/bin/ultar-luks
QARGS="-drive file=$W/disk2.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" SMP=2 MEM=1024 WAIT=15 \
  timeout 300 python3 tools/kern-test/qtest.py "$W/esp.img" "/bin/ultar-luks unlock" "@falsch" "@$PASS" \
  "/bin/cat /sys/kernel/ultar/crypt" "/bin/cat /data/probe.txt" > "$W/log2" 2>&1 || true
grep -aq "Falsches Passwort" "$W/log2" || { echo "FAIL  wrong passphrase not reported"; fail=1; }
grep -aq "Daten entsperrt" "$W/log2" || { echo "FAIL  ultar-luks unlock did not open the data"; fail=1; }
grep -aq "ultar luks probe" "$W/log2" || { echo "FAIL  /data not readable after unlock"; fail=1; }
grep -aq "$PASS" "$W/log2" && { echo "FAIL  the passphrase was echoed"; fail=1; }
[ "$fail" = 0 ] || { tail -30 "$W/log2"; exit 1; }
echo "ok    ultar-luks unlock: wrong passphrase refused, right one opens /data in the kernel"

# 3) the graphical unlock screen: ultar-unlock (static) draws on the framebuffer, the keys come
# from the PS/2 keyboard (QEMU monitor sendkey): a wrong passphrase, then the right one.
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -q --target x86_64-unknown-linux-gnu -p ultar-unlock
mcopy -o -i "$W/esp.img" target/x86_64-unknown-linux-gnu/release/ultar-unlock ::/bin/ultar-unlock
dd if="$W/luks2.img" of="$W/disk2.img" bs=1M seek=1 conv=notrunc status=none
keys() { # text -> monitor sendkey commands
  python3 -c 'import sys
names={"-":"minus"," ":"spc"}
print("\n".join("!sendkey "+names.get(c,c) for c in sys.argv[1]))' "$1"
}
WAITS=$(for i in 1 2 3 4 5 6 7 8; do echo "!info status"; done)
mapfile -t CMDS < <(echo "/bin/ultar-unlock"; keys "falsch"; echo "!sendkey ret"; echo "$WAITS"; keys "$PASS"; \
  echo "!screendump $W/screen.ppm"; echo "!sendkey ret"; echo "$WAITS"; \
  echo "/bin/cat /sys/kernel/ultar/crypt"; echo "!info status"; echo "/bin/cat /data/probe.txt"; \
  echo "!info status"; echo "/bin/cat /data/probe.txt")
QARGS="-drive file=$W/disk2.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c -monitor unix:$W/mon,server,nowait" \
  MONITOR="$W/mon" SMP=2 MEM=1024 WAIT=8 timeout 300 python3 tools/kern-test/qtest.py "$W/esp.img" "${CMDS[@]}" > "$W/log3" 2>&1 || true
grep -aq "^open" "$W/log3" || { echo "FAIL  the unlock screen did not open the data"; fail=1; }
grep -aq "ultar luks probe" "$W/log3" || { echo "FAIL  /data not readable after the unlock screen"; fail=1; }
python3 - "$W/screen.ppm" <<'EOF' || fail=1
import sys
d = open(sys.argv[1], "rb").read()
px = d[d.index(b"255\n") + 4:]
colours = {px[i:i + 3] for i in range(0, len(px) - 3, 3 * 97)}
if len(colours) < 4:
    print("FAIL  the unlock screen was not drawn"); sys.exit(1)
EOF
[ "$fail" = 0 ] || { tail -30 "$W/log3"; exit 1; }
cp "$W/screen.ppm" "${SCREEN_OUT:-/tmp}/ultar-unlock-screen.ppm" 2>/dev/null || true
echo "ok    ultar-unlock: drawn on the framebuffer, wrong passphrase refused, right one opens /data"
