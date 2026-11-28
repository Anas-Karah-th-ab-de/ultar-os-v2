#!/usr/bin/env bash
# What programs of other systems (dpkg, apt, perl, GTK apps, terminals) need from the Ultar kernel
# (docs/integration/ultar-foreign.md), with fremdtest (kernel/ultar-kern/user/fremdtest.c): execve resets
# caught signal handlers and keeps ignored ones; rmdir of a file is ENOTDIR and unlink of a directory EISDIR
# (dpkg tells files from directories that way); a directory named with a trailing slash lists its entries;
# close_range, sync_file_range, msync, fchdir; pseudo-terminals: /dev/ptmx and /dev/pts/N with the line
# discipline (canonical lines, echo, ICRNL, ONLCR, raw mode), TIOCGPTPEER, only the owner opens the slave.
# IMG=<esp image> (else scripts/build-kern.sh builds one). Verdict line: "FREMD-KERN: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
LOG="${FREMD_KERN_LOG:-$(mktemp)}"
SMP="${SMP:-2}" WAIT="${WAIT:-180}" timeout 600 python3 tools/kern-test/qtest.py "$IMG" "/bin/fremdtest" > "$LOG" 2>&1 || true
grep -aE "^(ok|FEHLER) " "$LOG" || true
if grep -aq "^FREMDTEST: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "FREMD-KERN: ALLES OK"
else
  tail -30 "$LOG"
  echo "FREMD-KERN: FEHLER"
  exit 1
fi
