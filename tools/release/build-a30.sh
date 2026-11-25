#!/usr/bin/env bash
# Ultar OS (phone edition) for the Samsung Galaxy A30 (SM-A305F) on Samsung's own Linux 4.4 kernel:
#   out/release/ultar-os-<ver>-phone-a30-linux.zip   Ultar Flash package: system.img (Samsung sparse ext4),
#                                                    vbmeta.img (empty, verification off), ultar-package.txt
# Why the system partition (claude/A30-BOOT-UND-DISPLAY.md): the bootloader passes `skip_initramfs ... root=
# /dev/mmcblk0p25 init=/init`, so the stock kernel mounts the system partition as / and starts /init - Ultar's
# init. Boot partition, bootloader, modem and EFS stay Samsung's; Ultar Flash writes only system and vbmeta
# and refuses without a checked backup (tools/ultar-flash/LIESMICH.md). The display goes through DECON
# (ultar-hal-gfx::decon), the same way the first Ultar shell showed its picture on this phone.
# There is no installer on the phone: without an account the lock screen opens by itself.
# Needs (builder .8): rustc/cargo 1.97 + rust-src (Ubuntu rust-1.97), gcc-aarch64-linux-gnu with the arm64 libc
# (libc6-dev-arm64-cross), e2fsprogs, python3, zip. Experimental: built and checked here, not started on a phone.
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0
fail() { echo "BUILD-A30: FEHLER ($*)"; exit 1; }
for tool in aarch64-linux-gnu-gcc mke2fs debugfs python3 zip cargo; do command -v "$tool" >/dev/null || fail "tool missing: $tool"; done
VER=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VER" ] || fail "workspace version"
TARGET=aarch64-unknown-linux-gnu
TDIR=${CARGO_TARGET_DIR:-target}-a30
W=out/a30; REL=out/release; NAME=ultar-os-$VER-phone-a30-linux
rm -rf "$W"; mkdir -p "$W/pkg" "$REL"

echo "== 1/4 programs for $TARGET (static, standard library from source)"
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
CRATES="ultar-init ultar-busd ultar-logd ultar-settingsd ultar-devd ultar-compositor ultar-auth ultar-mobile
  ultar-launcher ultar-sessiond ultar-powerd ultar-notifyd ultar-calendard ultar-audiod ultar-mediad ultar-captured
  ultar-netd ultar-btd ultar-volumed ultar-mesh ultar-agentd ultar-llm-bus ultar-llm-cloud ultar-displayd
  ultar-accountd ultar-call ultar-pkg ultar-settings ultar-files ultar-store ultar-phone ultar-web ultar-editor ultar-terminal"
args=(); for c in $CRATES; do args+=(-p "$c"); done
CARGO_TARGET_DIR=$TDIR RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target $TARGET \
  -Z build-std=std,panic_abort "${args[@]}" --features ultar-store/gui --bins >"$W/build.log" 2>&1 \
  || { tail -30 "$W/build.log"; fail "program build"; }
BIN=$TDIR/$TARGET/release
# the signing tools run on the builder
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target} cargo build --release -p ultar-pkg --bins >"$W/host.log" 2>&1 \
  || { tail -20 "$W/host.log"; fail "host tools"; }
TOOL_BIN=${CARGO_TARGET_DIR:-target}/release

echo "== 2/4 system tree (phone edition)"
EDITION=phone TREE_ONLY=1 BIN=$BIN TOOL_BIN=$TOOL_BIN LANG_CODE=${LANG_CODE:-ar} bash tools/vm/make-vmware.sh
R=out/vmware/UltarOS-v2-phone-tree
[ -x "$R/sbin/ultar-init" ] || fail "tree without ultar-init"
file -L "$R/sbin/ultar-init" | grep -q "ARM aarch64" || fail "ultar-init is not an arm64 program"
ln -sf sbin/ultar-init "$R/init"            # the stock kernel starts init=/init
mkdir -p "$R/dev" "$R/proc" "$R/sys" "$R/run" "$R/tmp" "$R/data"

