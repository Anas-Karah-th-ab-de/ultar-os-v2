#!/usr/bin/env bash
# Builds the driver depot for the installation medium (docs/PLAN.md §5.2):
#   out/depot/<arch>/*.utr + index.toml (+ signature)
# Usage: drivers/depot/build.sh <private-key-file> [arch]
# Test run with a throwaway key: drivers/depot/build.sh --test-key
set -euo pipefail
cd "$(dirname "$0")/../.."
ARCH="${2:-x86_64}"; OUT="out/depot/$ARCH"; UDRV="${UDRV:-cargo run -q -p ultar-driverpkg --bin udrv --}"
export ULTAR_LANG="${ULTAR_LANG:-en}"
rm -rf "$OUT"; mkdir -p "$OUT"
if [ "${1:-}" = "--test-key" ]; then
  KEYS="$(mktemp -d)"; $UDRV keygen > "$KEYS/keygen.txt"
  sed -n 's/^Private key[^:]*: *//p' "$KEYS/keygen.txt" > "$KEYS/priv"; sed -n 's/^Public key: *//p' "$KEYS/keygen.txt" > "$OUT/trusted-test.keys"
  KEY="$KEYS/priv"
else
  KEY="${1:?private key file}"
fi
for manifest in drivers/manifests/*.toml; do
  id="$(basename "$manifest" .toml)"; payload="drivers/payload/$id"
  [ -d "$payload" ] || { payload="$(mktemp -d)"; }   # no payload yet: the driver is still built into the kernel
  $UDRV pack "$manifest" "$payload" "$OUT/$id.utr" "$KEY"
done
$UDRV index "$OUT" "$KEY"
ls -la "$OUT"
