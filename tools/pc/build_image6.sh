#!/bin/bash
# s11f: Basis s11d + Binaries aus git main bins/a30 (init/shell/net/ctl). Aufruf in WSL.
set -e
SRC=/mnt/c/Users/anask/Documents/UltarFlash; BINS="$SRC/pull/bins/a30"; OUT="$SRC/neu/s11f"; W="$HOME/ultarimg5"
mkdir -p "$OUT"; rm -rf "$W"; mkdir -p "$W"; cd "$W"
simg2img "$SRC/neu/s11d/system.img" raw.img
e2fsck -fy raw.img || true
mkdir -p mnt; sudo mount -o loop raw.img mnt
gzip -dc "$BINS/ultar-shell.gz" > ultar-shell; sudo cp ultar-shell mnt/usr/bin/ultar-shell
sudo cp "$BINS/ultar-init"  mnt/sbin/init
sudo cp "$BINS/ultar-init"  mnt/usr/bin/ultar-init
sudo cp "$BINS/ultar-net"   mnt/usr/bin/ultar-net
sudo cp "$BINS/ultar-ctl"   mnt/usr/bin/ultar-ctl
sudo cp "$BINS/ultar-audio" mnt/usr/bin/ultar-audio
sudo cp "$BINS/ultar-compat" mnt/usr/bin/ultar-compat
for b in ultar-cam ultar-bt ultar-modem ultar-ril; do [ -f "$BINS/$b" ] && sudo cp "$BINS/$b" mnt/usr/bin/$b && sudo chmod 755 mnt/usr/bin/$b; done
# Gesichtserkennung: Programm + Modelle (YuNet, SFace fp16)
[ -f "$BINS/ultar-face.gz" ] && { gzip -dc "$BINS/ultar-face.gz" > ultar-face; sudo cp ultar-face mnt/usr/bin/ultar-face; sudo chmod 755 mnt/usr/bin/ultar-face; }
[ -d "$BINS/face" ] && { sudo mkdir -p mnt/usr/lib/ultar/face; sudo cp "$BINS"/face/*.onnx mnt/usr/lib/ultar/face/; }
sudo chmod 755 mnt/usr/bin/ultar-shell mnt/sbin/init mnt/usr/bin/ultar-init mnt/usr/bin/ultar-net mnt/usr/bin/ultar-ctl mnt/usr/bin/ultar-audio mnt/usr/bin/ultar-compat
for f in mnt/sbin/init mnt/usr/bin/ultar-shell mnt/usr/bin/ultar-net; do echo -n "$f: "; file "$f" | grep -o "statically linked"; done
sync; sudo umount mnt
e2fsck -fy raw.img || true; img2simg raw.img "$OUT/system.img"
cp "$SRC/neu/s11d/vbmeta.img" "$OUT/vbmeta.img"
S=$(sha256sum "$OUT/system.img" | cut -d' ' -f1); V=$(sha256sum "$OUT/vbmeta.img" | cut -d' ' -f1)
{ echo "device=a30"; echo "image.system=system.img"; echo "sha256.system=$S"; echo "image.vbmeta=vbmeta.img"; echo "sha256.vbmeta=$V"; } > "$OUT/ultar-package.txt"
echo FERTIG; ls -l "$OUT"
