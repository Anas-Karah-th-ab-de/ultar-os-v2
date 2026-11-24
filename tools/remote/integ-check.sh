#!/bin/bash
# Integration check: fmt and every workspace test; prints the failed targets and the number of passing ones.
cargo fmt --all --check || { echo "FMT FAILED"; exit 1; }
cargo test --workspace --no-fail-fast > /root/ultar-build/log/integ-ws.txt 2>&1
grep -A8 '^error: .* target' /root/ultar-build/log/integ-ws.txt
echo "passing result lines: $(grep -c 'test result: ok' /root/ultar-build/log/integ-ws.txt)"
