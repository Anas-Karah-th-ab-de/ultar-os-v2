#!/usr/bin/env bash
# Checks of the phone/keyboard fix: fmt, clippy -D warnings and tests of the touched crates
# (STEP=checks), the VMware-like real-input gate (STEP=gate, REPEAT/INPUT_RUNS as vmware-input-e2e.sh).
set -uo pipefail
cd "$(dirname "$0")/../.."
CRATES="-p ultar-phone -p ultar-compositor -p ultar-surface -p ultar-desktop -p ultar-settingsd -p ultar-displayd"
case "${STEP:-checks}" in
  checks)
    cargo fmt --all -- --check && echo "FMT OK" || echo "FMT FEHLER"
    cargo clippy $CRATES --all-targets -- -D warnings && echo "CLIPPY OK" || echo "CLIPPY FEHLER"
    cargo test $CRATES 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)" | sort | uniq -c
    echo "TESTS-EXIT ${PIPESTATUS[0]}"
    ;;
  phone)
    cargo test -p ultar-phone --test window_size 2>&1 | grep -aA4 "panicked" | head -20
    ;;
  gate)
    REPEAT=${REPEAT:-3} INPUT_RUNS=${INPUT_RUNS:-usb usb-xga} bash tools/vm/vmware-input-e2e.sh
    ;;
esac
# STEP=gate-reuse: the gate on the disk built before (only the gate script changed)
if [ "${STEP:-}" = gate-reuse ]; then
  VMDK=out/vmware/UltarOS-v2/UltarOS-v2.vmdk VM_PASSWORD_FILE=out/vmware/test.password \
    REPEAT=${REPEAT:-3} INPUT_RUNS=${INPUT_RUNS:-usb usb-xga} bash tools/vm/vmware-input-e2e.sh
fi
