#!/bin/bash
# Puts the previous website back (the newest /opt/ultar/web.old-*).
set -euo pipefail
OLD=$(ls -1d /opt/ultar/web.old-* 2>/dev/null | sort | tail -1)
[ -n "$OLD" ] || { echo "ROLLBACK-WEB: nothing to roll back to"; exit 1; }
TS=$(date -u +%Y%m%d-%H%M%S)
mv /opt/ultar/web "/opt/ultar/web.bad-$TS" && mv "$OLD" /opt/ultar/web
echo "ROLLBACK-WEB: $OLD is live again (replaced site kept as /opt/ultar/web.bad-$TS)"
