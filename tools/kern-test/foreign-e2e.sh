#!/usr/bin/env bash
# Apps from other systems on the Ultar kernel (docs/integration/ultar-foreign.md): boots the kernel with a
# second disk holding an ext4 named ultar-data (/data) with the Linux area of
# tools/foreign/make-linux-runtime.sh at /data/foreign/linux and a .deb, and runs foreigntest: every kernel
# feature the foreign cells need, one by one, then ultar-foreign's own inspection and installation cells with
# the area's Ubuntu programs (dpkg-deb, dpkg, apt-get). Verdict line: "FOREIGN: ALLES OK"; otherwise the list
# "FOREIGN: FEHLENDE KERN-FUNKTIONEN: …".
# AREA=<out dir of make-linux-runtime.sh with TEST_PACKAGE=mousepad> (default out/foreign-linux, built when
# missing); IMG=<esp image> (else scripts/build-kern.sh builds one); FOREIGN_BIN=<dir> with static
# ultar-foreign and examples/foreigntest (else built here). Without KVM QEMU emulates: WAIT defaults to 1500 s.
# With the component "Android apps" (AREA = out dir of tools/foreign/make-android-runtime.sh, with its
# hello.dex and a mousepad.deb copied in) foreigntest also lets ART run hello.dex (check android-art).
set -euo pipefail
cd "$(dirname "$0")/../.."
AREA="${AREA:-out/foreign-linux}"
if [ ! -f "$AREA/linux/etc/ultar-runtime" ] || [ ! -f "$AREA/mousepad.deb" ]; then
  TEST_PACKAGE=mousepad tools/foreign/make-linux-runtime.sh "$AREA"
fi
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${FOREIGN_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
    -p ultar-foreign --bin ultar-foreign --example foreigntest
  FOREIGN_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-foreign examples/foreigntest; do
  cp "$FOREIGN_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
# the data disk: the area (owners and modes kept) and the package
mkdir -p "$WORK/data/foreign"
cp -a "$AREA/linux" "$WORK/data/foreign/linux"
cp "$AREA/mousepad.deb" "$WORK/data/foreign/mousepad.deb"
DEXARG=""
if [ -f "$AREA/hello.dex" ]; then cp "$AREA/hello.dex" "$WORK/data/foreign/hello.dex"; DEXARG="--dex /data/foreign/hello.dex"; fi
size=$(( $(du -sm "$WORK/data" | cut -f1) * 3 / 2 + 128 ))
mke2fs -q -t ext4 -m 0 -E root_owner=0:0 -d "$WORK/data" "$WORK/data.ext4" "${size}M"
rm -rf "$WORK/data"
python3 tools/kern-test/gptimg.py "$WORK/disk.img" "ultar-data:linux:$WORK/data.ext4"
rm -f "$WORK/data.ext4"
LOG="${LOG:-$WORK/foreign-e2e.log}"
QARGS="-drive file=$WORK/disk.img,if=none,id=c,format=raw -device virtio-blk-pci,drive=c" \
  SMP="${SMP:-2}" MEM="${MEM:-2048}" WAIT="${WAIT:-1500}" timeout 2400 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/foreigntest --helper /bin/ultar-foreign --area /data/foreign/linux --deb /data/foreign/mousepad.deb $DEXARG" \
  > "$LOG" 2>&1 || true
grep -aE "^FOREIGN" "$LOG" | tr -d '\r' || true
if grep -aiqE "kernel panic|PANIK" "$LOG"; then echo "FAIL  kernel panic"; grep -aiE -A5 "kernel panic|PANIK" "$LOG" | head -20; exit 1; fi
if grep -aq "FOREIGN: ALLES OK" "$LOG"; then
  echo "ok    FOREIGN: ALLES OK"
else
  grep -aq "FOREIGN: FEHLENDE" "$LOG" || { echo "FAIL  foreigntest did not finish"; tail -30 "$LOG"; }
  exit 1
fi
