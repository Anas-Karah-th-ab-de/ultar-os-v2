#!/usr/bin/env bash
# What Wine and other Linux programs need from the Ultar kernel (docs/integration/kern-wine.md), with
# winetest (kernel/ultar-kern/user/winetest.c): mmap address wishes (hint, MAP_FIXED_NOREPLACE, MAP_FIXED over
# other mappings, partial munmap/mremap, MAP_32BIT, large MAP_NORESERVE reservations, the second low window
# at 0x7c000000 for Wine's preloader and KUSER_SHARED_DATA), fault siginfo (si_addr, si_code, REG_CR2),
# waitid (P_PID, P_ALL, P_PGID, WEXITED|WNOHANG|WNOWAIT), arch_prctl GS kept across switches and fork,
# sigaltstack with SA_ONSTACK and SS_AUTODISARM, MAP_SHARED on files (coherent with read/write, msync).
# IMG=<esp image> (else scripts/build-kern.sh builds one). Verdict line: "WINE-KERN: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
LOG="${WINE_KERN_LOG:-$(mktemp)}"
SMP="${SMP:-2}" WAIT="${WAIT:-180}" timeout 600 python3 tools/kern-test/qtest.py "$IMG" "/bin/winetest" > "$LOG" 2>&1 || true
grep -aE "^(ok|FEHLER|skip) " "$LOG" || true
if grep -aq "^WINETEST: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "WINE-KERN: ALLES OK"
else
  tail -30 "$LOG"
  echo "WINE-KERN: FEHLER"
  exit 1
fi
