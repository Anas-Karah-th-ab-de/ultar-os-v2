#!/bin/sh
# LUKS2 interoperability of ultar-luks with cryptsetup (host test, needs cryptsetup and sudo
# for the dm-crypt part). Both directions:
#  1. ultar-luks formats and encrypts an ext4 image -> cryptsetup reads the header, accepts the
#     passphrase, dm-crypt decrypts the same bytes;
#  2. cryptsetup formats, dm-crypt encrypts -> ultar-luks opens and decrypts the same bytes.
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -q -p ultar-luks
TOOL="$PWD/target/release/ultar-luks"
W=$(mktemp -d)
trap 'sudo -n cryptsetup close ultar-luks-t1 2>/dev/null || true; sudo -n cryptsetup close ultar-luks-t2 2>/dev/null || true; rm -rf "$W"' EXIT
PASS="interop-passwort-1"
MB=32

mkdir "$W/tree"
echo "ultar luks interop" > "$W/tree/probe.txt"
mke2fs -q -t ext4 -d "$W/tree" "$W/plain.img" ${MB}M

echo "== 1: ultar-luks -> cryptsetup"
truncate -s $((16 + MB))M "$W/a.img"
echo "$PASS" | "$TOOL" format "$W/a.img" "$W/plain.img" --test
cryptsetup luksDump "$W/a.img" | grep -E "Version|Cipher|PBKDF|offset" | head -8
printf '%s' "$PASS" | cryptsetup open --test-passphrase --key-file=- "$W/a.img"
if printf 'falsch' | cryptsetup open --test-passphrase --key-file=- "$W/a.img" 2>/dev/null; then
  echo "FEHLER: falsches Passwort angenommen"; exit 1
fi
printf '%s' "$PASS" | sudo -n cryptsetup open --key-file=- "$W/a.img" ultar-luks-t1
sudo -n cmp -n $((MB << 20)) /dev/mapper/ultar-luks-t1 "$W/plain.img"
sudo -n cryptsetup close ultar-luks-t1
echo "ok 1"

echo "== 2: cryptsetup -> ultar-luks"
truncate -s $((16 + MB))M "$W/b.img"
printf '%s' "$PASS" | cryptsetup luksFormat --batch-mode --type luks2 --cipher aes-xts-plain64 \
  --key-size 512 --sector-size 512 --pbkdf argon2id --pbkdf-memory 1024 \
  --pbkdf-force-iterations 4 --key-file=- "$W/b.img"
printf '%s' "$PASS" | sudo -n cryptsetup open --key-file=- "$W/b.img" ultar-luks-t2
sudo -n dd if="$W/plain.img" of=/dev/mapper/ultar-luks-t2 bs=1M conv=fsync status=none
sudo -n cryptsetup close ultar-luks-t2
echo "$PASS" | "$TOOL" check "$W/b.img"
echo "$PASS" | "$TOOL" decrypt "$W/b.img" "$W/back.img" $((MB << 20))
cmp "$W/back.img" "$W/plain.img"
if echo falsch | "$TOOL" check "$W/b.img" 2>/dev/null; then
  echo "FEHLER: falsches Passwort angenommen"; exit 1
fi
echo "ok 2"
echo "LUKS2-Interop: bestanden"
