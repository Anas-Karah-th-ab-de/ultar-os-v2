#!/bin/sh
# The Linux area of the apps from other systems (docs/integration/ultar-foreign.md,
# docs/FREMD-APPS.md stage P2): a minimal Ubuntu tree whose programs run in the cells of
# profile app.foreign (read-only, nosuid, nodev as their root) and whose package manager
# runs only in ultar-foreign's installation cell.
#
#   tools/foreign/make-linux-runtime.sh [OUT]          (as root, on a Linux build machine)
#
# Environment:
#   SUITE          Ubuntu release (default resolute = 26.04; noble = 24.04 when resolute
#                  cannot be reached, with the reason printed)
#   ARCH           amd64 or arm64 (default: this machine's)
#   MIRROR         default archive.ubuntu.com (amd64) or ports.ubuntu.com (arm64)
#   TEST_PACKAGE   e.g. mousepad: its dependencies are installed into the area and the
#                  package itself is saved as OUT/<package>.deb (offline end-to-end test:
#                  the installation cell has no network until stage P0)
#   ULTAR_PKG_KEY  file with the hex private key: OUT also gets the signed component
#                  foreign.linux-runtime-<version>-<arch>.uap (ultar-pkg, kind "data")
#   UPKG_SIGN      upkg-sign program (default: target/release/upkg-sign, then PATH)
#
# Output (OUT, default ./out/foreign-linux):
#   linux/                       the tree (what ultar-foreign keeps at /data/ultar/foreign/linux)
#   linux-<version>-<arch>.ext4  the same as an ext4 image, and its .sha256
#   *.uap                        the signed component (with ULTAR_PKG_KEY)
#
# What the area has: apt/dpkg, GTK 3 and 4, the Qt 6 Wayland plugin, Mesa (llvmpipe),
# Noto fonts with Arabic, CA certificates, the locales ar/en/de and a replacement for
# xdg-utils (no bus in the cells: it says so and fails). What it never has: systemd, an SSH
# server, set-id programs (removed here and after every dpkg run, and the area is mounted
# nosuid everywhere anyway).
#
# Never on the VPS (it builds a whole distribution tree).
set -eu

fail() { echo "make-linux-runtime: $*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || fail "run as root (the tree's files belong to root)"
[ "$(uname -s)" = Linux ] || fail "Linux only"
case " $(hostname -I 2>/dev/null) " in
  *" 194.62.29.64 "*) fail "not on the VPS" ;;
esac
command -v mmdebstrap >/dev/null || fail "mmdebstrap missing (apt-get install mmdebstrap)"
command -v mke2fs >/dev/null || fail "mke2fs missing (e2fsprogs)"

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
OUT=${1:-out/foreign-linux}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
ARCH=${ARCH:-$(dpkg --print-architecture)}
case "$ARCH" in
  amd64) MIRROR=${MIRROR:-http://archive.ubuntu.com/ubuntu}; PKG_ARCH=x86_64; TRIPLET=x86_64-linux-gnu ;;
  arm64) MIRROR=${MIRROR:-http://ports.ubuntu.com/ubuntu-ports}; PKG_ARCH=aarch64; TRIPLET=aarch64-linux-gnu ;;
  *) fail "ARCH $ARCH: amd64 or arm64" ;;
esac

SUITE=${SUITE:-resolute}
if ! curl -fsS -o /dev/null --max-time 20 "$MIRROR/dists/$SUITE/Release"; then
  [ "$SUITE" = resolute ] || fail "$MIRROR/dists/$SUITE not reachable"
  echo "make-linux-runtime: $MIRROR/dists/resolute (26.04) not reachable from here; using noble (24.04 LTS)" >&2
  SUITE=noble
fi
case "$SUITE" in resolute) RELEASE=26.04 ;; noble) RELEASE=24.04 ;; *) RELEASE=$SUITE ;; esac
VERSION=${VERSION:-$RELEASE.$(date -u +%Y%m%d)}
TREE="$OUT/linux"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# --- the xdg-utils replacement (a package, so that nothing pulls the real one) -----------
X="$WORK/xdg-utils"
mkdir -p "$X/DEBIAN" "$X/usr/bin"
cat >"$X/DEBIAN/control" <<CONTROL
Package: xdg-utils
Version: 99:1.0-ultar1
Architecture: all
Maintainer: KM Code Craft <os@code-craft.tech>
Description: replacement for xdg-utils in the Ultar Linux area
 The apps of the Linux area run in their own cells without the desktop's bus: opening
 a link or a file in another app is not available there. Each command says so and fails.
