#!/usr/bin/env bash
# The desktop session on the Ultar kernel: ultar-init (not PID 1) starts the shipped logd, bus, seat,
# authd, compositor, lock, sysdialog and desktop definitions, each as its own user; the session starts locked,
# the host unlocks it, locks with Super+L, types the test password through the virtual USB keyboard and judges
# the screenshots
# (tools/kern-test/desktop-qemu.py). Verdict line: "DESKTOP: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# DESK_BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-devd-seat, ultar-authd,
# ultar-compositor, ultar-lock, ultar-sysdialog, ultar-desktop and examples/desktoptest (else built here).
# DESK_MACHINE/DESK_DEV/DESK_NIC/DESK_VGA: other hardware (vmware-like.sh), DESK_LABEL names the run.
# SHOTS=<prefix> keeps the screenshots (<prefix>-desktop.ppm, -locked.ppm, -unlocked.ppm).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${DESK_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-devd -p ultar-auth -p ultar-compositor -p ultar-desktop \
    --features ultar-compositor/test-hooks --bins --examples
  DESK_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs (fonts inside) do not fit the 64 MiB test image.
truncate -s 512M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-init ultar-busd ultar-logd ultar-devd-seat ultar-authd ultar-compositor ultar-lock \
         ultar-sysdialog ultar-desktop examples/desktoptest; do
  cp "$DESK_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/desktop-e2e.log}"
SHOTS="${SHOTS:-$WORK/shot}"
MACHINE="${DESK_MACHINE:-q35}" DEV="${DESK_DEV:-virtio-blk-pci}" NIC="${DESK_NIC:-virtio-net-pci}" VGA="${DESK_VGA:-std}" \
SMP="${SMP:-2}" MEM="${MEM:-1536}" timeout 1500 python3 tools/kern-test/desktop-qemu.py "$WORK/esp.img" "$SHOTS" \
  > "$LOG" 2>&1 || true
if grep -aq "^DESKTOP: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    DESKTOP: ALLES OK${DESK_LABEL:+ ($DESK_LABEL)}"
  if [ "${VERBOSE:-0}" = 1 ]; then grep -aE "^screen|desktoptest:" "$LOG"; fi
else
  echo "FAIL  DESKTOP${DESK_LABEL:+ ($DESK_LABEL)}"; grep -aE "DESKTOP|desktoptest|screen [0-9]|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
