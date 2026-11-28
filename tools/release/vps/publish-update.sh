#!/usr/bin/env bash
# Signs and publishes a system update on the release server (docs/SYSTEM-UPDATE.md). Run as root.
#
#   publish-update.sh keygen                      one time: makes the update key, prints the public key (hex)
#   publish-update.sh publish <dir> [channel]     <dir> from tools/release/build-release.sh (update-<edition>/:
#                                                 kernel.efi, system.ext4.xz, manifest.txt, uupdate-sign, SHA256SUMS)
#
# The secret key stays here: /var/lib/ultar-sign/update/update.secret (root, 0400, directory 0700). Devices trust
# the public key from rootfs/etc/ultar/update-keys/. Nothing is built here: the manifest, the payloads and the
# signing tool come from the builder; this script only checks them, signs the manifest, joins the bundle (.uup),
# verifies it with the public key and hands it to ultraos-server (`admin publish-release`), which serves it at
# /v1/update/latest and /v1/update/file/<id>.
set -euo pipefail
KEYDIR=/var/lib/ultar-sign/update
SECRET=$KEYDIR/update.secret
PUBLIC=$KEYDIR/km-code-craft.ed25519
VOLUME=/var/lib/docker/volumes/ultar_ultar_daten/_data
fail() { echo "PUBLISH-UPDATE: FEHLER ($*)"; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"

case "${1:-}" in
keygen)
  TOOL=${2:?the uupdate-sign program}
  [ -e "$SECRET" ] && fail "$SECRET exists; rotate with uupdate-sign rotate, never overwrite"
  install -d -m 0700 -o root -g root "$KEYDIR"
  (umask 077; "$TOOL" keygen --secret "$SECRET" --public "$PUBLIC" --lang en >/dev/null)
  chmod 0400 "$SECRET"; chmod 0444 "$PUBLIC"
  cat "$PUBLIC"
  ;;
publish)
  DIR=$(realpath "${2:?the update directory}")
  CHANNEL=${3:-stable}
  [ -r "$SECRET" ] || fail "no update key; run: publish-update.sh keygen <uupdate-sign>"
  cd "$DIR"
  [ -f system.ext4 ] || xz -d -k system.ext4.xz
  sha256sum -c --quiet SHA256SUMS || fail "checksums"
  chmod 0755 uupdate-sign
  VER=$(sed -n 's/^version=//p' manifest.txt)
  EDITION=$(sed -n 's/^edition=//p' manifest.txt)
  ARCH=$(sed -n 's/^arch=//p' manifest.txt)
  [ "$(sed -n 's/^channel=//p' manifest.txt)" = "$CHANNEL" ] || fail "the manifest is for another channel"
  OUTF=ultar-os-$VER-$EDITION-$ARCH.uup
  ./uupdate-sign sign --manifest manifest.txt --key "$SECRET" --out manifest.sig --lang en >/dev/null
  ./uupdate-sign bundle --signed manifest.sig --out "$OUTF" --payload kernel=kernel.efi --payload system=system.ext4 --lang en >/dev/null
  KEYS=$(mktemp -d); cp "$PUBLIC" "$KEYS/"
  ./uupdate-sign verify --bundle "$OUTF" --keys "$KEYS" --lang en || fail "the bundle does not verify with the public key"
  rm -rf "$KEYS"
  # ultraos-server keeps two legacy fields per release (the device checks the bundle itself): the
  # manifest's digest and the Ed25519 signature from the signed manifest (its last 64 bytes).
  ROOTHASH=$(sha256sum manifest.txt | cut -d' ' -f1)
  SIG=$(tail -c 64 manifest.sig | base64 -w0)
  INC=$VOLUME/incoming; install -d -m 0755 "$INC"
  cp "$OUTF" "$INC/"
  printf '{"channel":"%s","version":"%s","arch":"%s","edition":"%s","roothash":"%s","sig":"%s"}\n' \
    "$CHANNEL" "$VER" "$ARCH" "$EDITION" "$ROOTHASH" "$SIG" >"$INC/$OUTF.json"
  chmod 0644 "$INC/$OUTF" "$INC/$OUTF.json"
  docker exec ultar_server ultraos-server admin publish-release "/var/lib/ultraos/incoming/$OUTF.json" \
    "/var/lib/ultraos/incoming/$OUTF" || fail "ultraos-server publish-release"
  rm -f "$INC/$OUTF" "$INC/$OUTF.json"
  sha256sum "$OUTF"
  curl -fsS "http://127.0.0.1:8096/v1/update/latest?channel=$CHANNEL&arch=$ARCH&edition=$EDITION"; echo
  echo "PUBLISH-UPDATE: ALLES OK ($VER, $CHANNEL, $EDITION)"
  ;;
*)
  sed -n '2,7p' "$0"; exit 2 ;;
esac
