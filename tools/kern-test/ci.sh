#!/usr/bin/env bash
# Kernel gate: build the Ultar kernel, boot it in QEMU and prove that
#   1. every device has a driver, 2. the ring-3 test program passes,
#   3. the system bus (ultar-bus) runs on the Ultar kernel,
#   4. the escape tests of the process isolation pass (zelltest: mount/pid namespaces, rights),
#   5. the same for symbolic links on an ext4 disk (zell-ext4.sh; skipped without mke2fs).
# Needs: rustc-1.97 cargo-1.97 rust-1.97-src qemu-system-x86 ovmf mtools dosfstools gcc e2fsprogs
# BUSCK=<file>: use this prebuilt ultar-bus selfcheck instead of building it from the workspace.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG=1 bash scripts/build-kern.sh >/dev/null
if [ -z "${BUSCK:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p ultar-bus --example selfcheck --target x86_64-unknown-linux-gnu
  BUSCK="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/examples/selfcheck"
fi
mcopy -o -i out/ultar-kern-esp.img "$BUSCK" ::/bin/busck
LOG="${LOG:-$(mktemp)}"
SMP="${SMP:-2}" timeout 300 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "geraete" "/bin/kerntest" "/bin/busck" "/bin/zelltest" > "$LOG" 2>&1 || true
fail=0
for expect in "alle Geräte haben einen Treiber" "Ergebnis: ALLES OK" "BUS SELFCHECK OK" "ZELLTEST: ALLES OK"; do
  if grep -q "$expect" "$LOG"; then echo "ok    $expect"; else echo "FAIL  $expect"; fail=1; fi
done
if grep -qiE "kernel panic|PANIK" "$LOG"; then echo "FAIL  kernel panic"; fail=1; fi
grep -o "\[kern\] bereit nach [0-9]* ms" "$LOG" || true
[ "$fail" = 0 ] || { tail -40 "$LOG"; exit 1; }
if command -v mke2fs >/dev/null; then
  bash tools/kern-test/zell-ext4.sh
else
  echo "skip  ZELLTEST-EXT4 (mke2fs missing)"
fi
