#!/bin/bash
# Re-runs the test targets that failed in the integration run and prints only the failures.
for t in "-p ultar-settingsd --lib" "-p ultar-busd --bin ultar-busd" "-p ultar-netd --test process" "-p ultar-ui --test widget_snapshots"; do
  echo "##### $t"
  cargo test $t 2>&1 | grep -E -A18 "panicked|^failures:|^error" | head -70
done
