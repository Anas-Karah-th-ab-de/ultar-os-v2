#!/usr/bin/env bash
# Camera for video calls on the Ultar kernel: kameratest opens /dev/ultar/kamera (gen-xhci uvc,
# USB Video Class MJPEG over isochronous transfers), reads 8 s of pictures and decodes them.
# KAMERA_USB=<vid>:<pid> passes a real USB camera of the host into the machine (QEMU usb-host;
# the host user needs access to /dev/bus/usb; KAMERA_HOSTOPTS adds usb-host options, e.g.
# isobufs=32,isobsize=64 for a camera behind usbip); without it the machine has no camera and the
# node must be absent ("KAMERA: KEINE KAMERA").
# Verdict lines: "KAMERA: ALLES OK" (camera) / "KAMERA: KEINE KAMERA" (none), "KAMERA-E2E: ALLES OK".
# IMG=<esp image> to use a built kernel image; KAMERA_BIN=<dir> with a static examples/kameratest.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${KAMERA_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
    -p ultar-call --example kameratest
  KAMERA_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 128M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
cp "$KAMERA_BIN/examples/kameratest" "$WORK/kameratest"; strip "$WORK/kameratest" 2>/dev/null || true
mcopy -o -i "$WORK/esp.img" "$WORK/kameratest" ::/bin/
LOG="${LOG:-$WORK/kamera-e2e.log}"
QARGS="-device qemu-xhci,id=xhci ${KAMERA_QARGS:-}"
WANT="KAMERA: KEINE KAMERA"
if [ -n "${KAMERA_USB:-}" ]; then
  QARGS="$QARGS -device usb-host,bus=xhci.0,vendorid=0x${KAMERA_USB%%:*},productid=0x${KAMERA_USB##*:}${KAMERA_HOSTOPTS:+,$KAMERA_HOSTOPTS}"
  WANT="KAMERA: ALLES OK"
fi
QARGS="$QARGS" SMP="${SMP:-2}" MEM="${MEM:-512}" WAIT="${WAIT:-90}" timeout 300 python3 tools/kern-test/qtest.py \
  "$WORK/esp.img" "/bin/kameratest" > "$LOG" 2>&1 || true
if ! grep -q "$WANT" "$LOG" || grep -qiE "kernel panic|PANIK" "$LOG"; then
  echo "FAIL  KAMERA (want: $WANT)"; grep -aE "KAMERA|uvc|xhci|FEHLER|PANIK" "$LOG" | tail -30; exit 1
fi
grep -aE "\[uvc\]|Kamera|KAMERA" "$LOG" | tail -6
echo "ok    KAMERA-E2E: ALLES OK ($WANT)"
