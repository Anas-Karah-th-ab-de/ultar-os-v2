#!/bin/sh
# Baut das Ultar-Fenstersystem (platform/ultar-wm) und packt es als .deb für den Ubuntu-Bereich (Ubuntu 24.04).
#   scripts/package-ultar-wm.sh [out/]
# Installation auf dem Gerät: Pakete -> Datei installieren, oder: ultar-compat apt install ./ultar-wm_<ver>_<arch>.deb
# Build-Abhängigkeiten (Ubuntu 24.04):
#   apt install libxkbcommon-dev libwayland-dev libgbm-dev libegl-dev libgles-dev libudev-dev libseat-dev \
#               libinput-dev libdrm-dev libpixman-1-dev pkg-config
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/out}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
cd "$ROOT/platform/ultar-wm"
# Querübersetzen (z. B. arm64 für das A30): ULTAR_WM_TARGET=aarch64-unknown-linux-gnu DEB_ARCH=arm64
if [ -n "${ULTAR_WM_TARGET:-}" ]; then
  cargo build --release --target "$ULTAR_WM_TARGET"
  BIN=target/$ULTAR_WM_TARGET/release/ultar-wm
else
  cargo build --release
  BIN=target/release/ultar-wm
fi
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=${DEB_ARCH:-$(dpkg --print-architecture)}
P=$(mktemp -d)
install -Dm755 "$BIN" "$P/usr/bin/ultar-wm"
install -Dm644 "$ROOT/rootfs/etc/ultar/desktop.toml" "$P/etc/ultar/desktop.toml"
install -Dm644 LICENSE-SMITHAY-MIT.txt "$P/usr/share/doc/ultar-wm/LICENSE-SMITHAY-MIT.txt"
mkdir -p "$P/DEBIAN"
cat > "$P/DEBIAN/control" <<EOF
Package: ultar-wm
Version: $VER
Architecture: $ARCH
Maintainer: KM Code Craft <https://code-craft.tech>
Depends: libinput10, libseat1, libgbm1, libegl1, libgles2, libxkbcommon0, libudev1, libwayland-server0, libdrm2, libpixman-1-0, libgl1-mesa-dri, xwayland, dbus
Recommends: foot, fonts-noto-core
Section: x11
Priority: optional
Description: Ultar-Fenstersystem (Wayland-Compositor) mit Leiste, Startmenü und Desktop-/Tablet-Modus
 Eigenes Fenstersystem von Ultar OS in Rust. Basis: Smithay/Anvil (MIT).
EOF
dpkg-deb --root-owner-group -b "$P" "$OUT/ultar-wm_${VER}_${ARCH}.deb"
rm -rf "$P"
echo "fertig: $OUT/ultar-wm_${VER}_${ARCH}.deb"
