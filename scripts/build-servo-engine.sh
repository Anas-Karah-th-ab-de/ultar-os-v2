#!/bin/bash
# Builds the Servo engine host of the browser (external/servo-host → ultar-web-engine) on a
# builder (16 cores; Servo with SpiderMonkey needs clang, python3 and about 10 GB in the target
# folder). Usage: scripts/build-servo-engine.sh [--media] [--shot ADDRESS OUT.png]
#   --media  with sound and video (GStreamer, servo-media)
#   --cell   with --shot: the host runs in its page cell (no network of its own); the Tor client
#            serves it through a Unix socket (`ultar-tor-port --unix`). Needs root (namespaces, memory group)
#   --shot   afterwards one page through the whole chain (host → Tor), saved as PNG; the Tor port
#            comes from platform/ultar-tor's `ultar-tor-port` (built in the main workspace: Arti is
#            never linked next to Servo)
# Output: $TARGET/release/ultar-web-engine (default TARGET=/root/ultar-servo/target).
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
target=${TARGET:-/root/ultar-servo/target}
export CARGO_NET_RETRY=20 CARGO_HTTP_TIMEOUT=120
features=()
shot=()
cell=0
while [ $# -gt 0 ]; do
  case "$1" in
    --media) features=(--features media); shift ;;
    --shot) shot=("$2" "$3"); shift 3 ;;
    --cell) cell=1; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
cd "$here/external/servo-host"
CARGO_TARGET_DIR=$target cargo build --release --bin ultar-web-engine --example shot "${features[@]}"
engine="$target/release/ultar-web-engine"
ls -l "$engine"
if [ ${#shot[@]} -eq 2 ]; then
  (cd "$here" && CARGO_TARGET_DIR=$target-main cargo build --release -p ultar-tor --bin ultar-tor-port)
  urls=$(mktemp)
  # the port runs as long as its standard input stays open
  unix=()
  lines=2
  if [ "$cell" = 1 ]; then
    sockdir=$(mktemp -d)
    unix=(--unix "$sockdir/tor.sock")
    lines=3
  fi
  sleep 600 | "$target-main/release/ultar-tor-port" "${unix[@]}" > "$urls" &
  port=$!
  for _ in $(seq 1 50); do [ "$(wc -l < "$urls")" -ge "$lines" ] && break; sleep 0.2; done
  proxy=$(head -1 "$urls")
  if [ "$cell" = 1 ]; then proxy=$(sed -n 3p "$urls"); fi
  status=0
  "$target/release/examples/shot" "$engine" "$proxy" "${shot[0]}" "${shot[1]}" || status=$?
  kill "$port" 2>/dev/null || true
  exit $status
fi
