#!/usr/bin/env bash
# Packs what the release server needs into out/release-ci/bundle.tar.gz:
#   vps/ (setup, publish, deploy, rollback scripts, unit, nginx snippet), site/ (website with
#   screenshots), ultar-signd (binary), store-staging/ (unsigned packages + catalogue template).
# Run after ci-release.sh (and shots.sh for screenshots) from the repo root.
set -euo pipefail
cd "$(dirname "$0")/../.."
R=out/release-ci; B=$R/bundle
rm -rf "$B" "$R/bundle.tar.gz"; mkdir -p "$B"
cp -r tools/release/vps "$B/vps"
cp -r tools/release/site "$B/site"
[ -f "$R/shots/v2-desktop-ar.png" ] && cp "$R/shots/v2-desktop-ar.png" "$B/site/img/"
cp "$R/ultar-signd" "$B/ultar-signd"
cp -r "$R/store-staging" "$B/store-staging"
tar -C "$R" -czf "$R/bundle.tar.gz" bundle
sha256sum "$R/bundle.tar.gz"
