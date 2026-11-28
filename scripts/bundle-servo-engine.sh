#!/usr/bin/env bash
# Makes the runtime bundle of the Servo engine host: /usr/lib/ultar/web
#   libexec/ultar-web-engine   the host (loader and run path patched to the bundle)
#   lib/                       glibc + loader, libstdc++, libgcc_s, zlib, fontconfig (+ freetype, expat,
#                              bzip2, libpng, brotli), Mesa (EGL, GBM, llvmpipe, iris) – package servo-laufzeit
#   share/fonts, etc/fonts     the fonts of the bundle and the font configuration that knows only them
# then proves that nothing points outside the bundle and packs an unsigned package pair
# (<stem>.manifest.toml + <stem>.payload.tar.gz, like tools/release/build-apps.sh; signing happens on
# the release server only).
#
# Usage: scripts/bundle-servo-engine.sh --runtime DIR --engine FILE [--out DIR] [--fonts DIR]
#                                       [--version V] [--tests FILE...]
#   --runtime DIR  the packed package servo-laufzeit: <ultar-paketbau work>/out/usr/lib/ultar/web
#   --engine FILE  ultar-web-engine from scripts/build-servo-engine.sh ($TARGET/release/ultar-web-engine)
#   --out DIR      result folder (default out/servo-bundle): usr/lib/ultar/web, the pair, CHECK.txt
#   --fonts DIR    more fonts (*.ttf, *.otf, *.ttc), e.g. Noto Serif and Noto Sans CJK (OFL)
#   --cacert FILE  CA store for share/ssl/cacert.pem (default /etc/ssl/certs/ca-certificates.crt of the builder)
#   --tests FILE.. test programs (examples/shot, ultar-tor-port ...) copied to $OUT/tests, not into the package
# Needs: patchelf, readelf, objdump (binutils), tar, gzip.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
BUNDLE=/usr/lib/ultar/web
runtime=""; engine=""; out="$here/out/servo-bundle"; fonts=""; version="1"; tests=(); cacert=/etc/ssl/certs/ca-certificates.crt
while [ $# -gt 0 ]; do
  case "$1" in
    --runtime) runtime=$2; shift 2 ;;
    --engine) engine=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    --fonts) fonts=$2; shift 2 ;;
    --version) version=$2; shift 2 ;;
    --cacert) cacert=$2; shift 2 ;;
    --tests) shift; while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do tests+=("$1"); shift; done ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -d "$runtime/lib" ] && [ -f "$engine" ] || { echo "usage: $0 --runtime DIR --engine FILE [--out DIR]" >&2; exit 2; }
for t in patchelf readelf objdump tar gzip; do command -v $t >/dev/null || { echo "$t missing" >&2; exit 2; }; done
fail() { echo "BUNDLE: ERROR ($*)" >&2; exit 1; }

root="$out/usr/lib/ultar/web"
rm -rf "$out"; mkdir -p "$root"

# 1) the libraries and what the package brings (without headers: the package's `remove` list did that)
cp -a "$runtime"/. "$root"/
mkdir -p "$root/libexec" "$root/share/fonts" "$root/etc/fonts"
rm -rf "$root/libexec/"* "$root/etc/fonts/conf.d" "$root/share/fontconfig"
# glibc's command line tools are not needed at run time; the font tools (fc-list, fc-match) stay
[ -d "$root/bin" ] && find "$root/bin" -type f ! -name 'fc-*' -delete
find "$root/bin" -type l -delete 2>/dev/null || true

# 1b) stand-in for libwayland-client (surfman's default connection panics without the library; the
#     stand-in reports "no display" so surfman falls back to the surfaceless Mesa display)
command -v gcc >/dev/null || fail "gcc missing (needed for the libwayland-client stand-in)"
gcc -shared -fPIC -O2 -nostdlib -Wl,-soname,libwayland-client.so.0 \
  -o "$root/lib/libwayland-client.so.0" "$here/tools/ultar-paketbau/pakete/servo-laufzeit/pruefung/wayland-client-stub.c"
ln -sf libwayland-client.so.0 "$root/lib/libwayland-client.so"

