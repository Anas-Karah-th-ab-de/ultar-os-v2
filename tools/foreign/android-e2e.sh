#!/bin/sh
# End to end of the Android apps on Linux (root, real cells):
# system/core/ultar-launcher/tests/foreign_android_e2e.rs with the Linux area plus the
# component "Android apps" of tools/foreign/make-android-runtime.sh (built here when OUT
# has none), a real test app of ATL's test set and a headless Weston as the display.
#
#   tools/foreign/android-e2e.sh [OUT]       (as root; never on the VPS)
#
#   OUT   default out/foreign-android (OUT/linux: the area with the component)
#
# Environment:
#   LINUX_AREA   the Linux area for make-android-runtime.sh (default out/foreign-linux/linux,
#                built with make-linux-runtime.sh when missing)
#   TEST_APKS    a checkout of atl_test_apks (default OUT/src/atl_test_apks, fetched)
#
# The test app is Gravity Defied (org.happysanta.gd, GPL-2.0, a Java app, APK signature v2),
# checked against its SHA-256 below. Its variants are made here: only ARM code (an ARM
# library added), one byte of its code changed, signed again by another (test) signer; the
# refusals of JAR-only signing and of an unsigned file use two other apps of the set.
#
# Output: OUT/e2e/window.png (the app's window) and the final line
#   FOREIGN-ANDROID-E2E: ALLES OK
set -eu
fail() { echo "android-e2e: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root (real cells)"
case " $(hostname -I 2>/dev/null) " in *" 194.62.29.64 "*) fail "not on the VPS" ;; esac
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
OUT=${1:-$REPO/out/foreign-android}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

TEST_APKS_COMMIT=061e32a3172c8167b1746768d098f0e62d8f564b
APP_FILE=org.happysanta.gd_29.apk
APP_SHA256=0786257ce7e5bd3a3777372bf75fb2ade287ac892fffe16bfaf3ca91b76dd675
APP="org.happysanta.gd foreign.apk-gd Gravity Defied"

# --- the area with the component -------------------------------------------------------------
if ! grep -q '^android=' "$OUT/linux/etc/ultar-runtime" 2>/dev/null; then
  AREA=${LINUX_AREA:-$REPO/out/foreign-linux/linux}
  [ -f "$AREA/etc/ultar-runtime" ] || "$HERE/make-linux-runtime.sh" "$(dirname "$AREA")"
  "$HERE/make-android-runtime.sh" "$AREA" "$OUT"
fi

# --- the test apps -------------------------------------------------------------------------------
TEST_APKS=${TEST_APKS:-$OUT/src/atl_test_apks}
if [ ! -f "$TEST_APKS/$APP_FILE" ]; then
  git clone -q https://gitlab.com/android_translation_layer/atl_test_apks.git "$TEST_APKS"
  git -C "$TEST_APKS" checkout -q "$TEST_APKS_COMMIT"
fi
echo "$APP_SHA256  $TEST_APKS/$APP_FILE" | sha256sum -c --quiet - || fail "$APP_FILE is not the expected file"
W="$OUT/e2e"
rm -rf "$W"
mkdir -p "$W/apks" "$W/arm/lib/arm64-v8a"
cp "$TEST_APKS/$APP_FILE" "$W/apks/app.apk"
cp "$TEST_APKS/demo_app.apk" "$W/apks/old.apk"     # JAR signing only
cp "$TEST_APKS/gles3jni.apk" "$W/apks/unsigned.apk"
cp "$W/apks/app.apk" "$W/apks/arm.apk"
printf '\177ELF\002\001\001' >"$W/arm/lib/arm64-v8a/libgame.so"
(cd "$W/arm" && zip -q "$W/apks/arm.apk" lib/arm64-v8a/libgame.so)
cd "$REPO"
cargo build -q -p ultar-foreign --bin ultar-foreign
cargo build -q -p ultar-foreign --features test-sign --example apk-variant
T=${CARGO_TARGET_DIR:-$REPO/target}/debug
"$T/examples/apk-variant" tamper "$W/apks/app.apk" "$W/apks/tampered.apk" classes.dex
"$T/examples/apk-variant" resign "$W/apks/app.apk" "$W/apks/other.apk" "Ultar Test B"

