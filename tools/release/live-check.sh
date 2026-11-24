#!/usr/bin/env bash
# Checks the published store with the device code (HTTPS, signature, digests) against the
# store key shipped in rootfs/etc/ultar/trusted-keys/km-code-craft.ed25519.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
ULTAR_LIVE_STORE_KEY=$(cat rootfs/etc/ultar/trusted-keys/km-code-craft.ed25519) \
  cargo test -q -p ultar-store --features online --test live_server -- --ignored --nocapture
