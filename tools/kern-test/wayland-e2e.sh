#!/bin/sh
# Wayland bridge (platform/ultar-wayland) end to end on Linux:
#  1. the crate's tests with a raw Wayland client against the real compositor
#     (headless, in-process) and the bridge as its own process: window with
#     content, keys/text, click, resize, clipboard both ways, popups, close and
#     exit code, limits, fixed-seed fuzzing;
#  2. real Ubuntu apps (weston-simple-shm, weston-terminal, foot, gtk4-demo
#     with GSK_RENDERER=cairo, Qt6 via PyQt6, SDL3, uwl-demo), each with
#     screenshots; apps that are not installed are skipped.
# Last line: "WAYLAND: ALLES OK" (or "WAYLAND: FEHLER").
#   OUT=<dir> (default /tmp/uwl-e2e): logs and screenshots (OUT/shots/*.png)
set -u
cd "$(dirname "$0")/../.." || exit 2
OUT=${OUT:-/tmp/uwl-e2e}
mkdir -p "$OUT/shots"
fail=0

cargo test -p ultar-wayland >"$OUT/tests.log" 2>&1
grep -E "^test |test result|fuzz:" "$OUT/tests.log"
grep -q "test result: FAILED" "$OUT/tests.log" && fail=1
grep -q "test result: ok" "$OUT/tests.log" || fail=1

cargo build -q --release -p ultar-wayland --example uwl-demo || fail=1
UWL_DEMO="${CARGO_TARGET_DIR:-target}/release/examples/uwl-demo" \
ULTAR_WAYLAND_REAL=1 ULTAR_WAYLAND_SHOTS="$OUT/shots" ULTAR_WAYLAND_OUT="$OUT/apps" \
  cargo test --release -p ultar-wayland --test real -- --nocapture --test-threads=1 >"$OUT/real.log" 2>&1
grep -E "^REAL|WAYLAND-REAL" "$OUT/real.log"
grep -q "test result: ok. 1 passed" "$OUT/real.log" || fail=1

echo "screenshots:"
ls -1 "$OUT/shots" 2>/dev/null | sed "s|^|  $OUT/shots/|"
if [ "$fail" = 0 ]; then
  echo "WAYLAND: ALLES OK"
else
  echo "WAYLAND: FEHLER (logs in $OUT)"
  exit 1
fi
