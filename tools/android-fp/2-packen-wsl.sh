#!/bin/bash
cd ~/fp; mountpoint -q mnt || sudo mount -o ro,loop system.raw mnt; cd ~/fp/mnt || exit 1
sudo tar -cf ~/fp/aroot.tar --exclude=system/app --exclude=system/priv-app --exclude=system/framework --exclude=system/fonts --exclude=system/media --exclude=system/hidden --exclude=system/preload --exclude=system/cameradata --exclude=system/tts --exclude=system/apex/com.android.art.release '--exclude=system/apex/com.android.media*' --exclude=system/apex/com.android.extservices.gms --exclude=system/apex/com.android.permission.gms --exclude=system/apex/com.android.mediaprovider --exclude=system/apex/com.android.wifi --exclude=system/apex/com.android.conscrypt --exclude=system/apex/com.android.neuralnetworks --exclude=system/lib system system_ext 2>&1 | tail -3
sudo chown $(id -u) ~/fp/aroot.tar
ls -la ~/fp/aroot.tar
gzip -1 -c ~/fp/aroot.tar > /mnt/c/Users/anask/Documents/UltarFlash/wlan/aroot.tar.gz
ls -la /mnt/c/Users/anask/Documents/UltarFlash/wlan/aroot.tar.gz
