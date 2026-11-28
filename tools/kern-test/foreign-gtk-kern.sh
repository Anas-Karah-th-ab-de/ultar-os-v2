#!/usr/bin/env bash
# A GTK program of the Linux area on the Ultar kernel's screen (QEMU, screenshot): static ultar-compositor,
# ultar-wayland and foreignshow on the test image, the area with mousepad installed on a second disk
# (ultar-data, /data). foreignshow runs the program with the area as its root behind the bridge.
# AREA=<out dir of make-linux-runtime.sh with TEST_PACKAGE=mousepad> (default out/foreign-linux);
# IMG=<esp image>; SHOT=<png> (default out/foreign-gtk.png); PROGRAM (default /usr/bin/mousepad).
# Last line: FOREIGN-GTK: SHOT … (look at the picture) or FOREIGN-GTK: FAILED …
set -euo pipefail
cd "$(dirname "$0")/../.."
AREA="${AREA:-out/foreign-linux}"
[ -f "$AREA/linux/etc/ultar-runtime" ] && [ -f "$AREA/mousepad.deb" ] || TEST_PACKAGE=mousepad tools/foreign/make-linux-runtime.sh "$AREA"
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
  -p ultar-compositor --bin ultar-compositor -p ultar-wayland --bin ultar-wayland -p ultar-foreign --example foreignshow
BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 512M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-compositor ultar-wayland examples/foreignshow; do
  cp "$BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
# the area with the program installed (dpkg on this machine, the dependencies are in the area)
mkdir -p "$WORK/data/foreign"
cp -a "$AREA/linux" "$WORK/data/foreign/linux"
cp "$AREA/mousepad.deb" "$WORK/data/foreign/linux/tmp/mousepad.deb"
mount -t proc proc "$WORK/data/foreign/linux/proc"
chroot "$WORK/data/foreign/linux" dpkg -i /tmp/mousepad.deb > "$WORK/dpkg.log" 2>&1 || { umount "$WORK/data/foreign/linux/proc"; tail -10 "$WORK/dpkg.log"; exit 1; }
umount "$WORK/data/foreign/linux/proc"
rm -f "$WORK/data/foreign/linux/tmp/mousepad.deb"
size=$(( $(du -sm "$WORK/data" | cut -f1) * 3 / 2 + 128 ))
mke2fs -q -t ext4 -m 0 -E root_owner=0:0 -d "$WORK/data" "$WORK/data.ext4" "${size}M"
rm -rf "$WORK/data"
python3 tools/kern-test/gptimg.py "$WORK/disk.img" "ultar-data:linux:$WORK/data.ext4"
rm -f "$WORK/data.ext4"
SHOT="${SHOT:-out/foreign-gtk.png}"
mkdir -p "$(dirname "$SHOT")"
timeout 1200 python3 tools/kern-test/foreign-gtk-qemu.py "$WORK/esp.img" "$WORK/disk.img" "$SHOT" -- "${PROGRAM:-/usr/bin/mousepad}"
