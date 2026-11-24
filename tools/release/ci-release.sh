#!/usr/bin/env bash
# Builder gate of the release tooling: signing service (tests + release binary), the store and
# App-Builder (fmt, clippy, tests), then the store packages (build-apps.sh).
# Output: out/release-ci/{ultar-signd, store-staging/}. Usage from the repo root (B2):
#   bash tools/release/ci-release.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
OUT=out/release-ci; rm -rf "$OUT"; mkdir -p "$OUT"
echo "== signd"
(cd server/ultar-signd && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test -q \
  && cargo build --release -q)
SIGND_DIR=${CARGO_TARGET_DIR:-server/ultar-signd/target}
cp "$(ls -t "$SIGND_DIR"/release/ultar-signd server/ultar-signd/target/release/ultar-signd 2>/dev/null | head -1)" "$OUT/ultar-signd"
echo "== store + appbuilder"
cargo fmt -p ultar-store -p ultar-appbuilder --check
cargo clippy -q -p ultar-store -p ultar-appbuilder --all-targets --features ultar-store/gui,ultar-appbuilder/gui -- -D warnings
for i in 1 2 3; do cargo test -q -p ultar-store -p ultar-appbuilder --features ultar-store/online 2>&1 | grep -E "test result|FAILED|panicked"; done
echo "== packages"
bash tools/release/build-apps.sh "$OUT/store-staging"
echo "RELEASE-CI: OK"
