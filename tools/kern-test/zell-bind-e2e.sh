#!/usr/bin/env bash
# Single files in a cell's view and chown inside a cell on the Ultar kernel (docs/integration/zell-chown.md),
# with zellbind (kernel/ultar-kern/user/zellbind.c): bind mounts of device nodes (null, zero, urandom, tty), of a
# host file read-only and writable and of a host Unix socket's node onto files; the kind must match (ENOTDIR);
# remount and umount of a file mount point; a mount point cannot be removed or renamed (EBUSY). The right
# CELL_CHOWN: cell root without FILE_ADMIN changes owners on the cell's own writable mounts only – not through
# the copied host tree, not on a read-only mount, not through a descriptor of an outside entry, not outside a
# cell, not without the right (CAP_CHOWN takes it along); set-id bits never appear in a cell.
# IMG=<esp image> (else scripts/build-kern.sh builds one). Verdict line: "ZELL-BIND: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
LOG="${ZELL_BIND_LOG:-$(mktemp)}"
SMP="${SMP:-2}" WAIT="${WAIT:-180}" timeout 600 python3 tools/kern-test/qtest.py "$IMG" "/bin/zellbind" > "$LOG" 2>&1 || true
grep -aE "^(ok|FEHLER) |^      \(Antwort" "$LOG" || true
if grep -aq "^ZELLBIND: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ZELL-BIND: ALLES OK"
else
  tail -30 "$LOG"
  echo "ZELL-BIND: FEHLER"
  exit 1
fi
