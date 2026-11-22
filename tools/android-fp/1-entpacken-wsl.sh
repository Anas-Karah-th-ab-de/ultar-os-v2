#!/bin/bash
set -e
B=/mnt/c/Users/anask/Documents/UltarFlash
W=$HOME/fp; mkdir -p $W; cd $W
which lz4 >/dev/null || sudo apt-get install -y lz4 >/dev/null
if [ ! -f system.raw ]; then
  lz4 -dc $B/a30-RR8M6060BMP/partitions/system.img.lz4 > system.img
  if [ "$(xxd -l4 -p system.img)" = "3aff26ed" ]; then simg2img system.img system.raw && rm system.img; else mv system.img system.raw; fi
fi
ls -la system.raw
mkdir -p mnt; mountpoint -q mnt || sudo mount -o ro,loop system.raw mnt
ls mnt; ls mnt/system 2>/dev/null | head; du -sh mnt/system/lib64 mnt/system/apex 2>/dev/null
cat mnt/system/build.prop 2>/dev/null | grep -E "ro.build.version.release=|ro.build.display.id=" | head -3
