#!/usr/bin/env bash
# The service manager on the Ultar kernel: ultar-init (not PID 1) starts the shipped logd, bus,
# settingsd and devd definitions, each as its own user, in their memory groups; inittest checks
# users, output in the log and a clean shutdown (each service exits 0 on SIGTERM). Verdict line: "INIT: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# INIT_BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-settingsd, ultar-devd and
# examples/inittest (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${INIT_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-settingsd -p ultar-devd --bins --examples
  INIT_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs do not fit the 64 MiB test image.
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-init ultar-busd ultar-logd ultar-settingsd ultar-devd examples/inittest; do
  cp "$INIT_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/init-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-180}" timeout 900 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/inittest --bin /bin --work /tmp/it --groups" > "$LOG" 2>&1 || true
# every service ends with code 0 on SIGTERM (not by its runtime's stop deadline, code 72)
clean=1
for s in logd bus settingsd devd; do grep -aqE "exit $s pid=[0-9]+ code=0 " "$LOG" || clean=0; done
if grep -q "INIT: ALLES OK" "$LOG" && [ "$clean" = 1 ] && ! grep -qiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    INIT: ALLES OK (every service ended with code 0)"
else
  echo "FAIL  INIT"; grep -aE "INIT:|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
