#!/bin/sh
# Wayland bridge on the Ultar kernel (QEMU): static ultar-compositor,
# ultar-wayland and uwl-demo on a 512 MiB copy of the test image, started by
# uwl-qemu.sh; the verdict comes from a screenshot (wayland-qemu.py).
#   IMG=<esp.img> (default: IMG=1 scripts/build-kern.sh)  SHOT=<file.ppm>
set -u
cd "$(dirname "$0")/../.." || exit 2
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null || exit 2; IMG=out/ultar-kern-esp.img; fi
RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
  -p ultar-compositor --bin ultar-compositor -p ultar-wayland --bin ultar-wayland --example uwl-demo || exit 2
BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 512M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-compositor ultar-wayland examples/uwl-demo; do
  cp "$BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  file "$WORK/$(basename "$b")" | grep -q "statically linked" || echo "warning: $b is not static"
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
mcopy -o -i "$WORK/esp.img" tools/kern-test/uwl-qemu.sh ::/bin/uwl-qemu.sh
SHOT="${SHOT:-$WORK/shot.ppm}"
timeout 600 python3 tools/kern-test/wayland-qemu.py "$WORK/esp.img" "$SHOT"
