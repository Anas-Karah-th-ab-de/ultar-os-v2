#!/usr/bin/env bash
# crate tests + clippy of the app-bus fix (run by rb.sh in the worktree)
set -o pipefail
P="-p ultar-busd -p ultar-launcher -p ultar-init -p ultar-settings -p ultar-accountd"
case "${1:-test}" in
  test) cargo test $P 2>&1 | grep -E "^error|panicked|FAILED|^---- |error\[|^  *--> " | head -80; cargo test $P 2>&1 | grep -c "test result: ok" ;;
  clippy) cargo clippy $P --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A6 | head -120; echo "clippy exit ${PIPESTATUS[0]}" ;;
  settings) rm -rf /root/shots; ULTAR_CONFORMANCE_OUT=/root/shots cargo test --no-fail-fast -p ultar-settings -p ultar-accountd 2>&1 | grep -E "^test .*FAILED|panicked|keys without|differ|layout problems|^[0-9]+x|test result|error\[|^error|^ +-->" | cut -c1-260 | head -150 ;;
  fmt) cargo fmt --check $P && echo fmt-ok ;;
esac
