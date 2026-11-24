#!/bin/bash
# Integration fixes: the settingsd seed count, the busd test, the icon snapshot references.
cargo test -p ultar-settingsd --lib seed 2>&1 | grep -E -A6 "panicked" | head -20
cargo test -p ultar-busd --bin ultar-busd 2>&1 | grep -E "^test result|panicked"
ULTAR_UPDATE_SNAPSHOTS=1 cargo test -p ultar-ui --test widget_snapshots 2>&1 | grep -E "^test result"
git status --porcelain platform/ultar-ui/tests/snapshots | head
tar czf /tmp/integ-snapshots.tgz $(git status --porcelain platform/ultar-ui/tests/snapshots | awk '{print $2}') 2>/dev/null && echo TAR-OK
