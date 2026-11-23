#!/bin/sh
# A/B frame benchmark of platform/ultar-ui against an older commit (default a5eb384, the
# toolkit before the backdrop rework of wave 5).
#   tools/ui-bench/ab.sh [REF] [ROUNDS]
# Builds tests/ab_bench.rs in this tree and in REF (release), runs both ROUNDS times
# interleaved (default 3), prints the minimum per measurement and the ratio new/old, and
# fails when a measurement is more than 20 % (+0.2 ms, the noise of a shared machine) slower than in REF.
# Waits for a 1-minute load below 1.5 first (at most 10 minutes): measure on a quiet machine.
set -eu
REF=${1:-a5eb384}
ROUNDS=${2:-3}
ROOT=${AB_ROOT:-$(git rev-parse --show-toplevel)}
GIT=${AB_GIT:-$ROOT}   # a clone with the history of REF
WORK=${AB_WORK:-$(mktemp -d)}
OLD=$WORK/old
mkdir -p "$OLD"
git -C "$GIT" archive "$REF" | tar -x -C "$OLD"
cp "$ROOT/platform/ultar-ui/tests/ab_bench.rs" "$OLD/platform/ultar-ui/tests/ab_bench.rs"
export CARGO_INCREMENTAL=0
build() { # dir target
  (cd "$1/platform/ultar-ui" && CARGO_TARGET_DIR=$2 ULTAR_TOKENS=$1/design/tokens.toml \
    cargo test --release --test ab_bench --no-run -q)
}
run() { # dir target label
  (cd "$1/platform/ultar-ui" && CARGO_TARGET_DIR=$2 ULTAR_TOKENS=$1/design/tokens.toml \
    cargo test --release --test ab_bench -q -- --ignored --nocapture 2>/dev/null) |
    awk -v l="$3" '/^AB /{print l, $2, $3}'
}
build "$OLD" "$WORK/target-old"
build "$ROOT" "$WORK/target-new"
for i in $(seq 1 60); do
  l=$(cut -d' ' -f1 /proc/loadavg)
  awk "BEGIN{exit !($l < 1.5)}" && break
  sleep 10
done
echo "load: $(cat /proc/loadavg)"
: > "$WORK/ab.log"
for i in $(seq 1 "$ROUNDS"); do
  run "$OLD" "$WORK/target-old" old >> "$WORK/ab.log"
  run "$ROOT" "$WORK/target-new" new >> "$WORK/ab.log"
done
awk '
  { k=$2; v=$3+0; if ($1=="old") { if (!(k in o) || v<o[k]) o[k]=v } else { if (!(k in n) || v<n[k]) n[k]=v } }
  END {
    bad=0
    for (k in n) {
      r = (k in o && o[k]>0) ? n[k]/o[k] : 0
      printf "%-22s old %8.3f ms  new %8.3f ms  ratio %.2f\n", k, o[k], n[k], r
      if (k in o && n[k] > o[k]*1.2 + 0.2) { bad=1; print "  REGRESSION: " k }
    }
    exit bad
  }' "$WORK/ab.log"
