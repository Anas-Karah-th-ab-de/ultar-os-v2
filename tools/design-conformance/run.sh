#!/bin/sh
# Design conformance suite (docs/DESKTOP-SPEC.md §2.2c, §2.2d): fails on every regression
# between the artboards (design/boards/project) and the implementation.
#
#   tools/design-conformance/run.sh [out-dir]
#
# 1. Renders every board headless (Chrome + playwright; skipped with a notice when missing) and
#    checks that design/conformance/states.tsv is up to date with the boards.
# 2. Static checks: icons, tokens, click list, translations, coverage list
#    (accepted findings: design/conformance/known-static.txt).
# 3. Toolkit and compositor against the boards: sizes, colours, focus rings, clicks
#    (accepted differences: design/conformance/known-differences.txt – the list only shrinks).
# With out-dir it also writes the toolkit renders and side-by-side pictures there.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:-${TMPDIR:-/tmp}/design-conformance}
TOOLS="$ROOT/tools/design-conformance"
mkdir -p "$OUT"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

INV=""
CHROME=${CHROME:-$(command -v google-chrome || command -v chromium || command -v chromium-browser || true)}
if [ -n "$CHROME" ] && python3 -c "import playwright, PIL" 2>/dev/null; then
    python3 -I "$TOOLS/board_inventory.py" "$ROOT" "$OUT" --chrome "$CHROME"
    INV="--inventory $OUT/inventory.json"
    # The manifest must match the boards.
    if ! python3 -I "$TOOLS/make_manifest.py" "$ROOT" "$OUT" --check; then
        echo "design/conformance/states.tsv is stale: run tools/design-conformance/make_manifest.py" >&2
        exit 1
    fi
else
    echo "notice: no Chrome/playwright – board rendering and board-side checks skipped" >&2
fi

# shellcheck disable=SC2086
python3 -I "$TOOLS/static_checks.py" "$ROOT" $INV --known "$ROOT/design/conformance/known-static.txt"

cd "$ROOT"
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-ui --test design_conformance --test design_conformance_clicks
cargo test -p ultar-compositor --test design_conformance

if [ -n "$INV" ]; then
    for b in States StatesLight; do
        python3 -I "$TOOLS/compare.py" "$OUT/boards/$b.png" "$OUT/impl/$b-toolkit.png" "$OUT/$b-side.png"
    done
fi
echo "design conformance: OK"
