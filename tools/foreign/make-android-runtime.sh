#!/bin/sh
# The component "Android apps" of the Linux area (docs/integration/ultar-foreign.md,
# docs/FREMD-APPS.md stage P5): Android Translation Layer (ATL) with its runtime – ART
# (art_standalone, with wolfSSL-JNI), the bionic shim linker (bionic_translation), wolfSSL
# and OpenSL ES – built from pinned sources in a build tree of its own (never in the area),
# packaged as ultar-android-runtime_<version>_<arch>.deb and installed into a copy of the
# Linux area.
#
#   tools/foreign/make-android-runtime.sh AREA [OUT]   (as root, on a Linux build machine)
#
#   AREA   the Linux area (OUT/linux of tools/foreign/make-linux-runtime.sh)
#   OUT    default ./out/foreign-android
#
# Environment:
#   JOBS            parallel build jobs (default: number of processors)
#   SRC             source checkouts (default OUT/src; fetched at the pinned commits)
#   ULTAR_PKG_KEY   with it OUT also gets the signed component
#                   foreign.android-runtime-<version>-<arch>.uap (ultar-pkg, kind "data")
#   UPKG_SIGN       upkg-sign program (as in make-linux-runtime.sh)
#
# Output (OUT):
#   ultar-android-runtime_<version>_amd64.deb   ATL and its runtime, depending on the area's
#                                               Ubuntu libraries
#   linux/                                      the Linux area with the component installed
#   linux-android-<version>-amd64.ext4 (+.sha256)
#   SOURCES                                     what was built (repositories, commits, licences)
#
# What the component never has: Google services, real device identities (ATL answers
# fixed, neutral values), location, microphone or WebView (the start program leaves ATL's
# switches for them off). Native code of the apps lies read-only in the area
# (/opt/ultar-android/apps/<package>/lib, put there by ultar-foreign), never in the app's
# writable data.
#
# Never on the VPS.
set -eu

fail() { echo "make-android-runtime: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
[ "$(uname -s)" = Linux ] || fail "Linux only"
case " $(hostname -I 2>/dev/null) " in *" 194.62.29.64 "*) fail "not on the VPS" ;; esac
command -v mmdebstrap >/dev/null || fail "mmdebstrap missing"
[ "$(dpkg --print-architecture)" = amd64 ] || fail "amd64 only (ATL is built for x86-64 here)"

AREA=$(cd "${1:?the Linux area (OUT/linux of make-linux-runtime.sh)}" && pwd)
[ -f "$AREA/etc/ultar-runtime" ] || fail "$AREA is no Linux area"
OUT=${2:-out/foreign-android}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
SRC=${SRC:-$OUT/src}
JOBS=${JOBS:-$(nproc)}
SUITE=$(sed -n 's/^suite=//p' "$AREA/etc/ultar-runtime")
MIRROR=${MIRROR:-http://archive.ubuntu.com/ubuntu}

# --- pinned sources (reproducible) ----------------------------------------------------------
GL=https://gitlab.com/android_translation_layer
SOURCES="
android_translation_layer $GL/android_translation_layer.git b87f0455fda749b8138eba52dee45e0105861656 GPL-3.0-or-later
art_standalone $GL/art_standalone.git 381138f574985b593eaaa64ad5660cfc4040d4b4 Apache-2.0/GPL-2.0-with-classpath-exception
bionic_translation $GL/bionic_translation.git 6253dec7f24a57a94039a088cb436d880e92e3be Apache-2.0/BSD
libopensles-standalone $GL/libopensles-standalone.git c6982620c7dadeb90220b23c575b25eeffda765f Apache-2.0
wolfssl https://github.com/wolfSSL/wolfssl.git v5.8.2-stable GPL-3.0-or-later
"
VERSION=0.$(date -u +%Y%m%d).$(echo "$SOURCES" | sha256sum | cut -c1-8)

fetch() { # name url commit
  dir="$SRC/$1"
  if [ ! -d "$dir/.git" ]; then
    n=0
    case "$3" in
      *[!0-9a-f]*) set -- "$1" "$2" "$3" --depth=1 --branch="$3" ;; # a tag: only it
      *) set -- "$1" "$2" "$3" ;;
    esac
    until git clone -q ${4:-} ${5:-} "$2" "$dir"; do
      n=$((n + 1)); [ $n -lt 4 ] || fail "clone $2"; rm -rf "$dir"; sleep 10
    done
  fi
  git -C "$dir" checkout -q "$3" 2>/dev/null || { git -C "$dir" fetch -q --tags origin && git -C "$dir" checkout -q "$3"; } || fail "$1 at $3"
}
mkdir -p "$SRC"
echo "$SOURCES" | while read -r name url commit licence; do
  [ -n "$name" ] || continue
  fetch "$name" "$url" "$commit"
