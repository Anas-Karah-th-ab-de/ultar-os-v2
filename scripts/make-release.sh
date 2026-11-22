#!/usr/bin/env bash
# Baut alle Dateien eines Releases nach dist/:
#   ultar-initramfs-<arch>.cpio.zst   Kernsystem (ohne Test-/Entwicklerteile) – Quelle für das Systemupdate
#   UltarFlash-<version>.msi          Windows-Installationspaket – Quelle für das Update von Ultar Flash
#   UltarFlash-<version>.zip          dasselbe ohne Installation
#   SHA256SUMS                        Prüfsummen; ohne sie lehnen beide Updater ab
# Danach als GitHub-Release mit dem Tag v<version> hochladen (macht .github/workflows/release.yml bei einem Tag-Push).
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
FVER=$(grep -o 'Version = "[0-9.]*"' tools/ultar-flash/Update.cs | cut -d'"' -f2)
[ "$VER" = "$FVER" ] || { echo "Version uneinheitlich: Cargo.toml $VER, Ultar Flash $FVER"; exit 1; }
rm -rf dist; mkdir -p dist
for A in ${ARCHS:-$(uname -m)}; do
  ARCH=$A DEBUG=0 MODULES="" OUT=dist/tmp scripts/build-initramfs.sh
  mv dist/tmp/ultar-initramfs-$A.cpio.zst dist/
done
rm -rf dist/tmp
tools/ultar-flash/build.sh
cp "tools/ultar-flash/UltarFlash-$VER.msi" dist/
(cd tools/ultar-flash && zip -q "../../dist/UltarFlash-$VER.zip" UltarFlash.exe LIESMICH.md)
(cd dist && sha256sum * > SHA256SUMS)
echo "Release v$VER:"; ls -la dist
