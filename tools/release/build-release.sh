#!/usr/bin/env bash
# Public OS release files from a clean checkout (run on builder B2 through rb.sh, see docs/integration/release.md §3):
#   out/release/ultar-os-<ver>-installer-x86_64.img.xz   USB installer (raw UEFI GPT image, xz), graphical installer
#                                                       session; NO preset account or password (installer's
#                                                       account step + ultar-firstboot)
#   out/release/ultar-os-<ver>-installer-x86_64-vmware.zip  the same installer as a VMware machine (no account)
#   out/release/update-<edition>/                        system update: kernel.efi, system.ext4(.xz), manifest.txt
#                                                       (unsigned), uupdate-sign; signed + bundled by publish-update.sh
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
# The same kernel and system tree also become the system update (online and from a stick): payloads plus the
# unsigned manifest; publish-update.sh signs it on the release server and builds the .uup there.
UPD=$REL/update-$EDITION; rm -rf "$UPD"
EDITION=$EDITION INSTALLER_IMG=1 UPDATE_OUT=$UPD bash tools/vm/make-vmware.sh
IFS=. read -r V_MAJ V_MIN V_PAT <<<"$VER"
NOW=$(date +%s)
SIGNER=${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/uupdate-sign
[ -x "$SIGNER" ] || { echo "BUILD-RELEASE: FEHLER (uupdate-sign missing)"; exit 1; }
"$SIGNER" manifest --out "$UPD/manifest.txt" --version "$VER" --rollback-index $((V_MAJ * 1000000 + V_MIN * 1000 + V_PAT)) \
  --channel "${UPDATE_CHANNEL:-stable}" --edition "$EDITION" --arch x86_64 --devices '*' \
  --min-version "${UPDATE_MIN_VERSION:-0.8.1}" --notes-key update.notes.generic \
  --created $((NOW - 600)) --expires $((NOW + 365 * 86400)) \
  --payload "kernel=$UPD/kernel.efi" --payload "system=$UPD/system.ext4" --lang en >/dev/null
cp "$SIGNER" "$UPD/uupdate-sign"
(cd "$UPD" && xz -T0 -6 -k system.ext4 && sha256sum kernel.efi system.ext4 system.ext4.xz manifest.txt uupdate-sign >SHA256SUMS)
echo "== update payloads and manifest: $UPD"; cat "$UPD/manifest.txt"
xz -T0 -6 -c "out/vmware/$N-installer.img" >"$REL/$IMG.xz"
cp "out/vmware/$N-installer.png" "$REL/$SHOT.png" 2>/dev/null || cp "out/vmware/$N-installer.ppm" "$REL/$SHOT.ppm"
rm -f "out/vmware/$N-installer.img"
# The VMware machine of the installer (medium as SATA disk, empty NVMe target, no account): public as well.
cp "out/vmware/$N-installer.zip" "$REL/${IMG%.img}-vmware.zip"
if [ -n "${VMWARE:-}" ]; then
  bash tools/vm/make-vmware.sh
  cp out/vmware/UltarOS-v2.zip "$REL/ultar-os-$VER-vmware-PRIVATE.zip"
fi
(cd "$REL" && for f in *.xz *.zip; do [ -f "$f" ] && sha256sum "$f" >"$f.sha256"; done; ls -la; cat ./*.sha256)
echo "BUILD-RELEASE: ALLES OK ($VER)"