done
{
  echo "# ultar-android-runtime $VERSION – built $(date -u +%Y-%m-%dT%H:%M:%SZ) on Ubuntu $SUITE"
  echo "$SOURCES" | while read -r name url commit licence; do
    [ -n "$name" ] && echo "$name $url $(git -C "$SRC/$name" rev-parse HEAD) $licence"
  done
} >"$OUT/SOURCES"

# --- the build tree (its own Ubuntu, the same release as the area) ---------------------------
BUILD="$OUT/build"
BUILD_DEPS="build-essential,meson,ninja-build,pkg-config,git,ca-certificates,autoconf,automake,libtool,python3,python-is-python3,default-jdk-headless,zip,unzip,file,libasm-java,libgetopt-java,libegl-dev,libgl-dev,libbsd-dev,libunwind-dev,libelf-dev,valgrind,libicu-dev,libssl-dev,libexpat1-dev,liblz4-dev,liblzma-dev,libcap-dev,zlib1g-dev,libasound2-dev,libavcodec-dev,libswscale-dev,libdrm-dev,libglib2.0-dev,libgtk-4-dev,libgudev-1.0-dev,libopenxr-dev,libportal-dev,libsqlite3-dev,libwebkitgtk-6.0-dev,libsecret-1-dev,libspeechd-dev,libvulkan-dev,libwayland-dev,wayland-protocols,libfontconfig-dev,binutils"
if [ ! -f "$BUILD/.ready" ]; then
  rm -rf "$BUILD"
  mmdebstrap --mode=root --variant=minbase --arch=amd64 --components=main,universe \
    --include="$BUILD_DEPS" --aptopt='APT::Install-Recommends "false"' \
    "$SUITE" "$BUILD" "deb $MIRROR $SUITE main universe" "deb $MIRROR $SUITE-updates main universe" \
    || fail "build tree"
  touch "$BUILD/.ready"
fi
rm -rf "$BUILD/src" "$BUILD/stage"
cp -a "$SRC" "$BUILD/src"
cat >"$BUILD/src/build-inside.sh" <<'INSIDE'
#!/bin/sh
# Runs in the build tree: builds and installs everything into / and lists what came new.
set -eux
J=$1
LIB=lib/x86_64-linux-gnu
touch /.stamp
sleep 1
cd /src/wolfssl
autoreconf -i >/dev/null 2>&1
# the configuration of ATL's doc/Build.md (art_standalone builds wolfSSL-JNI against it)
./configure --prefix=/usr --libdir=/usr/$LIB --enable-shared --disable-static \
  --disable-opensslall --disable-opensslextra --enable-aescbc-length-checks --enable-curve25519 \
  --enable-ed25519 --enable-ed25519-stream --enable-oldtls --enable-base64encode --enable-tlsx \
  --enable-scrypt --disable-examples --enable-crl --enable-jni --enable-sessioncerts >/dev/null
make -j"$J" >/dev/null
make install >/dev/null
for p in bionic_translation libopensles-standalone; do
  cd /src/$p
  rm -rf builddir
  meson setup builddir --prefix=/usr --libdir=$LIB --buildtype=release >/dev/null
  meson compile -C builddir -j "$J" >/dev/null
  meson install -C builddir >/dev/null
done
cd /src/art_standalone
make -j"$J" ____PREFIX=/usr ____LIBDIR=$LIB >/tmp/art-build.log 2>&1 || { tail -40 /tmp/art-build.log; exit 1; }
make ____PREFIX=/usr ____LIBDIR=$LIB install >/dev/null
ldconfig
cd /src/android_translation_layer
rm -rf builddir
meson setup builddir --prefix=/usr --libdir=$LIB --buildtype=release \
  -Dasm_jar_path=/usr/share/java/asm.jar -Dasm_tree_jar_path=/usr/share/java/asm-tree.jar \
  -Dgetopt_jar_path=/usr/share/java/gnu-getopt.jar >/dev/null
meson compile -C builddir -j "$J" >/tmp/atl-build.log 2>&1 || { tail -40 /tmp/atl-build.log; exit 1; }
meson install -C builddir >/dev/null
ldconfig
# everything installed since the stamp (by creation time: installers keep the sources'
# modification times), without development files
mkdir -p /stage
find /usr /etc -xdev -cnewer /.stamp \( -type f -o -type l \) \
  ! -path '*/include/*' ! -name '*.a' ! -name '*.la' ! -path '*/pkgconfig/*' \
  ! -path '/etc/ld.so.cache' ! -path '/usr/share/doc/*' ! -path '/usr/share/man/*' \
  ! -path '*/__pycache__/*' \
  | while read -r f; do mkdir -p "/stage$(dirname "$f")"; cp -a "$f" "/stage$f"; done
