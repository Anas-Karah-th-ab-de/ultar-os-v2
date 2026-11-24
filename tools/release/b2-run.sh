#!/usr/bin/env bash
# One detached builder run: release gate, then the website screenshots.
set -euo pipefail
cd "$(dirname "$0")/../.."
bash tools/release/ci-release.sh
bash tools/release/shots.sh
