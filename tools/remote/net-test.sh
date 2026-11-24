#!/usr/bin/env bash
# Checks of build/net on a builder (tools/remote/rb.sh build/net 'bash tools/remote/net-test.sh [all|rules]'):
# format, repository rules, clippy and tests of the crates the network work touches, the catalogs.
# `rules`: the findings of ultar-check that are new against origin/build/appbus (the branch's base).
set -uo pipefail
fail=0
step() { echo "== $*"; "$@" || { echo "FAIL: $*"; fail=1; }; }
P="-p ultar-bus -p ultar-hal-linux -p ultar-netd -p ultar-settings -p ultar-desktop -p ultar-powerd"
rules() {
  local base=/tmp/net-rules-base
  git fetch -q origin build/appbus
  git worktree remove -f "$base" 2>/dev/null; git worktree add -q --detach "$base" FETCH_HEAD
  cargo build -q --locked -p ultar-check
  local bin="${CARGO_TARGET_DIR:-target}/debug/ultar-check"; bin=$(readlink -f "$bin")
  "$bin" all --lang en 2>&1 | grep -E "^[a-z].*:[0-9]+: " | sed 's/:[0-9]*:/:/' | sort > /tmp/net-rules-head.txt
  (cd "$base" && "$bin" all --lang en 2>&1 | grep -E "^[a-z].*:[0-9]+: " | sed 's/:[0-9]*:/:/' | sort > /tmp/net-rules-base.txt)
  git worktree remove -f "$base"
  echo "new findings:"; comm -13 /tmp/net-rules-base.txt /tmp/net-rules-head.txt
  echo "gone findings:"; comm -23 /tmp/net-rules-base.txt /tmp/net-rules-head.txt
}
case "${1:-all}" in
  rules) rules ;;
  # clippy without the settings app's test targets (their i18n test has a finding of build/appbus)
  clippy)
    step cargo clippy -q --locked -p ultar-bus -p ultar-hal-linux -p ultar-netd -p ultar-desktop -p ultar-powerd --all-targets -- -D warnings
    step cargo clippy -q --locked -p ultar-settings --lib --bins -- -D warnings
    exit $fail ;;
  # a test target on the base branch (is a failure ours?): base-test <package> <test target>
  base-test)
    git fetch -q origin build/appbus; git worktree remove -f /tmp/net-base-test 2>/dev/null
    git worktree add -q --detach /tmp/net-base-test FETCH_HEAD
    (cd /tmp/net-base-test && cargo test -q --locked -p "$2" --test "$3" 2>&1 | tail -25)
    git worktree remove -f /tmp/net-base-test ;;
  # the network service's process tests on this host and the settings app's clippy with its tests
  host)
    cargo test -q --locked -p ultar-netd 2>&1 | grep -E "test result|FAILED|panicked"
    cargo clippy -q --locked -p ultar-settings --all-targets -- -D warnings && echo SETTINGS-CLIPPY-OK
    exit 0 ;;
  tests)
    cargo test --no-fail-fast --locked $P 2>&1 | grep -E "^test result|FAILED|panicked|^error" | sort | uniq -c
    exit 0 ;;
  all)
    step cargo fmt --all --check
    step cargo clippy -q --locked $P --all-targets -- -D warnings
    step cargo test -q --locked $P
    step cargo run -q --locked -p ultar-i18n --bin ultar-i18n-check -- system/core/ultar-i18n/locales
    rules
    echo "NET-TEST: $([ $fail = 0 ] && echo ALLES OK || echo FEHLER)"
    exit $fail ;;
esac
