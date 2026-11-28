#!/usr/bin/env bash
# A program from another system the way a person gets it, on the Ultar kernel (QEMU): foreignapp brings up
# the bus with the shipped policy, the launcher, the system dialog (authorized), the compositor on the screen
# and ultar-foreign; Files installs mousepad.deb through foreign.install, the launcher starts the app as from
# the app grid (app.foreign: own user, the Linux area read-only, behind ultar-wayland, its own title bar),
# a screenshot is taken, the stop ends the cell and removal leaves nothing behind. Before it, componenttest
# installs a small signed component „Linux-Programme“ as the service does (unpacking cell, owners, swap).
# AREA=<out dir of make-linux-runtime.sh with TEST_PACKAGE=mousepad> (default out/foreign-linux); IMG=<esp
# image>; SHOT=<png> (default out/foreign-app-kern.png); APP_BIN=<dir> with the static binaries (else built).
# Last line: "FOREIGN-APP-KERN: ALLES OK" (and the screenshot to look at) or "FAIL  FOREIGN-APP-KERN".
set -euo pipefail
cd "$(dirname "$0")/../.."
AREA="${AREA:-out/foreign-linux}"
[ -f "$AREA/linux/etc/ultar-runtime" ] && [ -f "$AREA/mousepad.deb" ] || TEST_PACKAGE=mousepad tools/foreign/make-linux-runtime.sh "$AREA"
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${APP_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
    -p ultar-compositor --bin ultar-compositor -p ultar-wayland --bin ultar-wayland \
    -p ultar-foreign --bin ultar-foreign --example componenttest \
    -p ultar-launcher --bin ultar-launcher --example foreignapp
  APP_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 512M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-compositor ultar-wayland ultar-foreign ultar-launcher examples/foreignapp examples/componenttest; do
  cp "$APP_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
# the data disk: the area as built (mousepad not installed; the service installs it) and the package
mkdir -p "$WORK/data/foreign"
cp -a "$AREA/linux" "$WORK/data/foreign/linux"
cp "$AREA/mousepad.deb" "$WORK/data/mousepad.deb"
size=$(( $(du -sm "$WORK/data" | cut -f1) * 3 / 2 + 256 ))
mke2fs -q -t ext4 -m 0 -E root_owner=0:0 -d "$WORK/data" "$WORK/data.ext4" "${size}M"
rm -rf "$WORK/data"
python3 tools/kern-test/gptimg.py "$WORK/disk.img" "ultar-data:linux:$WORK/data.ext4"
rm -f "$WORK/data.ext4"
SHOT="${SHOT:-out/foreign-app-kern.png}"
mkdir -p "$(dirname "$SHOT")"
LOG="${LOG:-$WORK/foreign-app-kern.log}"
timeout 5400 python3 tools/kern-test/foreign-app-qemu.py "$WORK/esp.img" "$WORK/disk.img" "$SHOT" \
  "/bin/foreignapp --bin /bin --deb /data/mousepad.deb --groups --draw ${DRAW:-60} --component /bin/componenttest" > "$LOG" 2>&1 || true
grep -aE "^(FOREIGN-APP|COMPONENT)" "$LOG" | tr -d '\r' || true
if grep -aiqE "kernel panic|PANIK" "$LOG"; then echo "FAIL  kernel panic"; grep -aiE -A5 "kernel panic|PANIK" "$LOG" | head -20; exit 1; fi
if grep -aq "FOREIGN-APP-KERN: ALLES OK" "$LOG" && grep -aq "FOREIGN-APP-SHOT:" "$LOG" && grep -aq "COMPONENT: ALLES OK" "$LOG"; then
  echo "ok    FOREIGN-APP-KERN: ALLES OK ($(grep -aoE 'FOREIGN-APP-SHOT:.*' "$LOG" | tr -d '\r'))"
else
  echo "FAIL  FOREIGN-APP-KERN"; tr -d '\r' <"$LOG" | grep -avE "^\s*$" | tail -40; exit 1
fi
