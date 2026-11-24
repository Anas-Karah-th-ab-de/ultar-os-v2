#!/usr/bin/env bash
# Crate tests, clippy and the kernel run of the Settings wiring (run by rb.sh in the worktree).
#   test | one <package> <filter> | clippy | fmt | check | e2e | gate | vmware
set -o pipefail
P="-p ultar-settings -p ultar-settingsd -p ultar-displayd -p ultar-surface -p ultar-compositor -p ultar-desktop -p ultar-volumed -p ultar-bus -p settings-e2e"
case "${1:-test}" in
  test) cargo test --no-fail-fast $P 2>&1 | grep -E "^test .*FAILED|panicked|^error|error\[|^ +--> |^---- |test result: FAILED|keys without|differ|layout problems" | cut -c1-300 | head -150
        echo "ok-results $(cargo test --no-fail-fast $P 2>&1 | grep -c 'test result: ok')" ;;
  one) cargo test -p "$2" -- "$3" 2>&1 | grep -vE "^test .* ok$|Compiling|Finished|Running|^$|running 0 tests|0 passed" | cut -c1-2500 | tail -60 ;;
  clippy) cargo clippy $P --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A7 | head -150; echo "clippy exit ${PIPESTATUS[0]}" ;;
  fmt) cargo fmt --check --all && echo fmt-ok ;;
  check) cargo run -q --locked -p ultar-check -- all --lang en 2>&1 | grep -v "warning:" | grep -E "settings-e2e|ultar-settings|ultar-displayd|ultar-compositor|ultar-desktop|ultar-volumed|ultar-surface|ultar-bus/|^ultar-check" | tail -40 ;;
  e2e) VERBOSE=1 bash tools/kern-test/settings-e2e.sh ;;
  gate) bash tools/kern-test/ci.sh 2>&1 | grep -aE "^ok |^FAIL|ALLES OK|FEHLER" | tail -80 ;;
  vmware) bash tools/vm/vmware-input-e2e.sh 2>&1 | grep -aE "VMWARE|FEHLER|FAIL|ALLES OK|settings|failures|problem" | tail -80 ;;
esac
