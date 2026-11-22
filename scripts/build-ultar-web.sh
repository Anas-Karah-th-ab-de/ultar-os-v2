#!/bin/bash
# Ultar Web: Laufzeit-Bündel (ohne Ubuntu) + Rust-Kern bauen und zu einem Paket zusammensetzen.
#
#   scripts/build-ultar-web.sh <buildroot-ausgabe> [ziel-arch]
#
# Voraussetzung: Buildroot-Lauf mit br-ext (WPE WebKit 2.54, WPEPlatform) ist fertig, z. B.
#   ~/ultar-web-build/out-aarch64.  Ergebnis: out/ultar-web-<arch>/ (Bündel) und out/ultar-web-<arch>.tar.zst
#
# Das Bündel liegt auf dem Gerät unter /usr/lib/ultar/web. Alle Programme darin benutzen den mitgelieferten
# Lader (lib/ld-linux-*.so.1) und finden ihre Bibliotheken über RUNPATH – das übrige System bleibt statisch.
set -euo pipefail
BR=${1:?Buildroot-Ausgabe angeben}
ARCH=${2:-aarch64}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=/usr/lib/ultar/web
OUT=$ROOT/out/ultar-web-$ARCH
T=$BR/target
S=$BR/staging
HOSTBIN=$BR/host/bin
PATCHELF=$HOSTBIN/patchelf
CC=$(ls $HOSTBIN/*-buildroot-linux-gnu-gcc | head -1)
TRIPLE=$ARCH-unknown-linux-gnu
export PATH=$HOME/.cargo/bin:$PATH

echo "== Rust-Kern ($TRIPLE)"
cd "$ROOT"
TV=$(echo "$TRIPLE" | tr 'a-z-' 'A-Z_')
export "CARGO_TARGET_${TV}_LINKER=$CC"
export ULTAR_WEB_SYSROOT=$S
export RUSTFLAGS="-C link-arg=--sysroot=$S -C target-cpu=${ULTAR_WEB_CPU:-cortex-a73}"
cargo build -p ultar-web --release --target "$TRIPLE" --bin ultar-web
unset RUSTFLAGS
env -u ULTAR_WEB_SYSROOT cargo build -p ultar-web --release --bin ultar-web-filter

echo "== Bündel zusammensetzen"
rm -rf "$OUT"; mkdir -p "$OUT"/{bin,lib,libexec,share/ultar-web,share/fonts,etc/fonts,etc/ssl/certs}
cp target/$TRIPLE/release/ultar-web "$OUT/bin/"
# Bibliotheken (ohne statische Archive, Entwicklerdateien)
for d in "$T/lib" "$T/usr/lib"; do
  (cd "$d" && find . -maxdepth 1 \( -name '*.so*' \) -exec cp -a {} "$OUT/lib/" \;)
done
for sub in gio gstreamer-1.0 gconv wpe-webkit-2.0; do
  [ -d "$T/usr/lib/$sub" ] && cp -a "$T/usr/lib/$sub" "$OUT/lib/"
done
[ -d "$T/usr/lib/dri" ] && cp -a "$T/usr/lib/dri" "$OUT/lib/" || true
cp -a "$T/usr/libexec/wpe-webkit-2.0" "$OUT/libexec/"
[ -d "$T/usr/libexec/gstreamer-1.0" ] && cp -a "$T/usr/libexec/gstreamer-1.0" "$OUT/libexec/"
mkdir -p "$OUT/share/X11" && cp -a "$T/usr/share/X11/xkb" "$OUT/share/X11/"
cp -a "$T/usr/share/fonts/." "$OUT/share/fonts/" 2>/dev/null || true
cp "$ROOT"/assets/fonts/*.ttf "$OUT/share/fonts/"
cat "$T"/etc/ssl/certs/*.pem > "$OUT/etc/ssl/certs/ca-certificates.crt" 2>/dev/null || cp "$T/etc/ssl/certs/ca-certificates.crt" "$OUT/etc/ssl/certs/"
cat > "$OUT/etc/fonts/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$DEST/share/fonts</dir>
  <cachedir>/data/ultar/web/fontcache</cachedir>
  <cachedir prefix="xdg">fontconfig</cachedir>
  <alias><family>sans-serif</family><prefer><family>Noto Sans</family><family>Noto Sans Arabic</family><family>DejaVu Sans</family></prefer></alias>
  <alias><family>serif</family><prefer><family>Noto Sans</family><family>DejaVu Serif</family></prefer></alias>
  <alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family></prefer></alias>
  <match target="font"><edit name="antialias" mode="assign"><bool>true</bool></edit><edit name="hintstyle" mode="assign"><const>hintslight</const></edit><edit name="rgba" mode="assign"><const>none</const></edit></match>
</fontconfig>
EOF
# Hardware-Videodekoder (v4l2) getrennt: er wird nur zugeschaltet, wenn das Gerät ihn ohne Hänger öffnen kann
mkdir -p "$OUT/lib/gstreamer-hw"
[ -f "$OUT/lib/gstreamer-1.0/libgstvideo4linux2.so" ] && mv "$OUT/lib/gstreamer-1.0/libgstvideo4linux2.so" "$OUT/lib/gstreamer-hw/"
[ -f "$OUT/lib/gstreamer-1.0/libgstv4l2codecs.so" ] && mv "$OUT/lib/gstreamer-1.0/libgstv4l2codecs.so" "$OUT/lib/gstreamer-hw/"

echo "== Werbe-/Trackingfilter"
F=$ROOT/out/filterlisten; mkdir -p "$F"
for l in easylist easyprivacy; do
  [ -s "$F/$l.txt" ] || curl -fsSL "https://easylist.to/easylist/$l.txt" -o "$F/$l.txt" || echo "Liste $l nicht ladbar"
done
LISTS=$(ls "$F"/*.txt 2>/dev/null || true)
if [ -n "$LISTS" ]; then
  target/release/ultar-web-filter "$OUT/share/ultar-web/filters.json" $LISTS
else
  echo '[{"trigger":{"url-filter":"^[a-z]+://([^/]*\.)?doubleclick\.net"},"action":{"type":"block"}}]' > "$OUT/share/ultar-web/filters.json"; echo 1 > "$OUT/share/ultar-web/filters.count"
fi
sha256sum "$OUT/share/ultar-web/filters.json" | cut -c1-12 > "$OUT/share/ultar-web/filters.version"

echo "== Lader und Suchpfade setzen"
LDSO=$(cd "$OUT/lib" && ls ld-linux-*.so.* | head -1)
find "$OUT" -type f | while read -r f; do
  head -c4 "$f" 2>/dev/null | grep -q $'\x7fELF' || continue
  if $PATCHELF --print-interpreter "$f" >/dev/null 2>&1; then
    $PATCHELF --set-interpreter "$DEST/lib/$LDSO" "$f"
  fi
  case "$f" in */lib/$LDSO) continue ;; esac
  $PATCHELF --set-rpath "$DEST/lib" "$f" 2>/dev/null || true
done
# Ablageort für Programme, die WebKit direkt sucht
echo "$DEST" > "$OUT/share/ultar-web/prefix"
du -sh "$OUT"
echo "== Paket"
tar -C "$OUT" -cf - . | zstd -19 -T0 -q -o "$ROOT/out/ultar-web-$ARCH.tar.zst" -f
# Geräte ohne zstd (A30: nur gzip/tar): zusätzlich .tar.gz
tar -C "$OUT" -czf "$ROOT/out/ultar-web-$ARCH.tar.gz" .
ls -la "$ROOT/out/ultar-web-$ARCH.tar.zst" "$ROOT/out/ultar-web-$ARCH.tar.gz"
