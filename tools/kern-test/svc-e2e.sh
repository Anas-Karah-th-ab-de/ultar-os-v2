#!/usr/bin/env bash
# The display and account services on the Ultar kernel: ultar-init (not PID 1) starts the shipped logd, bus,
# settingsd, displayd and accountd definitions and a headless compositor with the shipped rights file, each as
# its own user; svctest checks the displays on the bus (identity from the displays' own data), Super+P waiting for
# "Keep?", killing the display service (the compositor goes back at once, the restarted service takes the
# waiting setting back), the first administrator with a home handed to id 1000 (right file_admin), a second user
# only after the system dialog, and a clean shutdown. Verdict line: "SVC: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# SVC_BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-settingsd, ultar-compositor,
# ultar-displayd, ultar-accountd and examples/svctest (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${SVC_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-settingsd -p ultar-compositor -p ultar-displayd \
    -p ultar-accountd --bins
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-displayd --example svctest
  SVC_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs do not fit the 64 MiB test image.
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-init ultar-busd ultar-logd ultar-settingsd ultar-compositor ultar-displayd ultar-accountd examples/svctest; do
  cp "$SVC_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/svc-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-300}" timeout 900 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/svctest --bin /bin --work /tmp/svc --group-root /sys/fs/cgroup" > "$LOG" 2>&1 || true
# every service ends with code 0 on SIGTERM
clean=1
for s in logd bus settingsd compositor displayd accountd; do grep -aqE "exit $s pid=[0-9]+ code=0 " "$LOG" || clean=0; done
if grep -aq "SVC: ALLES OK" "$LOG" && [ "$clean" = 1 ] && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    SVC: ALLES OK (display and account services, every service ended with code 0)"
  if [ "${VERBOSE:-0}" = 1 ]; then grep -aE "^SVC" "$LOG"; fi
else
  echo "FAIL  SVC"; grep -aE "SVC|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
