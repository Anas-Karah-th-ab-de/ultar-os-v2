#!/bin/sh
# Baut ultar-cam statisch für aarch64 (braucht gcc-aarch64-linux-gnu).
set -e
cd "$(dirname "$0")"
mkdir -p kinc/linux
# Samsung-Kern: VIDEO_MAX_PLANES 17 statt 8 -> andere ioctl-Nummern für S_FMT/QBUF/DQBUF
sed 's/define VIDEO_MAX_PLANES .*/define VIDEO_MAX_PLANES 17/' /usr/aarch64-linux-gnu/include/linux/videodev2.h > kinc/linux/videodev2.h
aarch64-linux-gnu-gcc -O2 -static -Ikinc -Istub -o ultar-cam ultar-cam.c -lm
aarch64-linux-gnu-strip ultar-cam
ls -la ultar-cam
