#!/usr/bin/env bash
# Builds every app of tools/release/apps.list as an UNSIGNED package pair for the store:
#   <out>/<id>-<version>.manifest.toml   the app's ultar-app.toml + arch + exec (signed bytes later)
#   <out>/<id>-<version>.payload.tar.gz  usr/lib/ultar/apps/<id>/<binary> (+ assets), reproducible tar
#   <out>/apps.tsv                       id, version, key role, file stem
#   <out>/ultar-catalog.in.toml          the catalogue with @SIZE:/@SHA256: placeholders
# Signing happens only on the release server (tools/release/vps/publish-store.sh through the
# signing service); no key is ever on the build machine.
# Runs on a builder (B2): `bash tools/release/build-apps.sh [out-dir]` from the repo root.
# Then: tools/release/publish.ps1 copies <out> to the release server and runs publish-store.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
OUT=${1:-out/store-staging}
TARGET=x86_64-unknown-linux-gnu
ARCH=x86_64
fail() { echo "BUILD-APPS: ERROR ($*)" >&2; exit 1; }
rm -rf "$OUT"; mkdir -p "$OUT"
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT

# 1) one static build of every app binary
PKGS=(); FEATS=()
while read -r crate bin feats role; do
  case "$crate" in ''|'#'*) continue ;; esac
  PKGS+=(-p "$crate")
  [ "$feats" != - ] && FEATS+=("$crate/$feats")
done < tools/release/apps.list
FEATURE_ARG=()
[ ${#FEATS[@]} -gt 0 ] && FEATURE_ARG=(--features "$(IFS=,; echo "${FEATS[*]}")")
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --locked --target $TARGET \
  "${PKGS[@]}" "${FEATURE_ARG[@]}" --bins >"$W/build.log" 2>&1 || { tail -30 "$W/build.log"; fail "cargo build"; }
BIN="${CARGO_TARGET_DIR:-target}/$TARGET/release"

# 2) per app: manifest + payload + catalogue entry
cat tools/release/catalog/header.toml | sed "s/@DATE@/$(date -u +%F)/" >"$OUT/ultar-catalog.in.toml"
: >"$OUT/apps.tsv"
while read -r crate bin feats role; do
  case "$crate" in ''|'#'*) continue ;; esac
  dir=$(cargo metadata --format-version 1 --no-deps --locked | python3 -c '
import json,sys,os
m=json.load(sys.stdin)
p=[x for x in m["packages"] if x["name"]==sys.argv[1]][0]
print(os.path.dirname(p["manifest_path"]))' "$crate")
  man="$dir/ultar-app.toml"
  [ -f "$man" ] || fail "$crate has no ultar-app.toml"
  [ -x "$BIN/$bin" ] || fail "binary missing: $bin"
  file "$BIN/$bin" | grep -Eq "statically linked|static-pie linked" || fail "$bin is not static"
  id=$(sed -n 's/^id *= *"\(.*\)".*/\1/p' "$man" | head -1)
  ver=$(sed -n 's/^version *= *"\(.*\)".*/\1/p' "$man" | head -1)
  [ -n "$id" ] && [ -n "$ver" ] || fail "$man: id/version"
  stem="$id-$ver"
  exec_path="/usr/lib/ultar/apps/$id/$bin"
  # Manifest: arch and exec go before the first [table] (TOML: keys after a table belong to it).
  python3 - "$man" "$ARCH" "$exec_path" >"$OUT/$stem.manifest.toml" <<'PY'
import sys,re
text=open(sys.argv[1],encoding="utf-8").read()
arch,exe=sys.argv[2],sys.argv[3]
lines=text.split("\n")
first=next((i for i,l in enumerate(lines) if re.match(r"^\s*\[",l)),len(lines))
head=[l for l in lines[:first] if not re.match(r"^\s*(arch|exec)\s*=",l)]
add=[f'arch        = "{arch}"']
if not re.search(r'^\s*exec\s*=',"\n".join(lines[:first]),re.M):
    add.append(f'exec        = "{exe}"')
else:
    add.append([l for l in lines[:first] if re.match(r"^\s*exec\s*=",l)][0])
print("\n".join(head+add+[""]+lines[first:]).rstrip("\n"))
PY
  P="$W/payload-$id"; mkdir -p "$P/usr/lib/ultar/apps/$id"
  install -m 0755 "$BIN/$bin" "$P/usr/lib/ultar/apps/$id/$bin"
  [ -d "$dir/assets" ] && cp -r "$dir/assets" "$P/usr/lib/ultar/apps/$id/assets"
  tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -C "$P" -cf - . | gzip -9n >"$OUT/$stem.payload.tar.gz"
  entry="tools/release/catalog/$id.toml"
  [ -f "$entry" ] || fail "no catalogue entry $entry"
  { echo; sed -e "s/@VERSION@/$ver/" -e "s/@PACKAGE@/$stem.uap/" \
      -e "s/@SIZE@/@SIZE:$stem.uap@/" -e "s/@SHA256@/@SHA256:$stem.uap@/" "$entry"; } >>"$OUT/ultar-catalog.in.toml"
  printf '%s\t%s\t%s\t%s\n' "$id" "$ver" "$role" "$stem" >>"$OUT/apps.tsv"
  echo "BUILD-APPS: $stem ($role) $(stat -c %s "$OUT/$stem.payload.tar.gz") bytes"
done < tools/release/apps.list
(cd "$OUT" && sha256sum ./* >SHA256SUMS)
echo "BUILD-APPS: OK $(wc -l <"$OUT/apps.tsv") apps in $OUT"
