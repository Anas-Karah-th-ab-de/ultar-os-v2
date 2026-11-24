#!/bin/sh
# Baut ultar-llm (statisch, llama.cpp eingebunden) und packt es als .deb für Ultar-Desktop / Ubuntu-PCs.
#   scripts/package-ultar-llm.sh [out/]          x86_64 (Grundlage x86-64-v3 = AVX2, alle PCs ab ~2015)
#   ARCH=aarch64 scripts/package-ultar-llm.sh     ARM (Querübersetzer g++-aarch64-linux-gnu nötig)
#   X86_LEVEL=x86-64-v2 …                         für alte PCs ohne AVX2
# Auf Ultar OS selbst gehört ultar-llm ins Kernsystem (scripts/build-initramfs.sh) – dieses Paket ist für
# fremde Linux-Systeme und den Ubuntu-Bereich gedacht.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/out}; mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)
ARCH=${ARCH:-$(uname -m)}
cd "$ROOT"
if [ "$ARCH" = aarch64 ]; then
  T=aarch64-unknown-linux-gnu; DEB=arm64
  export CC_aarch64_unknown_linux_gnu=${CC_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-gcc}
  export CXX_aarch64_unknown_linux_gnu=${CXX_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-g++}
  export AR_aarch64_unknown_linux_gnu=${AR_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-ar}
  FLAGS="-C target-feature=+crt-static"
else
  T=x86_64-unknown-linux-gnu; DEB=amd64
  FLAGS="-C target-feature=+crt-static -C target-cpu=${X86_LEVEL:-x86-64-v3}"
fi
if [ "${BUILD_STD:-0}" = 1 ]; then
  RUSTC_BOOTSTRAP=1 RUSTFLAGS="$FLAGS" cargo build --release -p ultar-llm -Z build-std=std,panic_abort --target "$T"
else
  RUSTFLAGS="$FLAGS" cargo build --release -p ultar-llm --target "$T"
fi
BIN=${CARGO_TARGET_DIR:-target}/$T/release/ultar-llm
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
P=$(mktemp -d); trap 'rm -rf "$P"' EXIT
install -Dm755 "$BIN" "$P/usr/bin/ultar-llm"
install -Dm644 rootfs/etc/ultar/llm.toml "$P/etc/ultar/llm.toml"
install -Dm644 rootfs/usr/lib/ultar/systemd/ultar-llm.service "$P/usr/lib/systemd/system/ultar-llm.service"
install -Dm644 rootfs/usr/lib/ultar/systemd/ultar-llm-user.service "$P/usr/lib/systemd/user/ultar-llm.service"
install -Dm644 docs/LLM-DIENST.md "$P/usr/share/doc/ultar-llm/LLM-DIENST.md"
mkdir -p "$P/var/lib/ultar/models" "$P/DEBIAN"
cat > "$P/DEBIAN/control" <<CTL
Package: ultar-llm
Version: $VER
Architecture: $DEB
Maintainer: KM Code Craft <info@code-craft.tech>
Section: utils
Priority: optional
Description: Ultar LLM – lokales Sprachmodell als Dienst (llama.cpp, OpenAI-kompatibel)
 Rust-Dienst von Ultar OS: lädt GGUF-Modelle bei Bedarf, Schnittstelle auf 127.0.0.1:8080
 und Unix-Socket, entlädt im Leerlauf. Statisch gebaut, keine Abhängigkeiten.
CTL
cat > "$P/DEBIAN/conffiles" <<CF
/etc/ultar/llm.toml
CF
dpkg-deb --root-owner-group --build "$P" "$OUT/ultar-llm_${VER}_${DEB}.deb"
