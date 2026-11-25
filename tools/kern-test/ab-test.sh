#!/usr/bin/env bash
# A/B gate: the whole start chain with the real updater, in QEMU (OVMF).
#   install (graphical installer, A/B layout) → start slot A → offline update to B (ultar-updated + uupdate,
#   .uup from a stick) → restart into B (trial) → confirm → C: kernel panics, given up after its 3 attempts →
#   D: never confirms, given up after its 3 attempts → E: power loss while staging and applying (several
#   points), then confirmed → F: power loss while the loader writes the slot table (6 cuts) → the device
#   starts the slot the table names. Loader time: median over all starts < 50 ms.
# Verdict line: "AB-TEST: ALLES OK (<n> checks)" or "AB-TEST: FEHLER …". Exit code 0 only when all passed.
# Needs: rustc/cargo 1.97 (+ rust-src), qemu-system-x86, ovmf, mtools, dosfstools, e2fsprogs, python3.
# AB_WORK=<dir>: where the run's own fresh work directory is made (default $TMPDIR or /tmp); it is removed
# at the end unless AB_KEEP=1 (then the serial log of every start is in <work>/serial.log).
set -euo pipefail
cd "$(dirname "$0")/../.."
# every tool is required: a missing one is an error, not a skipped step
for tool in qemu-system-x86_64 mke2fs debugfs e2fsck mkfs.vfat mcopy mmd python3 cargo; do
  command -v "$tool" >/dev/null || { echo "AB-TEST: FEHLER (tool missing: $tool)"; exit 1; }
done
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_VARS_4M.fd; do
  [ -f "$f" ] || { echo "AB-TEST: FEHLER (firmware missing: $f)"; exit 1; }
done
ROOT=$PWD
export PATH=$PATH:/usr/sbin:/sbin
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export CARGO_INCREMENTAL=0
mkdir -p "${AB_WORK:-${TMPDIR:-/tmp}}"
W=$(mktemp -d "${AB_WORK:-${TMPDIR:-/tmp}}/ab-test.XXXXXX")
cleanup() { [ -n "${AB_KEEP:-}" ] || rm -rf "$W"; }
trap cleanup EXIT

# 1) Programs: kernel, kernel that panics, loader, updater, agent
# kernel and loader are separate workspaces with their own target directories
env -u CARGO_TARGET_DIR bash scripts/build-kern.sh >/dev/null 2>&1
KERN=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
cp "$KERN" "$W/kernel.efi"
# The broken release: a copy of the kernel source with one panic right after the clock is set up
# (the product source has no such hook)
KP=$W/kpanic
mkdir -p "$KP/kernel" "$KP/assets"
cp -r kernel/ultar-kern "$KP/kernel/"; rm -rf "$KP/kernel/ultar-kern/target"
cp -r assets/fonts "$KP/assets/"
# every path dependency of the kernel, read from its manifest (a new one - the Intel GT and NPU
# drivers - was missing from a fixed list: the copy did not build and the gate ended without a word)
for dep in $(sed -n 's/.*path = "\.\.\/\.\.\/\([^"]*\)".*/\1/p' kernel/ultar-kern/Cargo.toml); do
  mkdir -p "$KP/$(dirname "$dep")"; cp -r "$dep" "$KP/$dep"; rm -rf "${KP:?}/${dep:?}/target"
done
python3 - "$KP/kernel/ultar-kern/src/main.rs" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
anchor = "        bootslot::report();\n"
assert anchor in s
open(p, "w").write(s.replace(anchor, anchor + '        panic!("ab-test-panic: kernel built to fail its start");\n', 1))
PY
( cd "$KP/kernel/ultar-kern" && env -u CARGO_TARGET_DIR RUSTC_BOOTSTRAP=1 cargo build --release -q >"$W/kpanic-build.log" 2>&1 ) \
  || { tail -20 "$W/kpanic-build.log"; echo "AB-TEST: FEHLER (the kernel that panics did not build)"; exit 1; }
cp "$KP/kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi" "$W/kernel-panic.efi"
rm -rf "$KP"
( cd kernel/ultar-loader && env -u CARGO_TARGET_DIR RUSTC_BOOTSTRAP=1 cargo build --release -q )
cp kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi "$W/loader.efi"
UT=${CARGO_TARGET_DIR:-target}
RUSTFLAGS="-C target-feature=+crt-static" CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release -q \
  --target x86_64-unknown-linux-gnu -p ultar-update -p ab-agent --bins
BIN=$UT/x86_64-unknown-linux-gnu/release
SIGN=$BIN/uupdate-sign

