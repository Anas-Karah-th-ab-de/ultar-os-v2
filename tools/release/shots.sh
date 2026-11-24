#!/usr/bin/env bash
# Real screenshots for the website: builds the VMware image of this tree in Arabic and English
# (tools/vm/make-vmware.sh boots it in QEMU and screenshots the desktop) and keeps the PNGs in
# out/release-ci/shots/. Not a release: the images themselves are not kept.
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=out/release-ci/shots; mkdir -p "$OUT"
ACCEL=tcg; [ -w /dev/kvm ] && ACCEL=kvm
for l in ar en; do
  LANG_CODE=$l ACCEL=$ACCEL CHECK=0 bash tools/vm/make-vmware.sh
  cp out/vmware/UltarOS-v2-raw.png "$OUT/v2-desktop-$l.png"
done
ls -la "$OUT"
echo "SHOTS: OK"