# the Ubuntu packages the built programs need (their NEEDED libraries)
find /stage -type f | while read -r f; do
  file -b "$f" | grep -q '^ELF' || continue
  readelf -d "$f" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'
done | sort -u | while read -r so; do
  [ -n "$(find /stage -name "$so" -print -quit)" ] && continue
  path=$(ldconfig -p | awk -v s="$so" '$1 == s && /x86-64/ {print $NF; exit}')
  [ -n "$path" ] || { echo "unresolved: $so" >&2; continue; }
  { dpkg -S "$path" 2>/dev/null || dpkg -S "$(readlink -f "$path")" 2>/dev/null; } \
    | grep -v '^diversion' | head -1 | cut -d: -f1 | cut -d, -f1
done | sort -u >/stage.depends
INSIDE
mount -t proc proc "$BUILD/proc"
mount --bind /dev "$BUILD/dev"
set +e
chroot "$BUILD" sh /src/build-inside.sh "$JOBS" >"$OUT/build.log" 2>&1
built=$?
set -e
umount "$BUILD/dev" "$BUILD/proc"
[ "$built" = 0 ] || { tail -60 "$OUT/build.log"; fail "build failed (log: $OUT/build.log)"; }
[ -x "$BUILD/stage/usr/bin/android-translation-layer" ] || fail "no android-translation-layer built"

# --- the package ------------------------------------------------------------------------------
P="$OUT/pkg"
rm -rf "$P"
cp -a "$BUILD/stage" "$P"
mkdir -p "$P/DEBIAN" "$P/opt/ultar-android/bin" "$P/opt/ultar-android/apps"
cat >"$P/opt/ultar-android/bin/ultar-apk-run" <<'RUN'
#!/bin/sh
# Starts an Android app of the Ultar Linux area in its cell (docs/integration/ultar-foreign.md):
#   ultar-apk-run <package> <activity>      (the launcher's manifest of the app gives both)
# The APK and its native libraries lie read-only in /opt/ultar-android/apps/<package>; the
# app's data (Android's /data/data and "sdcard") is $HOME/android, writable and not executable.
set -eu
pkg=${1:-}
act=${2:-}
case "$pkg" in "" | *[!A-Za-z0-9_.]*) echo "ultar-apk-run: bad package name" >&2; exit 2 ;; esac
case "$act" in "" | *[!A-Za-z0-9_/\$]*) echo "ultar-apk-run: bad activity" >&2; exit 2 ;; esac
app=/opt/ultar-android/apps/$pkg
[ -f "$app/base.apk" ] || { echo "ultar-apk-run: $pkg is not installed" >&2; exit 3; }
: "${HOME:?}"
export ANDROID_APP_DATA_DIR="$HOME/android"
data="$ANDROID_APP_DATA_DIR/base.apk_"
mkdir -p "$data"
# native code never in the writable data: the read-only copy of the area
if [ ! -L "$data/lib" ] || [ "$(readlink "$data/lib")" != "$app/lib" ]; then
  rm -rf "$data/lib"
  ln -s "$app/lib" "$data/lib"
fi
export ATL_SKIP_NATIVES_EXTRACTION=1
# ART's cache (compiled code is not mapped from noexec data: ART falls back to its interpreter/JIT)
export ANDROID_DATA="$ANDROID_APP_DATA_DIR/art"
mkdir -p "$ANDROID_DATA/dalvik-cache"
# location, microphone and WebView stay off (no switch of the system grants them yet)
unset ATL_UGLY_ENABLE_LOCATION ATL_UGLY_ENABLE_MICROPHONE ATL_UGLY_ENABLE_WEBVIEW JDWP_LISTEN
exec /usr/bin/android-translation-layer "$app/base.apk" -l "$act"
RUN
chmod 0755 "$P/opt/ultar-android/bin/ultar-apk-run"
depends=$(grep -v '^$' "$BUILD/stage.depends" | grep -v -- '-dev$' | paste -sd, | sed 's/,/, /g')
[ -n "$depends" ] || fail "no dependencies found"
size_kb=$(du -sk "$P" | cut -f1)
cat >"$P/DEBIAN/control" <<CONTROL
Package: ultar-android-runtime
Version: $VERSION
Architecture: amd64
Maintainer: KM Code Craft <os@code-craft.tech>
Installed-Size: $size_kb
Depends: $depends
Description: Android apps for the Ultar Linux area (Android Translation Layer)
 ATL, ART (art_standalone), bionic_translation, wolfSSL (JNI) and OpenSL ES, built from
 the pinned sources in /opt/ultar-android/SOURCES, plus ultar-apk-run. No Google
 services; neutral device values; location, microphone and WebView off.
