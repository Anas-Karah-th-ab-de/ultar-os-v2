#!/bin/sh
# End to end of the apps from other systems on Linux (root, real cells):
# system/core/ultar-launcher/tests/foreign_e2e.rs with the Linux area of
# tools/foreign/make-linux-runtime.sh (built here when OUT has none).
#
#   tools/foreign/linux-e2e.sh [OUT]       (as root; never on the VPS)
#
# Final line on success: FOREIGN-LINUX-E2E: ALLES OK
set -eu
fail() { echo "linux-e2e: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root (real cells)"
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
OUT=${1:-$REPO/out/foreign-linux}
if [ ! -f "$OUT/linux/etc/ultar-runtime" ] || [ ! -f "$OUT/mousepad.deb" ]; then
  TEST_PACKAGE=mousepad "$HERE/make-linux-runtime.sh" "$OUT"
fi
cd "$REPO"
cargo build -q -p ultar-foreign --bin ultar-foreign
BIN=${CARGO_TARGET_DIR:-$REPO/target}/debug/ultar-foreign
[ -x "$BIN" ] || fail "no $BIN"
LOG=$(mktemp)
ULTAR_FOREIGN_RUNTIME="$OUT/linux" ULTAR_FOREIGN_DEB="$OUT/mousepad.deb" ULTAR_FOREIGN_BIN="$BIN" \
  cargo test -q -p ultar-launcher --test foreign_e2e -- --nocapture --test-threads 1 >"$LOG" 2>&1 \
  || { tail -60 "$LOG"; fail "test failed"; }
grep '^FOREIGN' "$LOG"
grep -q '^FOREIGN-LINUX-E2E: ALLES OK$' "$LOG" || { tail -40 "$LOG"; fail "the test did not run (skipped?)"; }
rm -f "$LOG"
