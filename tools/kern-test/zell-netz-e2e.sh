#!/usr/bin/env bash
# Network isolation of cells on the Ultar kernel (docs/integration/zell-netz.md), with zellnetz
# (kernel/ultar-kern/user/zellnetz.c): a network namespace has only its own loopback (no host loopback service,
# no abstract Unix name of the host, no way out); with the "internet only" barrier it reaches the internet but
# not the device's own address, the LAN, broadcast or multicast – only the leased DNS server on port 53; a cell's
# listening socket is reachable neither through the host's loopback nor through the device's address; the
# barrier in the host namespace forbids its loopback, listening and fixed ports; it survives fork and execve;
# CLONE_NEWNET needs the MOUNT right; PR_SET_PDEATHSIG ends an orphan with its parent.
# The QEMU user network uses a public test range (RFC 5737 TEST-NET-2) so "the internet" is a guestfwd address
# inside the test machine: device 198.51.100.15, DNS .3, 198.51.100.100:8080 answers "INTERNET-OK".
# IMG=<esp image> (else scripts/build-kern.sh builds one). Verdict line: "ZELL-NETZ: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
LOG="${ZELL_NETZ_LOG:-$(mktemp)}"
NETDEV="user,id=n,net=198.51.100.0/24,guestfwd=tcp:198.51.100.100:8080-cmd:echo INTERNET-OK" \
  SMP="${SMP:-2}" WAIT="${WAIT:-240}" timeout 600 python3 tools/kern-test/qtest.py "$IMG" "/bin/zellnetz" > "$LOG" 2>&1 || true
grep -aE "^(ok|FEHLER) " "$LOG" || true
if grep -aq "^ZELLNETZ: ALLES OK" "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ZELL-NETZ: ALLES OK"
else
  tail -30 "$LOG"
  echo "ZELL-NETZ: FEHLER"
  exit 1
fi