CONTROL
for tool in xdg-open xdg-mime xdg-settings xdg-email xdg-screensaver xdg-desktop-menu xdg-desktop-icon xdg-icon-resource; do
  cat >"$X/usr/bin/$tool" <<'TOOL'
#!/bin/sh
echo "$(basename "$0"): not available in an Ultar cell (apps from other systems cannot open other apps)" >&2
exit 3
TOOL
  chmod 0755 "$X/usr/bin/$tool"
done
dpkg-deb --root-owner-group -Zxz --build "$X" "$WORK/xdg-utils.deb" >/dev/null

# --- the bwrap replacement: the app's cell is the sandbox (tools/foreign/bwrap-shim.c) ---------
# GTK's image loader glycin starts its loaders through bwrap, which needs user namespaces (the Ultar
# kernel has none). The replacement starts the program in the same cell with the same user.
B="$WORK/bubblewrap"
mkdir -p "$B/DEBIAN" "$B/usr/bin"
${CC:-gcc} -Os -static -Wall -Wextra -o "$B/usr/bin/bwrap" "$(dirname "$0")/bwrap-shim.c" || fail "bwrap-shim.c does not build"
strip "$B/usr/bin/bwrap"
cat >"$B/DEBIAN/control" <<CONTROL
Package: bubblewrap
Version: 99:0.11-ultar1
Architecture: $ARCH
Maintainer: KM Code Craft <os@code-craft.tech>
Description: bwrap for the Ultar Linux area (no namespaces: the app's cell is the sandbox)
 Understands bwrap's command line and starts the program in the same cell with the same user;
 namespace, mount and capability requests are ignored.
CONTROL
dpkg-deb --root-owner-group -Zxz --build "$B" "$WORK/bubblewrap.deb" >/dev/null

# --- the tree ----------------------------------------------------------------------------
PACKAGES="apt,dpkg,ca-certificates,locales,libgtk-3-0t64,libgtk-4-1,qt6-wayland,libgl1-mesa-dri,libegl-mesa0,libegl1,libgles2,fonts-noto-core,fonts-dejavu-core,gsettings-desktop-schemas,adwaita-icon-theme,hicolor-icon-theme,shared-mime-info,libglib2.0-bin"
rm -rf "$TREE"
mmdebstrap --mode=root --variant=minbase --arch="$ARCH" --components=main,universe \
  --skip=cleanup/apt/lists \
  --include="$PACKAGES" \
  --aptopt='APT::Install-Recommends "false"' \
  --aptopt='APT::Install-Suggests "false"' \
  --dpkgopt='path-exclude=/usr/share/doc/*' \
  --dpkgopt='path-include=/usr/share/doc/*/copyright' \
  --dpkgopt='path-exclude=/usr/share/man/*' \
  --dpkgopt='path-exclude=/usr/share/info/*' \
  --dpkgopt='path-exclude=/usr/share/lintian/*' \
  --dpkgopt='path-exclude=/usr/share/locale/*' \
  --dpkgopt='path-include=/usr/share/locale/ar/*' \
  --dpkgopt='path-include=/usr/share/locale/en*/*' \
  --dpkgopt='path-include=/usr/share/locale/de/*' \
  --dpkgopt='path-include=/usr/share/locale/locale.alias' \
  --setup-hook='mkdir -p "$1/etc/apt/preferences.d"' \
  --setup-hook="cat >\"\$1/etc/apt/preferences.d/ultar\" <<'PIN'
# The Ultar Linux area never gets an init system, a login or SSH server, or the real
# xdg-utils (its replacement is installed: there is no bus in the cells).
Package: systemd systemd-sysv systemd-resolved systemd-timesyncd udev init openssh-server dropbear-bin snapd policykit-1 polkitd dbus-user-session
Pin: version *
Pin-Priority: -1

Package: xdg-utils bubblewrap
Pin: version 99:*
Pin-Priority: 1001
PIN" \
  --customize-hook="copy-in $WORK/xdg-utils.deb /tmp" \
  --customize-hook='chroot "$1" dpkg --purge --force-depends xdg-utils >/dev/null 2>&1 || true' \
  --customize-hook='chroot "$1" dpkg -i /tmp/xdg-utils.deb' \
  --customize-hook='rm -f "$1/tmp/xdg-utils.deb"' \
  --customize-hook="copy-in $WORK/bubblewrap.deb /tmp" \
  --customize-hook='chroot "$1" dpkg --purge --force-depends bubblewrap >/dev/null 2>&1 || true' \
  --customize-hook='chroot "$1" dpkg -i /tmp/bubblewrap.deb' \
  --customize-hook='rm -f "$1/tmp/bubblewrap.deb"' \
  --customize-hook='printf "ar_SY.UTF-8 UTF-8\nen_US.UTF-8 UTF-8\nde_DE.UTF-8 UTF-8\n" >"$1/etc/locale.gen"' \
  --customize-hook='chroot "$1" locale-gen >/dev/null' \
  "$SUITE" "$TREE" "deb $MIRROR $SUITE main universe" "deb $MIRROR $SUITE-updates main universe" "deb $MIRROR $SUITE-security main universe"

# --- the test package's dependencies (before the package lists go) ------------------------
if [ -n "${TEST_PACKAGE:-}" ]; then
  case "$TEST_PACKAGE" in *[!a-z0-9.+-]*) fail "TEST_PACKAGE: a package name" ;; esac
  mount -t proc proc "$TREE/proc"
  set +e
  (
    set -e
    chroot "$TREE" apt-get install -y -q --no-install-recommends "$TEST_PACKAGE" >"$OUT/test-deps.log" 2>&1
    # its dependencies stay (manually installed), the package itself is installed later by
    # ultar-foreign from the saved file
    chroot "$TREE" sh -c 'apt-mark showauto | xargs -r apt-mark manual >/dev/null'
    chroot "$TREE" sh -c "cd /tmp && apt-get download -q '$TEST_PACKAGE'" >>"$OUT/test-deps.log" 2>&1
    chroot "$TREE" dpkg --purge "$TEST_PACKAGE" >>"$OUT/test-deps.log" 2>&1
  )
  result=$?
  set -e
  umount "$TREE/proc"
  [ "$result" = 0 ] || { tail -20 "$OUT/test-deps.log"; fail "TEST_PACKAGE $TEST_PACKAGE"; }
  mv "$TREE"/tmp/"${TEST_PACKAGE}"_*.deb "$OUT/$TEST_PACKAGE.deb"
