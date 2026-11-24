#!/usr/bin/env bash
# The app launcher on the Ultar kernel: ultar-init (not PID 1) starts the shipped logd, bus and launcher
# definitions; launchtest installs signed packages over the bus, starts apps in their cells (own user ids,
# mount + pid namespaces, memory groups apps/<app>, no kernel rights) and lets a malicious app attack its
# cell: another app's data and process, root, kernel rights, namespaces, mounts, the system's files,
# privileged sockets, memory and processes beyond its group's limits. Verdict line: "LAUNCHER: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# LAUNCH_BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-launcher, ultar-launcher-testapp and
# examples/launchtest (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${LAUNCH_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-launcher --bins --examples
  LAUNCH_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs do not fit the 64 MiB test image.
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-init ultar-busd ultar-logd ultar-launcher ultar-launcher-testapp examples/launchtest; do
  cp "$LAUNCH_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/launcher-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-300}" timeout 900 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/launchtest --bin /bin --work /tmp/lt --groups" > "$LOG" 2>&1 || true
if grep -aq "LAUNCHER: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    LAUNCHER: ALLES OK"
  [ "${VERBOSE:-0}" = 1 ] && grep -aE "^LAUNCHER" "$LOG"
else
  echo "FAIL  LAUNCHER"; grep -aE "LAUNCHER|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
