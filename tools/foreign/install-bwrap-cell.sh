#!/bin/sh
# Puts bwrap-cell (tools/foreign/bwrap-cell.c) into a Linux area in place of bubblewrap:
# helpers that programs of the area start through bwrap (GTK's image loaders "glycin") then run
# directly in the app's own cell, which already is the sandbox (docs/integration/ultar-foreign.md).
# Ubuntu's bwrap stays in the area, diverted to /usr/bin/bwrap.ubuntu (dpkg keeps it there on
# updates of bubblewrap); bwrap-cell is static, never set-id.
#
#   tools/foreign/install-bwrap-cell.sh AREA      (as root, on a Linux build machine)
set -eu
fail() { echo "install-bwrap-cell: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
HERE=$(cd "$(dirname "$0")" && pwd)
AREA=$(cd "${1:?the Linux area}" && pwd)
[ -f "$AREA/etc/ultar-runtime" ] || fail "$AREA is no Linux area"
# A Linux area of make-linux-runtime.sh that already brings its own stand-in (package bubblewrap
# from tools/foreign/bwrap-shim.c) keeps it.
if grep -qs 'bwrap (Ultar cell)' "$AREA/usr/bin/bwrap"; then
  echo "install-bwrap-cell: $AREA/usr/bin/bwrap is the area's own stand-in"
  exit 0
fi
CC=${CC:-cc}
command -v "$CC" >/dev/null || fail "no C compiler ($CC)"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$CC" -static -O2 -Wall -Wextra -Werror -o "$tmp/bwrap" "$HERE/bwrap-cell.c" || fail "build failed"
# the stand-in runs a command and passes on what it may
"$tmp/bwrap" --unshare-all --die-with-parent --clearenv --setenv ULTAR_T ok --ro-bind /usr /usr \
  --dev /dev --tmpfs /tmp-home --chdir / -- /bin/sh -c 'test "$ULTAR_T" = ok && test "$PWD" = /' \
  || fail "self-test failed"
if [ -e "$AREA/usr/bin/bwrap" ] && ! grep -qs 'bwrap-cell' "$AREA/usr/bin/bwrap"; then
  chroot "$AREA" dpkg-divert --local --rename --divert /usr/bin/bwrap.ubuntu --add /usr/bin/bwrap >/dev/null
fi
install -m 0755 -o root -g root "$tmp/bwrap" "$AREA/usr/bin/bwrap"
[ -z "$(find "$AREA/usr/bin/bwrap" "$AREA/usr/bin/bwrap.ubuntu" -perm /6000 2>/dev/null)" ] \
  || chmod ug-s "$AREA/usr/bin/bwrap.ubuntu"
echo "install-bwrap-cell: $AREA/usr/bin/bwrap"
