#!/usr/bin/env bash
# The A/B gate alone on a builder, its serial logs kept: bash tools/remote/ab-run.sh   (through rb.sh)
set -uo pipefail
mkdir -p out/ab
rm -rf out/ab/work; mkdir -p out/ab/work
AB_KEEP=1 AB_WORK="$PWD/out/ab/work" SMP="${SMP:-2}" timeout 3600 bash tools/kern-test/ab-test.sh >out/ab/ab.log 2>&1
echo "AB-RC $?"
grep -aE "^(FAIL|AB-TEST)|FEHLER" out/ab/ab.log | tail -20
tail -15 out/ab/ab.log
