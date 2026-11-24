#!/bin/bash
# Remote build on the build server (Beelink): run a command in a clean checkout of a pushed branch.
#
#   bash rb.sh <branch> <command...>
#
# Each branch gets its own worktree under /root/ultar-build/wt/<branch> and its own target dir
# (shared cargo registry). Output goes to /root/ultar-build/log/<branch>.log; the last 60 lines and
# the exit code are printed. Long runs: start with RB_DETACH=1 and poll the log with rb-log.sh.
set -u
branch=${1:?branch}; shift
cmd="$*"
base=/root/ultar-build
repo=$base/ultar-os-v2
safe=$(echo "$branch" | tr '/' '_')
wt=$base/wt/$safe
log=$base/log/$safe.log
mkdir -p "$base/wt" "$base/log"
[ -d "$repo/.git" ] || git clone -q https://github.com/Anas-Karah-th-ab-de/ultar-os-v2 "$repo"
git -C "$repo" fetch -q origin "+refs/heads/$branch:refs/remotes/origin/$branch" || { echo "RB: branch $branch not found"; exit 2; }
if [ ! -d "$wt" ]; then git -C "$repo" worktree add -q --detach "$wt" "origin/$branch"; fi
git -C "$wt" checkout -q -f --detach "origin/$branch" && git -C "$wt" clean -fdq -e target -e out
echo "RB: $branch at $(git -C "$wt" rev-parse --short HEAD)"
run() {
  cd "$wt" && export PATH=/usr/lib/rust-1.97/bin:$PATH RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=${RB_JOBS:-12}
  echo "RB-START $(date -Is) $cmd"
  bash -c "$cmd"
  rc=$?
  echo "RB-EXIT $rc $(date -Is)"
  return $rc
}
if [ "${RB_DETACH:-0}" = 1 ]; then
  (run > "$log" 2>&1 < /dev/null &)
  echo "RB: detached, log $log"
  exit 0
fi
run > "$log" 2>&1
rc=$?
tail -60 "$log"
exit $rc
