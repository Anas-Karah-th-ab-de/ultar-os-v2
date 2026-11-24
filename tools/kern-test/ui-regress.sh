#!/usr/bin/env bash
# Regressions of the owner's VMware reports that the toolkit, the shell and the launcher must never bring back
# (part of ci.sh, verdict line "UI-REGRESS: ALLES OK"):
#   1. Files, "show as list" hung the window: the list view rebuilt its whole tree on every turn of the loop
#      and lost the press of every click (apps/ultar-files/tests/listview.rs; REAL-INPUT: pointer down, an
#      idle turn of the loop, pointer up; an empty folder and 1 500 entries).
#   2. Quick settings: the Wi-Fi symbol, the tiles with an arrow, the gear, lock and power opened nothing and
#      the sliders said "0%" (platform/ultar-desktop/tests/e2e_quick.rs; REAL-INPUT: the real compositor at
#      1024x768, the real shell, absolute pointer positions and button presses injected like VMware's mouse);
#      the click checks of every quick settings part (tests/clicks.rs, services.rs, wifi.rs).
#   3. The launcher hands a page of settings://<page> to Settings as `--area <page>` (tests/opening.rs).
set -euo pipefail
cd "$(dirname "$0")/../.."
LOG="${UI_REGRESS_LOG:-$(mktemp)}"
fail=0
run() {
  local name=$1; shift
  if "$@" >>"$LOG" 2>&1; then
    echo "ok    $name"
  else
    echo "FAIL  $name"; tail -40 "$LOG"; fail=1
  fi
}
run "Files list view (REAL-INPUT, 1500 entries, empty folder)" cargo test -q -p ultar-files --test listview
run "quick settings by real clicks (REAL-INPUT, compositor 1024x768)" cargo test -q -p ultar-desktop --test e2e_quick
run "quick settings clicks of CLICKS.md" cargo test -q -p ultar-desktop --test clicks --test services --test wifi --test settings_pages
run "launcher: page of a URI as argument" cargo test -q -p ultar-launcher --test opening
[ "$fail" = 0 ] && echo "UI-REGRESS: ALLES OK" || { echo "UI-REGRESS: FEHLER"; exit 1; }
