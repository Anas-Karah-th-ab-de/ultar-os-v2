#!/bin/sh
# End to end of Windows programs on Linux (root, real cells): stage P4 of docs/FREMD-APPS.md,
# system/core/ultar-launcher/tests/foreign_windows_e2e.rs with the Linux area of
# tools/foreign/make-linux-runtime.sh (component "windows"; Xvfb/xwd for the screenshot are
# test-only extras, never shipped) and two real, free Windows installers.
#
#   tools/foreign/windows-e2e.sh [OUT]       (as root; never on the VPS)
#
# Final line on success: FOREIGN-WINDOWS-E2E: ALLES OK
set -eu
fail() { echo "windows-e2e: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root (real cells)"
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
OUT=${1:-$REPO/out/foreign-windows}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
# a finished build leaves its image checksum, newer than the area's mark
built() {
  for s in "$OUT"/linux-*.ext4.sha256; do
    [ -f "$s" ] && [ ! "$OUT/linux/etc/ultar-runtime" -nt "$s" ] && return 0
  done
  return 1
}
# the area carries the app wrapper as built: a changed wrapper needs a new area
wrapper_same() {
  tr -d '\r' <"$HERE/wine/ultar-wine-app" | cmp -s - "$OUT/linux/usr/lib/ultar/wine/ultar-wine-app"
}
if ! grep -q '^components=.*windows' "$OUT/linux/etc/ultar-runtime" 2>/dev/null \
   || [ ! -x "$OUT/linux/usr/bin/Xvfb" ] || ! built || ! wrapper_same; then
  COMPONENTS=windows TEST_EXTRA="xvfb x11-apps" "$HERE/make-linux-runtime.sh" "$OUT"
fi
# the installers (official downloads, pinned by their SHA-256)
fetch() { # url file sha256
  if [ ! -f "$OUT/$2" ]; then
    curl -fsSL --max-time 600 -o "$OUT/$2.part" "$1" || fail "download $1"
    mv "$OUT/$2.part" "$OUT/$2"
  fi
  got=$(sha256sum "$OUT/$2" | cut -d' ' -f1)
  echo "windows-e2e: $2 sha256 $got"
  [ -z "$3" ] || [ "$got" = "$3" ] || fail "$2: SHA-256 $got, expected $3"
}
fetch https://github.com/notepad-plus-plus/notepad-plus-plus/releases/download/v8.7.1/npp.8.7.1.Installer.x64.exe \
  npp.8.7.1.Installer.x64.exe \
  "${NPP_SHA256-aa130d151859eccc8b4cc535ae756682069d626087e8c56a35b630df4b5f0024}"
fetch https://www.7-zip.org/a/7z2409-x64.msi 7z2409-x64.msi \
  "${SEVENZIP_SHA256-ec6af1ea0367d16dde6639a89a080a524cebc4d4bedfe00ed0cac4b865a918d8}"
cd "$REPO"
cargo build -q -p ultar-foreign --bin ultar-foreign
BIN=${CARGO_TARGET_DIR:-$REPO/target}/debug/ultar-foreign
SHOTS="$OUT/shots"
rm -rf "$SHOTS"; mkdir -p "$SHOTS"
LOG=$(mktemp)
ULTAR_FOREIGN_RUNTIME="$OUT/linux" ULTAR_FOREIGN_BIN="$BIN" \
ULTAR_WINDOWS_EXE="$OUT/npp.8.7.1.Installer.x64.exe" ULTAR_WINDOWS_MSI="$OUT/7z2409-x64.msi" \
ULTAR_WINDOWS_SHOTS="$SHOTS" \
  cargo test -q -p ultar-launcher --test foreign_windows_e2e -- --nocapture --test-threads 1 >"$LOG" 2>&1 \
  || { tail -80 "$LOG"; fail "test failed"; }
grep '^FOREIGN' "$LOG" | grep -v '^FOREIGN-WINDOWS-E2E'

# the screenshots as PNG (XWD, Z pixmap, 24 or 32 bits per pixel) for people to look at
for x in "$SHOTS"/*.xwd; do
  [ -f "$x" ] || continue
  python3 - "$x" "${x%.xwd}.png" <<'PY'
import struct, sys, zlib
d = open(sys.argv[1], 'rb').read()
f = struct.unpack('>25I', d[:100])
header, width, height, bpp, line, ncolors = f[0], f[4], f[5], f[11], f[12], f[19]
n = bpp // 8
px = d[header + ncolors * 12:]
rows = b''.join(b'\0' + bytes(c for i in range(width) for c in (px[y*line+n*i+2], px[y*line+n*i+1], px[y*line+n*i])) for y in range(height))
def chunk(t, b): return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t + b))
open(sys.argv[2], 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows, 6)) + chunk(b'IEND', b''))
PY
  echo "windows-e2e: screenshot ${x%.xwd}.png"
done
grep -q '^FOREIGN-WINDOWS-E2E: ALLES OK$' "$LOG" || { tail -40 "$LOG"; fail "the test did not run (skipped?)"; }
rm -f "$LOG"
echo "FOREIGN-WINDOWS-E2E: ALLES OK"
