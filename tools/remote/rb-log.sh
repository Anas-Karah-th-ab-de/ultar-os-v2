#!/bin/bash
# Show the state of a remote build started with rb.sh: running or finished, exit code, last lines.
#   bash rb-log.sh <branch> [lines] [grep-pattern]
branch=${1:?branch}; n=${2:-40}; pat=${3:-}
log=/root/ultar-build/log/$(echo "$branch" | tr '/' '_').log
[ -f "$log" ] || { echo "RB: no log for $branch"; exit 2; }
if grep -q '^RB-EXIT' "$log"; then grep '^RB-EXIT' "$log"; else echo "RB: running ($(wc -l < "$log") lines)"; fi
if [ -n "$pat" ]; then grep -aE "$pat" "$log" | tail -"$n"; else tail -"$n" "$log"; fi
