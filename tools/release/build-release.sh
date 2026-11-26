#!/usr/bin/env bash
# Public OS release files from a clean checkout (run on builder B2 through rb.sh, see docs/integration/release.md §3):
#   out/release/ultar-os-<ver>-installer-x86_64.img.xz   USB installer (raw UEFI GPT image, xz), graphical installer
#                                                       session; NO preset account or password (installer's
#                                                       account step + ultar-firstboot)
#   out/release/*.sha256, installer-screen.png           digest, screenshot of the QEMU start (INSTALLER-CHECK)
# VMWARE=1 also builds the VMware zip (tools/vm/make-vmware.sh). That image has a preset account with a random
# per-build password (out/vmware/UltarOS-v2.password) and no first-start account setup: it is for the owner's
# tests and is NOT published (docs/integration/release.md, open points).
# Signing and publishing happen on the release server (publish-release.sh); nothing here holds a release key.
set -euo pipefail
cd "$(dirname "$0")/../.."
VER=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VER" ] || { echo "BUILD-RELEASE: FEHLER (workspace version)"; exit 1; }
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "BUILD-RELEASE: FEHLER (checkout not clean)"; git status --short | head; exit 1
fi
REL=out/release; [ -n "${KEEP_RELEASE:-}" ] || rm -rf "$REL"; mkdir -p "$REL"
echo "== Ultar OS $VER from $(git rev-parse --short HEAD)"
# EDITION=phone: the phone edition on the Ultar kernel (x86_64 phones/tablets, QEMU)
EDITION=${EDITION:-desktop}
if [ "$EDITION" = phone ]; then N=UltarOS-v2-phone; IMG=ultar-os-$VER-phone-installer-x86_64.img; SHOT=phone-installer-screen
else N=UltarOS-v2; IMG=ultar-os-$VER-installer-x86_64.img; SHOT=installer-screen; fi
EDITION=$EDITION INSTALLER_IMG=1 bash tools/vm/make-vmware.sh
xz -T0 -6 -c "out/vmware/$N-installer.img" >"$REL/$IMG.xz"
cp "out/vmware/$N-installer.png" "$REL/$SHOT.png" 2>/dev/null || cp "out/vmware/$N-installer.ppm" "$REL/$SHOT.ppm"
rm -f "out/vmware/$N-installer.img"
if [ -n "${VMWARE:-}" ]; then
  bash tools/vm/make-vmware.sh
  cp out/vmware/UltarOS-v2.zip "$REL/ultar-os-$VER-vmware-PRIVATE.zip"
fi
(cd "$REL" && for f in *.xz *.zip; do [ -f "$f" ] && sha256sum "$f" >"$f.sha256"; done; ls -la; cat ./*.sha256)
echo "BUILD-RELEASE: ALLES OK ($VER)"
