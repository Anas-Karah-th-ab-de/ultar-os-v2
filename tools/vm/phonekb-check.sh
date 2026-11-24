#!/usr/bin/env bash
# Checks of the phone/keyboard fix: fmt, clippy -D warnings and tests of the touched crates
# (STEP=checks), the VMware-like real-input gate on a newly built disk (STEP=gate) or on the disk
# built before (STEP=gate-reuse: only the gate script changed), both (STEP=all).
# REPEAT (3) and INPUT_RUNS as vmware-input-e2e.sh; RUNS=usb,usb-xga,usb-hd is INPUT_RUNS with commas
# (a remote shell that mangles spaces and quotes).
set -uo pipefail
cd "$(dirname "$0")/../.."
CRATES="-p ultar-phone -p ultar-compositor -p ultar-surface -p ultar-desktop -p ultar-settingsd -p ultar-displayd"
export REPEAT=${REPEAT:-3}
RUNS=${RUNS:-usb,usb-xga}
export INPUT_RUNS=${INPUT_RUNS:-${RUNS//,/ }}
checks() {
  cargo fmt --all -- --check && echo "FMT OK" || echo "FMT FEHLER"
  cargo clippy $CRATES --all-targets -- -D warnings && echo "CLIPPY OK" || echo "CLIPPY FEHLER"
  cargo test $CRATES 2>&1 | grep -aE "^test result|FAILED|panicked|error(\[|:)" | sort | uniq -c
  echo "TESTS-EXIT ${PIPESTATUS[0]}"
}
case "${STEP:-checks}" in
  checks) checks ;;
  phone) cargo test -p ultar-phone --test window_size 2>&1 | grep -aA4 "panicked" | head -20 ;;
  gate) bash tools/vm/vmware-input-e2e.sh ;;
  gate-reuse)
    VMDK=out/vmware/UltarOS-v2/UltarOS-v2.vmdk VM_PASSWORD_FILE=out/vmware/test.password \
      bash tools/vm/vmware-input-e2e.sh ;;
  all) checks; bash tools/vm/vmware-input-e2e.sh ;;
esac