# --- the display: a headless Weston (kiosk shell: an empty desktop is one colour) -----------
WESTON=$(command -v weston || true)
if [ -z "$WESTON" ]; then
  DEBIAN_FRONTEND=noninteractive apt-get install -y -q weston >/dev/null
  WESTON=$(command -v weston)
fi
SHOOTER=$(command -v weston-screenshooter || ls /usr/libexec/weston-screenshooter 2>/dev/null || true)
[ -n "$SHOOTER" ] || fail "weston-screenshooter missing"
cat >"$W/weston" <<EOF
#!/bin/sh
exec "$WESTON" --shell=kiosk-shell.so "\$@"
EOF
cat >"$W/shows.py" <<'PY'
# exit 0 when the PNG shows more than one colour on a noticeable part of the screen
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
assert data[:8] == b"\x89PNG\r\n\x1a\n"
at, idat, w, h, ct = 8, b"", 0, 0, 0
while at < len(data):
    n, kind = struct.unpack(">I4s", data[at:at + 8])
    body = data[at + 8:at + 8 + n]
    if kind == b"IHDR":
        w, h, depth, ct = struct.unpack(">IIBB", body[:10])
        assert depth == 8 and ct in (2, 6) and body[12] == 0
    elif kind == b"IDAT":
        idat += body
    at += 12 + n
bpp = 4 if ct == 6 else 3
raw = zlib.decompress(idat)
stride = w * bpp
prev = bytearray(stride)
counts = {}
for y in range(h):
    f = raw[y * (stride + 1)]
    line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
    for i in range(stride):
        a = line[i - bpp] if i >= bpp else 0
        b = prev[i]
        c = prev[i - bpp] if i >= bpp else 0
        if f == 1: line[i] = (line[i] + a) & 255
        elif f == 2: line[i] = (line[i] + b) & 255
        elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
        elif f == 4:
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
    for x in range(0, stride, bpp * 4):
        px = bytes(line[x:x + 3])
        counts[px] = counts.get(px, 0) + 1
    prev = line
total = sum(counts.values())
top = max(counts.values())
print("colours %d, other than the most common: %.1f%%" % (len(counts), 100.0 * (total - top) / total))
sys.exit(0 if len(counts) >= 8 and (total - top) * 100 >= total else 1)
PY
cat >"$W/shot.sh" <<EOF
#!/bin/sh
# shot.sh SOCKET PNG: a screenshot of the Weston at SOCKET; exit 0 when it shows the app
d=\$(mktemp -d)
cd "\$d" || exit 1
WAYLAND_DISPLAY="\$1" XDG_RUNTIME_DIR="\$d" timeout 30 "$SHOOTER" >/dev/null 2>&1
f=\$(ls "\$d"/*.png 2>/dev/null | head -1)
ok=1
if [ -n "\$f" ] && python3 -I "$W/shows.py" "\$f" >"$W/shot.last" 2>&1; then cp "\$f" "\$2"; ok=0; fi
rm -rf "\$d"
exit \$ok
EOF
chmod 0755 "$W/weston" "$W/shot.sh"

# --- the test -----------------------------------------------------------------------------------
LOG="$W/test.log"
ULTAR_FOREIGN_RUNTIME="$OUT/linux" ULTAR_FOREIGN_APKS="$W/apks" ULTAR_FOREIGN_APP="$APP" \
ULTAR_FOREIGN_BIN="$T/ultar-foreign" ULTAR_FOREIGN_WESTON="$W/weston" \
ULTAR_FOREIGN_SHOT="$W/shot.sh" ULTAR_FOREIGN_SHOT_OUT="$W/window.png" \
  cargo test -q -p ultar-launcher --test foreign_android_e2e -- --nocapture --test-threads 1 >"$LOG" 2>&1 \
  || { tail -80 "$LOG"; fail "test failed (log: $LOG)"; }
grep '^FOREIGN' "$LOG"
echo "test app: $APP_FILE sha256 $APP_SHA256"
echo "window: $W/window.png ($(cat "$W/shot.last"))"
grep -q '^FOREIGN-ANDROID-E2E: ALLES OK$' "$LOG" || { tail -40 "$LOG"; fail "the test did not run (skipped?)"; }