CONTROL
cp "$OUT/SOURCES" "$P/opt/ultar-android/SOURCES"
find "$P" -perm /6000 -type f -exec chmod ug-s {} +
DEB="$OUT/ultar-android-runtime_${VERSION}_amd64.deb"
rm -f "$OUT"/ultar-android-runtime_*.deb
dpkg-deb --root-owner-group -Zxz --build "$P" "$DEB" >/dev/null

# --- the area with the component --------------------------------------------------------------
TREE="$OUT/linux"
rm -rf "$TREE"
cp -a "$AREA" "$TREE"
cp "$DEB" "$TREE/tmp/ultar-android-runtime.deb"
mount -t proc proc "$TREE/proc"
mount --bind /dev "$TREE/dev"
set +e
(
  set -e
  chroot "$TREE" apt-get update -q
  chroot "$TREE" apt-get install -y -q --no-install-recommends /tmp/ultar-android-runtime.deb
) >"$OUT/install.log" 2>&1
installed=$?
set -e
umount "$TREE/dev" "$TREE/proc"
[ -z "$(ls -A "$TREE/dev")" ] || fail "device nodes left in the area"
[ "$installed" = 0 ] || { tail -30 "$OUT/install.log"; fail "installing into the area"; }
rm -rf "$TREE"/tmp/* "$TREE"/var/lib/apt/lists/* "$TREE"/var/cache/apt/*.bin "$TREE"/var/cache/apt/archives/*.deb
mkdir -p "$TREE/var/lib/apt/lists/partial"
find "$TREE" -xdev -perm /6000 -type f -exec chmod ug-s {} +
[ -z "$(find "$TREE" -xdev -perm /6000 -type f | head -1)" ] || fail "set-id programs in the area"
missing=$(chroot "$TREE" sh -c 'for f in /usr/bin/android-translation-layer /usr/bin/dalvikvm64 /usr/lib/x86_64-linux-gnu/libbionic_translation*.so* /usr/lib/x86_64-linux-gnu/java/dex/art/natives/*.so /usr/lib/x86_64-linux-gnu/libwolfssl.so*; do [ -e "$f" ] && ldd "$f" 2>&1 | grep "not found"; done' || true)
[ -z "$missing" ] || fail "libraries missing in the area: $missing"
[ -x "$TREE/opt/ultar-android/bin/ultar-apk-run" ] || fail "no start program"
printf 'android=%s\n' "$VERSION" >>"$TREE/etc/ultar-runtime"

IMAGE="$OUT/linux-android-$VERSION-amd64.ext4"
size_mb=$(du -sm "$TREE" | cut -f1)
size_mb=$((size_mb + size_mb / 4 + 64))
rm -f "$OUT"/linux-android-*.ext4 "$OUT"/linux-android-*.ext4.sha256
mke2fs -q -t ext4 -L ultar-linux -m 0 -E root_owner=0:0 -d "$TREE" "$IMAGE" "${size_mb}M"
(cd "$OUT" && sha256sum "$(basename "$IMAGE")" >"$(basename "$IMAGE").sha256")

if [ -n "${ULTAR_PKG_KEY:-}" ]; then
  SIGN=${UPKG_SIGN:-$(command -v upkg-sign || true)}
  [ -n "$SIGN" ] || fail "ULTAR_PKG_KEY given but no upkg-sign"
  C="$OUT/component"
  rm -rf "$C"; mkdir -p "$C/payload"
  cp "$DEB" "$C/payload/ultar-android-runtime.deb"
  cp "$OUT/SOURCES" "$C/payload/SOURCES"
  (cd "$C/payload" && sha256sum ultar-android-runtime.deb >ultar-android-runtime.deb.sha256)
  cat >"$C/manifest.toml" <<MANIFEST
id = "foreign.android-runtime"
name = "Android apps for apps from other systems (Android Translation Layer)"
version = "$VERSION"
arch = "x86_64"
kind = "data"
publisher = "KM Code Craft"
size = $(stat -c %s "$DEB")
MANIFEST
  "$SIGN" "$C/manifest.toml" "$C/payload" "$OUT/foreign.android-runtime-$VERSION-amd64.uap" "$ULTAR_PKG_KEY"
fi

echo "make-android-runtime: $(basename "$DEB") ($(du -h "$DEB" | cut -f1)), area $(du -sh "$TREE" | cut -f1)"
echo "make-android-runtime: depends: $depends"
echo "ANDROID-RUNTIME: OK"
