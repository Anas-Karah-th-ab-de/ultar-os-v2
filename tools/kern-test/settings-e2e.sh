#!/usr/bin/env bash
# The Settings app against the real settings service on the Ultar kernel: ultar-init (not PID 1) starts the
# shipped logd, bus and settingsd definitions, each as its own user; settingstest runs the app headless with its
# real bus ports and opens every area in Arabic, English and German (none may say "not available in this
# version", every settingsd page shows its rows, no untranslated key), checks the grouped sidebar and its
# search, and switches screen sharing on through the Sharing page. Verdict line: "SETTINGS: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# ST_BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-settingsd and examples/settingstest
# (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${ST_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-settingsd --bins
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-settings --example settingstest
  ST_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs (fonts inside) do not fit the 64 MiB test image.
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-init ultar-busd ultar-logd ultar-settingsd examples/settingstest; do
  cp "$ST_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/settings-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-600}" timeout 1200 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/settingstest --bin /bin --work /tmp/st --group-root /sys/fs/cgroup" > "$LOG" 2>&1 || true
# every service ends with code 0 on SIGTERM
clean=1
for s in logd bus settingsd; do grep -aqE "exit $s pid=[0-9]+ code=0 " "$LOG" || clean=0; done
if grep -aq "SETTINGS: ALLES OK" "$LOG" && [ "$clean" = 1 ] && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    SETTINGS: ALLES OK (every page of the Settings app on the real settings service)"
  if [ "${VERBOSE:-0}" = 1 ]; then grep -aE "^SETTINGS" "$LOG"; fi
else
  echo "FAIL  SETTINGS"; grep -aE "SETTINGS|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
