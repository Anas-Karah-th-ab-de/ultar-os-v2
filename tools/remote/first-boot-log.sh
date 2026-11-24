#!/usr/bin/env bash
# Why the installed disk's first start failed: builds the VMware disk keeping its work files (KEEP=1), and when
# the start check fails reads the log service's records off the disk's data partition (what the services
# wrote to stderr is in there, the console has the service manager's lines only).
#   bash tools/remote/first-boot-log.sh [grep-pattern]      through tools/remote/rb.sh
set -uo pipefail
pat=${1:-.}
OUT=out/vmware
mkdir -p "$OUT"
PW=$OUT/test.password
(umask 077; printf '24680135792\n' >"$PW")
KEEP=1 CHECK=0 VM_PASSWORD_FILE="$PW" ACCEL=${ACCEL:-$([ -w /dev/kvm ] && echo kvm || echo tcg)} \
  bash tools/vm/make-vmware.sh >"$OUT/first-boot-build.log" 2>&1
echo "MAKE-RC $?"; tail -3 "$OUT/first-boot-build.log" | cut -c1-400
raw=$OUT/work/disk.raw
[ -f "$raw" ] || { echo "no $raw (the build got past the first start: nothing to read)"; exit 0; }
# the check started the disk on a copy-on-write layer (nothing of that start is on the disk): start it once
# more for real, the disk still as the installer left it
vars=$OUT/work/vars.fd; cp /usr/share/OVMF/OVMF_VARS_4M.fd "$vars"
timeout -s KILL "${BOOT_S:-45}" qemu-system-x86_64 -machine q35 -accel "$([ -w /dev/kvm ] && echo kvm || echo tcg)" \
  -cpu "$([ -w /dev/kvm ] && echo host || echo max)" -m 1024 -smp 4 -display none -no-reboot -vga std \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive "if=pflash,format=raw,file=$vars" \
  -netdev user,id=n -device e1000e,netdev=n -device qemu-xhci,id=xh -device usb-kbd,bus=xh.0 -device usb-tablet,bus=xh.0 \
  -drive "file=$raw,if=none,id=d0,format=raw" -device nvme,serial=ultar0,drive=d0 \
  -serial "file:$OUT/first-boot-serial.log" >/dev/null 2>&1
grep -a "ultar-init" "$OUT/first-boot-serial.log" | grep -aE "exit|refused|failed" | grep -av "code=0" | cut -c1-200
python3 - "$raw" "$OUT/work/data.ext4" <<'PY'
import sys
raw, out = sys.argv[1:3]
with open(raw, "rb") as f:
    f.seek(512)
    head = f.read(92)
    entries, count, size = (int.from_bytes(head[72:80], "little"), int.from_bytes(head[80:84], "little"),
                            int.from_bytes(head[84:88], "little"))
    f.seek(entries * 512)
    table = f.read(count * size)
    for i in range(count):
        e = table[i * size:(i + 1) * size]
        if e[56:128].decode("utf-16-le", "ignore").rstrip("\0") == "ultar-data":
            first, last = int.from_bytes(e[32:40], "little"), int.from_bytes(e[40:48], "little")
            f.seek(first * 512)
            with open(out, "wb") as o:
                left = (last - first + 1) * 512
                while left > 0:
                    chunk = f.read(min(left, 1 << 22))
                    if not chunk:
                        break
                    o.write(chunk)
                    left -= len(chunk)
PY
rm -rf "$OUT/first-boot-log"; mkdir -p "$OUT/first-boot-log"
debugfs -R "rdump /ultar/log $OUT/first-boot-log" "$OUT/work/data.ext4" >/dev/null 2>&1
debugfs -R "ls -l /ultar/mesh/state" "$OUT/work/data.ext4" 2>/dev/null | head -20
ULOG="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/ulog"
"$ULOG" export "$OUT/first-boot-log/log" "$OUT/first-boot-log.txt" --include-kernel-text >/dev/null 2>&1
grep -aE "$pat" "$OUT/first-boot-log.txt" | cut -c1-400 | head -80
