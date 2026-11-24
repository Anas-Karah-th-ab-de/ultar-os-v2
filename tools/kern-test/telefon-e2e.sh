#!/usr/bin/env bash
# The phone on the Ultar kernel: telefontest starts a bus broker, the test-double modem
# (`modem.*`), the real ultar-calld process (--modem) and the phone app (headless) and runs
# incoming -> answer -> hold -> end plus the call history on disk. Verdict line: "TELEFON: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# TEL_BIN=<dir> with static ultar-calld and examples/telefontest (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${TEL_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-call --bin ultar-calld -p phone-e2e --example telefontest
  TEL_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in ultar-calld examples/telefontest; do
  cp "$TEL_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/telefon-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-180}" timeout 600 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/telefontest --calld /bin/ultar-calld --work /tmp/telefon" > "$LOG" 2>&1 || true
if grep -q "TELEFON: ALLES OK" "$LOG" && ! grep -qiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    TELEFON: ALLES OK"
else
  echo "FAIL  TELEFON"; grep -aE "TELEFON|^ok |FEHLER" "$LOG" | tail -20; exit 1
fi