# 2) the host: the bundle's loader and a run path relative to the program, so no LD_LIBRARY_PATH
install -m 0755 "$engine" "$root/libexec/ultar-web-engine"
patchelf --set-interpreter "$BUNDLE/lib/ld-linux-x86-64.so.2" --set-rpath '$ORIGIN/../lib' "$root/libexec/ultar-web-engine"
# the programs of the package (fontconfig tools) and the checks it ships keep their absolute run path
# $BUNDLE/lib: they are meant for the final place

# 3) fonts: Noto Sans, Naskh and Kufi Arabic of the repository (OFL), DejaVu Sans Mono; more on request
mkdir -p "$root/share/fonts/noto" "$root/share/licenses"
cp "$here"/assets/fonts/*.ttf "$root/share/fonts/noto/"
cp "$here/assets/fonts/OFL.txt" "$here/assets/fonts/LIZENZ.txt" "$root/share/licenses/"
if [ -n "$fonts" ]; then
  mkdir -p "$root/share/fonts/extra"
  find "$fonts" -maxdepth 2 \( -name '*.ttf' -o -name '*.otf' -o -name '*.ttc' \) -exec cp {} "$root/share/fonts/extra/" \;
fi
# the configuration: only the bundle's fonts (relative to this file, so it also works before it is moved)
cat > "$root/etc/fonts/fonts.conf" <<'XML'
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<!-- Ultar web engine: knows only the fonts of the bundle, never those of the machine. -->
<fontconfig>
  <dir prefix="relative">../../share/fonts</dir>
  <cachedir>/tmp/ultar-web-fontconfig</cachedir>
  <!-- Arabic first for Arabic text, Latin for the rest; CJK and Serif come from share/fonts/extra when present -->
  <alias binding="same"><family>sans-serif</family>
    <prefer><family>Noto Sans</family><family>Noto Sans Arabic</family><family>Noto Naskh Arabic</family><family>Noto Kufi Arabic</family><family>Noto Sans CJK SC</family></prefer></alias>
  <alias binding="same"><family>serif</family>
    <prefer><family>Noto Serif</family><family>Noto Naskh Arabic</family><family>Noto Serif CJK SC</family><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>monospace</family>
    <prefer><family>Noto Sans Mono</family><family>DejaVu Sans Mono</family></prefer></alias>
  <!-- names pages ask for -->
  <alias binding="same"><family>Arial</family><prefer><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>Helvetica</family><prefer><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>Times New Roman</family><prefer><family>Noto Serif</family><family>Noto Naskh Arabic</family></prefer></alias>
  <alias binding="same"><family>Courier New</family><prefer><family>Noto Sans Mono</family></prefer></alias>
  <alias binding="same"><family>system-ui</family><prefer><family>Noto Sans</family></prefer></alias>
</fontconfig>
XML

# 3b) the certificate store (Servo's platform verifier panics without one); the host sets SSL_CERT_FILE
mkdir -p "$root/share/ssl"
[ -f "$cacert" ] || fail "no CA store at $cacert (give --cacert FILE, Mozilla's cacert.pem)"
install -m 0644 "$cacert" "$root/share/ssl/cacert.pem"

# 4) proof that nothing points outside the bundle
check="$out/CHECK.txt"; : > "$check"
elfs=$(find "$root" -type f | while read -r f; do [ "$(head -c4 "$f" 2>/dev/null | od -An -c | tr -d ' ')" = '177ELF' ] && echo "$f"; done)
[ -n "$elfs" ] || fail "no programs or libraries in the bundle"
n=0; bad=0
ldso_ver=$("$root/lib/ld-linux-x86-64.so.2" --version | head -1 | grep -oE '[0-9]+\.[0-9]+' | tail -1)
cxx_max=$(objdump -T "$root/lib/libstdc++.so.6" | grep -oE 'GLIBCXX_[0-9.]+' | sort -Vu | tail -1 || true)
[ -n "$cxx_max" ] || fail "the bundle's libstdc++ has no GLIBCXX versions"
while read -r f; do
  rel=${f#"$root"/}; n=$((n+1))
  # interpreter: none, or the bundle's loader
  interp=$(readelf -l "$f" 2>/dev/null | sed -n 's/.*Requesting program interpreter: \(.*\)\]/\1/p')
  if [ -n "$interp" ] && [ "$interp" != "$BUNDLE/lib/ld-linux-x86-64.so.2" ]; then echo "OUTSIDE interpreter $interp in $rel" | tee -a "$check"; bad=$((bad+1)); fi
  # run paths: relative to the object or inside the bundle
  for rp in $(readelf -d "$f" 2>/dev/null | sed -n 's/.*(\(RPATH\|RUNPATH\)).*\[\(.*\)\]/\2/p' | tr ':' ' '); do
    case "$rp" in '$ORIGIN'*|"$BUNDLE"/*|"$BUNDLE") ;; *) echo "OUTSIDE run path $rp in $rel" | tee -a "$check"; bad=$((bad+1)) ;; esac
  done
  # every library it needs is in the bundle
  for need in $(readelf -d "$f" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    # next to the object itself (glibc's gconv modules) or in lib/
    [ -e "$root/lib/$need" ] || [ -e "$(dirname "$f")/$need" ] || { echo "MISSING $need needed by $rel" | tee -a "$check"; bad=$((bad+1)); }
  done
  # symbol versions the bundle's own glibc and libstdc++ do not offer
  need_glibc=$(objdump -T "$f" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1 | sed 's/GLIBC_//' || true)
  if [ -n "$need_glibc" ] && [ "$(printf '%s\n%s\n' "$need_glibc" "$ldso_ver" | sort -V | tail -1)" != "$ldso_ver" ]; then
    echo "TOO NEW GLIBC_$need_glibc in $rel (bundle has $ldso_ver)" | tee -a "$check"; bad=$((bad+1))
  fi
  need_cxx=$(objdump -T "$f" 2>/dev/null | grep -oE 'GLIBCXX_[0-9.]+' | sort -Vu | tail -1 || true)
  if [ -n "$need_cxx" ] && [ "$(printf '%s\n%s\n' "$need_cxx" "$cxx_max" | sort -V | tail -1)" != "$cxx_max" ]; then
    echo "TOO NEW $need_cxx in $rel (bundle has $cxx_max)" | tee -a "$check"; bad=$((bad+1))
  fi
done <<< "$elfs"
# the host resolves everything by itself: the bundle's loader lists its libraries without any environment
host_list=$(env -i "$root/lib/ld-linux-x86-64.so.2" --list "$root/libexec/ultar-web-engine" 2>&1) || fail "loader cannot list the host: $host_list"
echo "$host_list" | sed 's/^/host: /' >> "$check"
# (libraries the bundle's own libraries ask for by their absolute run path /usr/lib/ultar/web/lib
# count as inside: that is where the bundle lives)
outside=$(echo "$host_list" | awk '/=>/ {print $3}' | grep -v -e "^$root/" -e "^$BUNDLE/" || true)
[ -z "$outside" ] || { echo "OUTSIDE (host resolves to): $outside" | tee -a "$check"; bad=$((bad+1)); }
echo "checked $n programs and libraries, glibc $ldso_ver, $cxx_max, problems: $bad" | tee -a "$check"
[ "$bad" = 0 ] || fail "$bad problems, see $check"

# 5) the unsigned package pair
stem="servo-bundle-$version"
payload="$out/$stem.payload.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -C "$out" -cf - usr | gzip -9n > "$payload"
cat > "$out/$stem.manifest.toml" <<EOF
# Runtime bundle of the Servo engine host (scripts/bundle-servo-engine.sh); signed on the release server.
id          = "servo-bundle"
version     = "$version"
publisher   = "km-code-craft"
kind        = "runtime"
arch        = "x86_64"
exec        = "$BUNDLE/libexec/ultar-web-engine"
description = "Runtime of the browser engine: own glibc, libstdc++, fontconfig, Mesa (EGL, llvmpipe, iris), fonts"
EOF
# test programs (kept out of the package): the bundle's loader and libraries, for the kernel run
for t in "${tests[@]}"; do
  mkdir -p "$out/tests"; install -m 0755 "$t" "$out/tests/"
  patchelf --set-interpreter "$BUNDLE/lib/ld-linux-x86-64.so.2" --set-rpath "$BUNDLE/lib" "$out/tests/$(basename "$t")"
  for need in $(readelf -d "$t" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    [ -e "$root/lib/$need" ] || fail "test program $(basename "$t") needs $need, which the bundle does not have"
  done
done
(cd "$out" && sha256sum "$stem.payload.tar.gz" "$stem.manifest.toml" > SHA256SUMS)
echo "BUNDLE: OK $root ($(du -sh "$root" | cut -f1)), $(stat -c %s "$payload") bytes packed, $n objects checked"
