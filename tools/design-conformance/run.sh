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
# 3. Toolkit, compositor and phone app against the boards: sizes, colours, focus rings, clicks
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
    if ! python3 -I "$TOOLS/phone_manifest.py" "$ROOT" "$OUT" --check; then
        echo "design/conformance/phone.tsv is stale: run tools/design-conformance/phone_manifest.py" >&2
        exit 1
    fi
    if ! python3 -I "$TOOLS/make_board_manifest.py" "$ROOT" "$OUT" --check; then
        echo "design/conformance/boards/*.tsv are stale: run tools/design-conformance/make_board_manifest.py" >&2
        exit 1
    fi
    # The browser window's board boxes (apps/ultar-web/tests/fixtures/browser-board.tsv).
    python3 -I "$TOOLS/browser_boxes.py" "$ROOT" "$OUT/inventory.json" --check
else
    echo "notice: no Chrome/playwright – board rendering and board-side checks skipped" >&2
fi

# shellcheck disable=SC2086
python3 -I "$TOOLS/static_checks.py" "$ROOT" $INV --known "$ROOT/design/conformance/known-static.txt"

cd "$ROOT"
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-ui --test design_conformance --test design_conformance_clicks --test design_conformance_toasts
cargo test -p ultar-compositor --test design_conformance
# Phone app against the boards PhoneDial, PhoneIncoming, PhoneInCall (design/conformance/phone.tsv).
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-phone --test design_conformance
# product surfaces: boards/*.tsv against the apps (ratchet design/conformance/known-boards.txt)
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-settings --test design_conformance --test clicks --test access
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-installer-ui --test design_conformance --test clicks --test safety --test engine --test access
# Files app against its boards.
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-files --test design_conformance --test clicks
ULTAR_CONFORMANCE_OUT="$OUT/impl" cargo test -p ultar-web --test design_conformance --test clicks
# Continue on another device: boards MeshSend, MeshIncoming, MeshStates (desktop) and
# CompactMeshSend, CompactMeshIncoming (phone), one click test per mesh.* action, and the
# flow between two real mesh services (docs/integration/mesh-handoff.md).
cargo test -p ultar-desktop --test mesh_conformance --test mesh_clicks --test mesh
cargo test -p ultar-mobile --test mesh_conformance --test mesh_e2e --test clicks --test dialog_clicks
cargo test -p ultar-mobile --test handoff_flow
# The Ultar Store app: screens per board (Store, SurfaceStates, CompactStore,
# CompactStoreApp) in ar+en and dark+light, and one click test per store.*
# action of docs/ui/CLICKS.md (design/conformance/coverage.toml names them).
cargo test -p ultar-store --test screens --test clicks --test flow --test transitions --test transitions2 --test logic

if [ -n "$INV" ]; then
    for b in States StatesLight; do
        python3 -I "$TOOLS/compare.py" "$OUT/boards/$b.png" "$OUT/impl/$b-toolkit.png" "$OUT/$b-side.png"
    done
fi
echo "design conformance: OK"
