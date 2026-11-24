#!/bin/bash
# Points https://ultar.code-craft.tech/store/v1/ back to the previous published set.
set -euo pipefail
BASE=/opt/ultar/store; WEB=/opt/ultar/web/store
cur=$(readlink -f "$WEB/v1")
prev=$(ls -1d "$BASE"/releases/* | sort | grep -v -x "$cur" | tail -1)
[ -n "$prev" ] || { echo "STORE-ROLLBACK: no previous set"; exit 1; }
ln -sfn "$prev" "$WEB/v1.new" && mv -T "$WEB/v1.new" "$WEB/v1"
echo "STORE-ROLLBACK: $cur -> $prev"