# 2) Keys and system images (ext4): A 0.7.0 (factory), B 0.8.0, C 0.9.0 (kernel panics), D 0.10.0, E 0.11.0
"$SIGN" keygen --secret "$W/release.secret" --public "$W/release.ed25519" --lang en >/dev/null
# `mke2fs -d` keeps the owners of the source files: the images must not depend on who builds them
# (a CI runner is not root, and the updater only trusts a state directory owned by itself = root)
root_owned() { # image source-dir
  local cmds; cmds=$(mktemp)
  { printf 'sif / uid 0\nsif / gid 0\n'
    ( cd "$2" && find . -mindepth 1 | sed 's|^\.||' ) | while read -r p; do printf 'sif %s uid 0\nsif %s gid 0\n' "$p" "$p"; done
  } > "$cmds"
  debugfs -w -f "$cmds" "$1" >/dev/null 2>&1
  rm -f "$cmds"
  [ "$(debugfs -R 'stat /' "$1" 2>/dev/null | sed -n 's/.*User: *\([0-9]*\).*/\1/p')" = 0 ] || { echo "AB-TEST: FEHLER (owners of $1)"; exit 1; }
}
system() { # name version index
  local R=$W/root-$1
  mkdir -p "$R/bin" "$R/usr/bin" "$R/etc/ultar/update-keys" "$R/data" "$R/tmp"
  cp "$BIN/ab-agent" "$R/bin/"; cp "$BIN/ultar-updated" "$BIN/uupdate" "$R/usr/bin/"
  cp "$W/release.ed25519" "$R/etc/ultar/update-keys/"
  printf '%s %s\n' "$1" "$2" > "$R/etc/ultar/release"
  cat > "$R/etc/ultar/update.conf" <<CONF
device=vm
arch=x86_64
edition=desktop
version=$2
rollback_index=$3
roles=kernel,system
partition_dir=/dev/ultar/part
running_slot_file=/proc/ultar/boot_slot
state_dir=/data/ultar/update
keys_dir=/etc/ultar/update-keys
admin=system.settings
offline_root=/media
reserve_mib=1
health_service=none
CONF
  mke2fs -q -t ext4 -L "system-$1" -d "$R" "$W/sys-$1.ext4" 16M
  root_owned "$W/sys-$1.ext4" "$R"
  rm -rf "$R"
}
system a 0.7.0 1; system b 0.8.0 2; system c 0.9.0 3; system d 0.10.0 4; system e 0.11.0 5; system f 0.12.0 6
NOW=$(date +%s)
bundle() { # name version index kernel
  local M=$W/m-$1
  "$SIGN" manifest --out "$M.txt" --version "$2" --rollback-index "$3" --channel stable --edition desktop \
    --arch x86_64 --devices vm --min-version 0.7.0 --notes-key update.notes.generic \
    --created $((NOW - 3600)) --expires $((NOW + 30 * 86400)) \
    --payload "kernel=$4" --payload "system=$W/sys-$1.ext4" --lang en >/dev/null
  "$SIGN" sign --manifest "$M.txt" --key "$W/release.secret" --out "$M.sig" --lang en >/dev/null
  "$SIGN" bundle --signed "$M.sig" --out "$W/$1.uup" --payload "kernel=$4" --payload "system=$W/sys-$1.ext4" --lang en >/dev/null
}
bundle b 0.8.0 2 "$W/kernel.efi"; bundle c 0.9.0 3 "$W/kernel-panic.efi"
bundle d 0.10.0 4 "$W/kernel.efi"; bundle e 0.11.0 5 "$W/kernel.efi"; bundle f 0.12.0 6 "$W/kernel.efi"

# 3) Installation medium (GPT): installer, start partition with the loader, kernel, system, data
truncate -s 16M "$W/install.fat"; mkfs.vfat -n ULTAR-INST "$W/install.fat" >/dev/null
mmd -i "$W/install.fat" ::/EFI ::/EFI/BOOT; mcopy -i "$W/install.fat" "$W/kernel.efi" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS installation medium\n' > "$W/marker"; mcopy -i "$W/install.fat" "$W/marker" ::/ultar-installer
truncate -s 16M "$W/esp.fat"; mkfs.vfat -n ULTAR-EFI "$W/esp.fat" >/dev/null
mmd -i "$W/esp.fat" ::/EFI ::/EFI/BOOT ::/ultar; mcopy -i "$W/esp.fat" "$W/loader.efi" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS\n' > "$W/inst"; mcopy -i "$W/esp.fat" "$W/inst" ::/ultar/installiert
mkdir -p "$W/data-root/ultar"; mke2fs -q -t ext4 -L ultar-data -d "$W/data-root" "$W/data.ext4" 64M
root_owned "$W/data.ext4" "$W/data-root"; rm -rf "$W/data-root"
python3 tools/kern-test/gptimg.py "$W/medium.img" "ULTAR-INSTALL:esp:$W/install.fat" "ULTAR-EFI:esp:$W/esp.fat" \
  "ultar-kernel:raw:$W/kernel.efi" "ultar-compat:linux:$W/sys-a.ext4" "ultar-data:linux:$W/data.ext4"
# 4) The stick with the offline bundles (FAT32, no partition table)
rm -f "$W/stick.img"; truncate -s 128M "$W/stick.img"; mkfs.vfat -F 32 -n UPDATES "$W/stick.img" >/dev/null
mcopy -i "$W/stick.img" "$W/b.uup" "$W/c.uup" "$W/d.uup" "$W/e.uup" "$W/f.uup" ::/
rm -f "$W"/sys-*.ext4 "$W"/*.fat "$W/data.ext4"

# 5) The runs in QEMU
python3 tools/kern-test/abtest.py "$W"
