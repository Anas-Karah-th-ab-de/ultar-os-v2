#!/usr/bin/env bash
# Baut aus einem Checkout von universal-pos-erp das Ultar-App-Paket kmpos-server_<version>_<arch>.deb.
# Diese Datei als Release-Anhang im GitHub-Projekt hochladen – Ultar OS installiert dann daraus.
#   scripts/package-kmpos.sh /pfad/zu/universal-pos-erp [out/]
set -euo pipefail
SRC=$(cd "${1:?Pfad zu universal-pos-erp}" && pwd); OUT=$(mkdir -p "${2:-out}" && cd "${2:-out}" && pwd)
ARCH=$(dpkg --print-architecture)
VER=$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)
(cd "$SRC" && cargo build --release -p sync-server --bin pos-sync-server)
P=$(mktemp -d); trap 'rm -rf "$P"' EXIT
install -Dm755 "$SRC/target/release/pos-sync-server" "$P/usr/bin/pos-sync-server"
install -Dm644 "$SRC/config/default_tenant.toml" "$P/etc/kmpos/tenant.toml"
mkdir -p "$P/usr/share/ultar/services" "$P/DEBIAN"
cat > "$P/usr/share/ultar/services/kmpos-server.toml" <<'T'
# KM POS Server (Master-Knoten) – läuft im Ubuntu-Bereich, Daten unter /data/kmpos
exec = "/usr/bin/pos-sync-server"
args = ["/etc/kmpos/tenant.toml", "/data/kmpos/master.sqlite"]
compat = true
restart = "always"
after = ["net"]
T
cat > "$P/DEBIAN/control" <<C
Package: kmpos-server
Version: $VER
Architecture: $ARCH
Maintainer: KM Code Craft <ak@code-craft.tech>
Depends: libc6
Section: misc
Priority: optional
Description: KM Code Craft Universal POS – Sync-Server (Master-Knoten)
C
printf '#!/bin/sh\nmkdir -p /data/kmpos\n' > "$P/DEBIAN/postinst"; chmod 755 "$P/DEBIAN/postinst"
echo "/etc/kmpos/tenant.toml" > "$P/DEBIAN/conffiles"
dpkg-deb --root-owner-group -b "$P" "$OUT/kmpos-server_${VER}_${ARCH}.deb"