fi

# --- the area's own settings -------------------------------------------------------------
# No set-id program after any later dpkg run either (the installation cell runs dpkg).
cat >"$TREE/etc/apt/apt.conf.d/99ultar" <<'APT'
// Ultar Linux area (docs/integration/ultar-foreign.md)
APT::Install-Recommends "false";
APT::Install-Suggests "false";
// the installation cell is the sandbox: no switch to _apt (no set-id in the cell)
APT::Sandbox::User "root";
DPkg::Post-Invoke { "find / -xdev -perm /6000 -type f -exec chmod ug-s {} + 2>/dev/null || true"; };
APT
# The cells have no network without the app's permission; with it, the kernel's barrier lets
# through the internet only (stage P0), so the resolvers are public ones, not the LAN's.
cat >"$TREE/etc/resolv.conf" <<'DNS'
# Ultar Linux area: public resolvers (an app with network reaches the internet only).
nameserver 9.9.9.9
nameserver 1.1.1.1
DNS
printf 'version=%s\nsuite=%s\narch=%s\nbuilt=%s\n' "$VERSION" "$SUITE" "$ARCH" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$TREE/etc/ultar-runtime"
: >"$TREE/etc/machine-id"
echo ultar-linux >"$TREE/etc/hostname"
# the cells mount their own /dev: no device node stays in the area
find "$TREE/dev" -mindepth 1 -delete
for d in run tmp proc dev sys; do mkdir -p "$TREE/$d"; done
chmod 1777 "$TREE/tmp"
# smaller: package lists and caches come back with the first update of the area
rm -rf "$TREE"/var/lib/apt/lists/* "$TREE"/var/cache/apt/*.bin "$TREE"/var/cache/apt/archives/*.deb \
  "$TREE"/var/log/*.log "$TREE"/var/log/apt/* "$TREE"/tmp/* "$TREE"/var/tmp/*
mkdir -p "$TREE/var/lib/apt/lists/partial"
# apt's own directories belong to root: apt runs as root of the installation cell (no _apt
# sandbox), and root of a cell has no right over other owners' entries on the Ultar kernel
chown -R root:root "$TREE/var/cache/apt" "$TREE/var/lib/apt/lists"
chmod 0700 "$TREE/var/cache/apt/archives/partial" "$TREE/var/lib/apt/lists/partial"

# --- no set-id program, no init system, no SSH server ------------------------------------
find "$TREE" -xdev -perm /6000 -type f -exec chmod ug-s {} +
left=$(find "$TREE" -xdev -perm /6000 -type f | head -5)
[ -z "$left" ] || fail "set-id programs left: $left"
for gone in usr/lib/systemd/systemd sbin/init usr/sbin/init usr/sbin/sshd usr/sbin/dropbear usr/bin/snap; do
  [ ! -e "$TREE/$gone" ] || fail "$gone in the area"
done
for pkg in systemd systemd-sysv udev openssh-server; do
  if chroot "$TREE" dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed"; then
    fail "package $pkg in the area"
  fi
done

# --- what the apps need is there -----------------------------------------------------------
need() { [ -e "$TREE/$1" ] || fail "missing in the area: $1"; }
need usr/bin/apt-get
need usr/bin/bwrap
chroot "$TREE" bwrap --unshare-all --die-with-parent --ro-bind / / --chdir /tmp --setenv ULTAR_SHIM 1 -- sh -c 'test "$ULTAR_SHIM" = 1 && test "$(pwd)" = /tmp' \
  || fail "the bwrap replacement does not start its program"
need usr/bin/dpkg-deb
need "usr/lib/$TRIPLET/libgtk-3.so.0"
need "usr/lib/$TRIPLET/libgtk-4.so.1"
ls "$TREE/usr/lib/$TRIPLET/qt6/plugins/platforms/"libqwayland*.so >/dev/null 2>&1 || fail "missing: Qt 6 Wayland plugin"
ls "$TREE/usr/lib/$TRIPLET/"libgallium*.so "$TREE/usr/lib/$TRIPLET/dri/swrast_dri.so" >/dev/null 2>&1 \
  || ls "$TREE/usr/lib/$TRIPLET/dri/" | grep -q . || fail "missing: Mesa (llvmpipe)"
ls "$TREE"/usr/share/fonts/truetype/noto/NotoSansArabic-*.ttf "$TREE"/usr/share/fonts/truetype/noto/NotoNaskhArabic-*.ttf >/dev/null 2>&1 \
  || fail "missing: Noto fonts with Arabic"
need etc/ssl/certs/ca-certificates.crt
for l in ar_SY.utf8 en_US.utf8 de_DE.utf8; do
  chroot "$TREE" locale -a | grep -qx "$l" || fail "locale $l missing"
done
chroot "$TREE" xdg-open https://example.org >/dev/null 2>&1 && fail "xdg-open must fail in the area"

# --- the image and the component -----------------------------------------------------------
IMAGE="$OUT/linux-$VERSION-$ARCH.ext4"
size_mb=$(du -sm "$TREE" | cut -f1)
size_mb=$((size_mb + size_mb / 4 + 64))
rm -f "$IMAGE"
mke2fs -q -t ext4 -L ultar-linux -m 0 -E root_owner=0:0 -d "$TREE" "$IMAGE" "${size_mb}M"
e2fsck -fn "$IMAGE" >/dev/null 2>&1 || fail "the image does not check"
(cd "$OUT" && sha256sum "$(basename "$IMAGE")" >"$(basename "$IMAGE").sha256")

if [ -n "${ULTAR_PKG_KEY:-}" ]; then
  SIGN=${UPKG_SIGN:-}
  [ -n "$SIGN" ] || { [ -x "$REPO/target/release/upkg-sign" ] && SIGN="$REPO/target/release/upkg-sign"; } || true
  [ -n "$SIGN" ] || SIGN=$(command -v upkg-sign || true)
  [ -n "$SIGN" ] || fail "ULTAR_PKG_KEY given but no upkg-sign (cargo build --release -p ultar-pkg)"
  P="$WORK/component"
  mkdir -p "$P/payload"
  ln "$IMAGE" "$P/payload/linux.ext4" 2>/dev/null || cp "$IMAGE" "$P/payload/linux.ext4"
  (cd "$P/payload" && sha256sum linux.ext4 >linux.ext4.sha256)
  cp "$TREE/etc/ultar-runtime" "$P/payload/runtime.txt"
  cat >"$P/manifest.toml" <<MANIFEST
id = "foreign.linux-runtime"
name = "Linux area for apps from other systems (Ubuntu $RELEASE)"
version = "$VERSION"
arch = "$PKG_ARCH"
kind = "data"
publisher = "KM Code Craft"
size = $(stat -c %s "$IMAGE")
MANIFEST
  UAP="$OUT/foreign.linux-runtime-$VERSION-$ARCH.uap"
  "$SIGN" "$P/manifest.toml" "$P/payload" "$UAP" "$ULTAR_PKG_KEY"
fi

echo "make-linux-runtime: $SUITE ($RELEASE) $ARCH, $(du -sh "$TREE" | cut -f1) in $TREE"
echo "make-linux-runtime: $(cat "$IMAGE.sha256")"
echo "LINUX-RUNTIME: OK"
