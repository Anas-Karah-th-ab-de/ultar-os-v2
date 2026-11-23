#!/usr/bin/env bash
# Narrows down a stage-1 failure on the target: same kernel and programs, different processor models and core counts.
# Usage (after target-stage1.sh built out/remote-test): tools/kern-test/target-diag.sh
set -uo pipefail
cd "$(dirname "$0")/../.."
B=out/remote-test
run() { # label accel cpu smp
  local log="out/diag-$1.log"
  ACCEL="$2" CPU="$3" SMPN="$4" SERIAL=1 LIMIT=200 bash tools/kern-test/kvm-on-target.sh "$B/ultar-kern-remote-test.efi" "$B/bin" > "$log" 2>&1
  local k=0 b=0 z=0
  grep -aq "Ergebnis: ALLES OK" "$log" && k=1; grep -aq "BUS SELFCHECK OK" "$log" && b=1; grep -aq "ZELLTEST: ALLES OK" "$log" && z=1
  echo "$1: kerntest=$k bus=$b zelltest=$z"
}
run kvm-host-all kvm host "$(nproc)"
run kvm-host-2   kvm host 2
run kvm-host-1   kvm host 1
run kvm-max-2    kvm max 2
run kvm-base-2   kvm qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt 2
run tcg-max-2    tcg max 2
echo "--- serial of kvm-host-2: lines around the first program ---"
grep -an -A25 "Programm kerntest" out/diag-kvm-host-2.log | head -60
echo "--- kernel messages with trap/fault ---"
grep -aniE "trap|fault|ausnahme|panik|#GP|#UD|#PF|invalid|sigill|sigsegv|beendet|exit" out/diag-kvm-host-2.log | head -20
