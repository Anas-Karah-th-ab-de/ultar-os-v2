#!/bin/bash
# Release server: signs the packages of a staging directory (from build-apps.sh) through the
# signing service, fills sizes and digests into the catalogue, signs it, and switches
# https://ultar.code-craft.tech/store/v1/ to the new set atomically (symlink swap).
# Usage: publish-store.sh <staging-dir>
# Rollback: store-rollback.sh (points v1 back to the previous set).
set -euo pipefail
IN=$(realpath "${1:?staging dir}")
SOCK=/run/ultar-sign/sign.sock
SIGN=/usr/local/lib/ultar-sign/ultar-signd
BASE=/opt/ultar/store
WEB=/opt/ultar/web/store
TS=$(date -u +%Y%m%d-%H%M%S)
NEW=$BASE/releases/$TS
fail() { echo "PUBLISH-STORE: ERROR ($*)" >&2; rm -rf "$NEW"; exit 1; }
[ -S "$SOCK" ] || fail "signing service not running"
[ -f "$IN/apps.tsv" ] && [ -f "$IN/ultar-catalog.in.toml" ] || fail "staging incomplete"
(cd "$IN" && sha256sum -c --quiet SHA256SUMS) || fail "staging damaged in transit"
mkdir -p "$NEW"
cp "$IN/ultar-catalog.in.toml" "$NEW/ultar-catalog.toml.in"
while IFS=$'\t' read -r id ver role stem; do
  case "$role" in system|store) ;; *) fail "role of $id" ;; esac
  "$SIGN" sign-uap "$SOCK" "$role" "$IN/$stem.manifest.toml" "$IN/$stem.payload.tar.gz" "$NEW/$stem.uap" >/dev/null \
    || fail "signing $stem"
  size=$(stat -c %s "$NEW/$stem.uap"); sum=$(sha256sum "$NEW/$stem.uap" | cut -d' ' -f1)
  sed -i -e "s|@SIZE:$stem.uap@|$size|g" -e "s|@SHA256:$stem.uap@|$sum|g" "$NEW/ultar-catalog.toml.in"
  echo "PUBLISH-STORE: $stem.uap $size $sum ($role)"
done <"$IN/apps.tsv"
grep -q '@SIZE:\|@SHA256:' "$NEW/ultar-catalog.toml.in" && fail "unfilled placeholders"
mv "$NEW/ultar-catalog.toml.in" "$NEW/ultar-catalog.toml"
"$SIGN" sign-catalog "$SOCK" "$NEW/ultar-catalog.toml" >/dev/null || fail "signing catalogue"
# Public keys next to the catalogue (devices get them from the image; these are for checking).
mkdir -p "$NEW/keys"
"$SIGN" pubkey "$SOCK" store >"$NEW/keys/km-code-craft.ed25519"
"$SIGN" pubkey "$SOCK" system >"$NEW/keys/system-km-code-craft.ed25519"
chmod -R a+rX "$NEW"; chmod 0644 "$NEW"/* "$NEW"/keys/*
mkdir -p "$WEB"
ln -sfn "$NEW" "$WEB/v1.new" && mv -T "$WEB/v1.new" "$WEB/v1"
echo "$TS" >>"$BASE/published.log"
# keep the last 5 sets
ls -1d "$BASE"/releases/* | sort | head -n -5 | while read -r old; do [ "$(readlink -f "$WEB/v1")" = "$old" ] || rm -rf "$old"; done
echo "PUBLISH-STORE: OK $TS -> $WEB/v1"