echo "== 3/4 system.img (ext4 for Linux 4.4, Samsung sparse) and vbmeta.img"
MIB=$(($(du -sm "$R" | cut -f1) * 13 / 10 + 64))
mke2fs -q -t ext4 -b 4096 -I 256 -L ultar-system \
  -O ^metadata_csum,^64bit,^huge_file,^metadata_csum_seed,^orphan_file,^fast_commit -E root_owner=0:0 -d "$R" "$W/system.raw" "${MIB}M" \
  || fail "mke2fs"
# device nodes the kernel's console and Rust's runtime need before devtmpfs is mounted
debugfs -w -f - "$W/system.raw" >/dev/null 2>&1 <<'NODES'
cd /dev
mknod null c 1 3
mknod zero c 1 5
mknod console c 5 1
mknod kmsg c 1 11
mknod urandom c 1 9
sif null mode 020666
sif zero mode 020666
sif urandom mode 020666
sif console mode 020600
sif kmsg mode 020600
NODES
debugfs -R 'ls -l /dev' "$W/system.raw" 2>/dev/null | grep -q console || fail "device nodes"
python3 - "$W/system.raw" "$W/pkg/system.img" <<'SPARSE'
# Samsung's download mode takes sparse images only: one raw chunk with the whole file.
import struct, sys
src, dst = sys.argv[1], sys.argv[2]
data = open(src, 'rb').read()
blk = 4096
assert len(data) % blk == 0
n = len(data) // blk
with open(dst, 'wb') as f:
    f.write(struct.pack('<IHHHHIIII', 0xed26ff3a, 1, 0, 28, 12, blk, n, 1, 0))
    f.write(struct.pack('<HHII', 0xCAC1, 0, n, 12 + len(data)))
    f.write(data)
SPARSE
python3 - "$W/pkg/vbmeta.img" <<'VBMETA'
# empty, unsigned vbmeta with verification off (flags 2), as Ultar Flash writes it (Package.cs PatchVbmeta)
import sys
o = bytearray(256)
o[0:4] = b'AVB0'; o[7] = 1; o[123] = 2; o[128:141] = b'avbtool 1.1.0'
open(sys.argv[1], 'wb').write(o)
VBMETA
rm -f "$W/system.raw"
{
  echo "# Ultar-Paket: Ultar OS $VER, phone edition, Samsung Galaxy A30 (SM-A305F) on its Linux kernel - experimental"
  echo "device=a30"
  echo "image.system=system.img"; echo "sha256.system=$(sha256sum "$W/pkg/system.img" | cut -d' ' -f1)"
  echo "image.vbmeta=vbmeta.img"; echo "sha256.vbmeta=$(sha256sum "$W/pkg/vbmeta.img" | cut -d' ' -f1)"
} >"$W/pkg/ultar-package.txt"

echo "== 4/4 package"
cat >"$W/pkg/README.txt" <<README
Ultar OS $VER - phone edition for the Samsung Galaxy A30 (SM-A305F), on Samsung's Linux kernel. EXPERIMENTAL.
Writes only the system and vbmeta partitions; boot, bootloader, modem and EFS stay as they are.
Flash with Ultar Flash (Windows): back up the phone first (full backup with root or TWRP), then
"Ultar OS flashen" with this folder. Back to Android: "Original wiederherstellen" with that backup.
Ultar OS ${VER} - إصدار الهاتف لـ Samsung Galaxy A30 على نواة Linux من Samsung. تجريبي.
README
rm -f "$REL/$NAME.zip"
(cd "$W/pkg" && zip -q -9 "../../../$REL/$NAME.zip" ultar-package.txt system.img vbmeta.img README.txt)
(cd "$REL" && sha256sum "$NAME.zip" >"$NAME.zip.sha256" && cat "$NAME.zip.sha256")
rm -rf "$R"
echo "BUILD-A30: ALLES OK ($REL/$NAME.zip)"
