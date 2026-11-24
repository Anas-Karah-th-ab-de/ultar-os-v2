#!/usr/bin/env bash
# Bluetooth scan on the Ultar kernel: the kernel gate's BT scenario (bt-e2e.sh: scripted USB controller, btd
# scans, the desktop page built from the bus) and the real-input run on the VMware disk (run "bt": Settings >
# Bluetooth at 1024x768, the search switch clicked with the pointer, the found devices read off the screen).
# Run on a builder through tools/remote/rb.sh: bash tools/remote/btscan-gate.sh
set -uo pipefail
IMG=1 bash scripts/build-kern.sh >/dev/null && IMG=out/ultar-kern-esp.img VERBOSE=1 bash tools/kern-test/bt-e2e.sh
echo "BT-E2E-RC $?"
INPUT_RUNS=bt bash tools/vm/vmware-input-e2e.sh
echo "BT-VM-RC $?"
grep -a "bluetooth" out/vmware/input-bt-1.out
