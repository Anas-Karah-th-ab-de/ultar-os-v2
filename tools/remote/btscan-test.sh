#!/usr/bin/env bash
# Bluetooth scan: the crates the scan touches (host stack, service, bus contract, desktop panel).
# Run on a builder through tools/remote/rb.sh: bash tools/remote/btscan-test.sh [extra cargo args]
set -uo pipefail
cargo fmt --all -- --check || echo "FMT-DIFF"
cargo test -p ultar-btd -p ultar-bus "$@" 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)|warning: unused" ; echo "BTD-RC ${PIPESTATUS[0]}"
cargo test -p ultar-desktop -p ultar-settings "$@" 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)" ; echo "DESK-RC ${PIPESTATUS[0]}"
{ cargo clippy -p ultar-btd -p ultar-desktop --all-targets -- -D warnings && cargo clippy -p ultar-settings -- -D warnings; } 2>&1 | grep -E "^(error|warning)" -A6 | head -60; echo "CLIPPY-RC ${PIPESTATUS[0]}"
