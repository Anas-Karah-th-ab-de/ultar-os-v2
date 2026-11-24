#!/usr/bin/env bash
# Ultar OS v2 as a VMware Workstation machine, built from this tree:
#   out/vmware/UltarOS-v2/UltarOS-v2.vmx     UEFI, 4 GiB, 4 processors, NVMe disk, e1000e (NAT), xHCI, no sound
#   out/vmware/UltarOS-v2/UltarOS-v2.vmdk    the installed disk (monolithicSparse, qemu-img)
#   out/vmware/UltarOS-v2.zip                both files (install on the PC: tools/vm/install-vmware.ps1)
# What the machine starts is what the tree holds today: loader (A/B) -> Ultar kernel -> ultar-init as PID 1 ->
# logd, bus, settingsd, devd, seat, authd -> ultar-compositor on the frame buffer, Arabic, with the desktop
# session on it: ultar-lock, ultar-sysdialog, ultar-desktop (bar, sheets, overview), each as its own user.
# One account: VM_USER (ultar) with the password in VM_PASSWORD_FILE, or a random one written to
# out/vmware/UltarOS-v2.password (mode 0600) - never a fixed password in a delivered image.
# Steps:
#   1. build the kernel, the loader and the static programs (BIN=<dir> uses prebuilt programs instead)
#   2. system tree (ext4) and an installation medium as tools/kern-test/ab-test.sh builds it
#   3. the kernel's own installer writes the A/B layout to an empty disk, in QEMU on VMware-like hardware
#      (medium on AHCI, target on NVMe, e1000e, no virtio) - the disk is exactly what the installer creates
#   4. the installed disk is started once (tools/vm/vmware-vm.py check): services, compositor, screenshot
#   5. qemu-img convert -O vmdk, the .vmx, and the .vmdk is started again on NVMe and on AHCI (CHECK=0 skips)
# Environment: DISK_MIB (4096), LANG_CODE (ar | de | en), KEEP=1 keeps out/vmware/work; for the QEMU runs
# SMP (4), MEM (1024), WAIT, ACCEL (tcg | kvm), see tools/vm/vmware-vm.py.
# Needs: rustc/cargo 1.97 (+ rust-src), gcc, make, qemu-system-x86, qemu-utils (qemu-img), ovmf, mtools,
# dosfstools, e2fsprogs, python3. Nothing is downloaded and nothing outside out/ is written.
# VMware Workstation itself is not started here: QEMU is the test bed (docs/integration/vmware.md).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0
NAME=UltarOS-v2
OUT=out/vmware
VM=$OUT/$NAME
W=$OUT/work
LANG_CODE=${LANG_CODE:-ar}
DISK_MIB=${DISK_MIB:-4096}
fail() { echo "MAKE-VMWARE: FEHLER ($*)"; exit 1; }
for tool in qemu-system-x86_64 qemu-img mke2fs debugfs mkfs.vfat mcopy mmd python3 cargo; do
  command -v "$tool" >/dev/null || fail "tool missing: $tool"
done
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_VARS_4M.fd; do [ -f "$f" ] || fail "firmware missing: $f"; done
case "$LANG_CODE" in ar | de | en) ;; *) fail "LANG_CODE must be ar, de or en" ;; esac
rm -rf "$W" "$VM" "$OUT/$NAME.zip"
mkdir -p "$W" "$VM"
cleanup() { [ -n "${KEEP:-}" ] || rm -rf "$W"; }
trap cleanup EXIT

# 1) Programs
echo "== 1/5 build"
env -u CARGO_TARGET_DIR bash scripts/build-kern.sh >"$W/build-kern.log" 2>&1 || { tail -20 "$W/build-kern.log"; fail "kernel build"; }
env -u CARGO_TARGET_DIR bash scripts/build-loader.sh >"$W/build-loader.log" 2>&1 || { tail -20 "$W/build-loader.log"; fail "loader build"; }
KERNEL=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
LOADER=kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi
PROGRAMS="ultar-init uinit ultar-busd ultar-logd ulog ultar-settingsd usettings ultar-devd ultar-devd-seat udevctl ultar-compositor ultar-authd ultar-lock ultar-sysdialog ultar-desktop"
if [ -z "${BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-settingsd -p ultar-devd -p ultar-compositor -p ultar-auth -p ultar-desktop --bins \
    >"$W/build-programs.log" 2>&1 || { tail -20 "$W/build-programs.log"; fail "program build"; }
  BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
for p in $PROGRAMS; do [ -x "$BIN/$p" ] || fail "program missing: $BIN/$p"; done

# 2) System tree: the shipped configuration of rootfs/etc/ultar, the services this image has programs for
echo "== 2/5 system tree and installation medium"
R=$W/root
mkdir -p "$R"/{bin,sbin,usr/bin,usr/lib/ultar/drivers,etc/ultar/services,data,tmp,run,root,home,var,proc,sys,dev,media}
cp "$BIN/ultar-init" "$R/sbin/"
for p in $PROGRAMS; do [ "$p" = ultar-init ] || cp "$BIN/$p" "$R/usr/bin/"; done
for f in init.toml ids.toml bus-policy.toml compositor-rights.toml devd-seat.toml; do cp "rootfs/etc/ultar/$f" "$R/etc/ultar/"; done
cp -r rootfs/etc/ultar/trusted-keys "$R/etc/ultar/"
for s in logd bus settingsd devd seat authd; do cp "rootfs/etc/ultar/services/$s.toml" "$R/etc/ultar/services/"; done
# The shipped session services are not started by the ordinary system (`manual = true`, or
# `[profiles] system = "manual"`) because the legacy ultar-shell would drive the same display; this image
# has no ultar-shell, so they start with the system. Nothing else of the files changes.
for s in compositor lock sysdialog desktop; do
  f="rootfs/etc/ultar/services/$s.toml"
  if grep -q '^manual = true$' "$f"; then
    grep -v '^manual = true$' "$f" >"$R/etc/ultar/services/$s.toml"
  elif grep -q '^system = "manual"$' "$f"; then
    sed 's/^system = "manual"$/system = "start"/' "$f" >"$R/etc/ultar/services/$s.toml"
  else
    fail "$s.toml: neither 'manual = true' nor 'system = \"manual\"'"
  fi
done
printf 'desktop\n' >"$R/etc/ultar/edition"
printf 'init\n' >"$R/etc/ultar/session"
printf '%s\n' "$LANG_CODE" >"$R/etc/ultar/sprache"
printf 'ultar\n' >"$R/etc/hostname"
printf 'NAME="Ultar OS"\nPRETTY_NAME="Ultar OS v2 (Ultar kernel)"\nID=ultar\nVERSION_ID="0.7.0"\nHOME_URL="https://code-craft.tech"\n' >"$R/etc/os-release"
printf '%s %s\n' "$(sed -n 's/^version = "\(.*\)"/\1/p' kernel/ultar-kern/Cargo.toml | head -1)" "$(git rev-parse --short HEAD 2>/dev/null || echo unknown)" >"$R/etc/ultar/release"
# users and groups from the fixed ids (as scripts/build-initramfs.sh writes them)
awk -v P="$R/etc/passwd" -v G="$R/etc/group" '
  /^\[/ { sec = $0; next }
  /^[a-z][a-z0-9_-]* *= */ {
    name = $1; v = $0; sub(/^[^=]*= */, "", v); sub(/ *#.*$/, "", v)
    if (sec == "[users]") { uid[name] = v; order[++n] = name; gid[name] = v }
    else if (sec == "[groups]") { gid[name] = v; order[++n] = name }
    else if (sec == "[members]") { gsub(/[][" ]/, "", v); mem[name] = v }
  }
  END {
    printf "" > P; printf "" > G
    for (i = 1; i <= n; i++) {
      k = order[i]
      if (k in uid) print k ":x:" uid[k] ":" uid[k] ":" k ":" (k == "root" ? "/root:/bin/sh" : "/nonexistent:/bin/false") >> P
      print k ":x:" gid[k] ":" mem[k] >> G
    }
  }' rootfs/etc/ultar/ids.toml
if [ -x /bin/busybox ] && file -L /bin/busybox | grep -q "statically linked"; then
  cp /bin/busybox "$R/bin/busybox"; /bin/busybox --list >"$R/bin/.applets"
fi
# `mke2fs -d` keeps the owners of the source files: the images must not depend on who builds them
root_owned() { # image source-dir
  local cmds; cmds=$(mktemp)
  { printf 'sif / uid 0\nsif / gid 0\n'
    (cd "$2" && find . -mindepth 1 | sed 's|^\.||') | while read -r p; do printf 'sif %s uid 0\nsif %s gid 0\n' "$p" "$p"; done
  } >"$cmds"
  debugfs -w -f "$cmds" "$1" >/dev/null 2>&1
  rm -f "$cmds"
  [ "$(debugfs -R 'stat /' "$1" 2>/dev/null | sed -n 's/.*User: *\([0-9]*\).*/\1/p')" = 0 ] || fail "owners of $1"
}
SYS_MIB=$(($(du -sm "$R" | cut -f1) * 13 / 10 + 16))
mke2fs -q -t ext4 -L ultar-compat -d "$R" "$W/system.ext4" "${SYS_MIB}M"
root_owned "$W/system.ext4" "$R"
mkdir -p "$W/data-root/ultar/auth"
# the account of the lock screen and the system dialogs (ultar-authd passwd reads the password on stdin)
VM_USER=${VM_USER:-ultar}
PWFILE=${VM_PASSWORD_FILE:-}
if [ -z "$PWFILE" ]; then
  PWFILE=$OUT/$NAME.password; (umask 077; head -c 12 /dev/urandom | base64 | tr -d '/+=' >"$PWFILE")
fi
pw=$(head -1 "$PWFILE")
printf '%s\n%s\n' "$pw" "$pw" | "$BIN/ultar-authd" --dir "$W/data-root/ultar/auth" passwd "$VM_USER" --lang "$LANG_CODE" \
  >/dev/null 2>&1 || fail "account $VM_USER"
unset pw
mke2fs -q -t ext4 -L ultar-data -d "$W/data-root" "$W/data.ext4" 512M
root_owned "$W/data.ext4" "$W/data-root"
# the authentication service (813) owns its directory and files
auth_cmds=$(mktemp)
(cd "$W/data-root" && find ultar/auth | sed 's|^|/|') | while read -r p; do printf 'sif %s uid 813\nsif %s gid 813\n' "$p" "$p"; done >"$auth_cmds"
debugfs -w -f "$auth_cmds" "$W/data.ext4" >/dev/null 2>&1; rm -f "$auth_cmds"
# installation medium (GPT): installer, start partition with the loader, kernel, system, data
truncate -s 16M "$W/install.fat"; mkfs.vfat -n ULTAR-INST "$W/install.fat" >/dev/null
mmd -i "$W/install.fat" ::/EFI ::/EFI/BOOT; mcopy -i "$W/install.fat" "$KERNEL" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS installation medium\n' >"$W/marker"; mcopy -i "$W/install.fat" "$W/marker" ::/ultar-installer
truncate -s 64M "$W/esp.fat"; mkfs.vfat -F 32 -n ULTAR-EFI "$W/esp.fat" >/dev/null
mmd -i "$W/esp.fat" ::/EFI ::/EFI/BOOT ::/ultar; mcopy -i "$W/esp.fat" "$LOADER" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS\n' >"$W/inst"; mcopy -i "$W/esp.fat" "$W/inst" ::/ultar/installiert
printf '%s\n' "$LANG_CODE" >"$W/lang"; mcopy -i "$W/esp.fat" "$W/lang" ::/ultar/sprache
python3 tools/kern-test/gptimg.py "$W/medium.img" "ULTAR-INSTALL:esp:$W/install.fat" "ULTAR-EFI:esp:$W/esp.fat" \
  "ultar-kernel:raw:$KERNEL" "ultar-compat:linux:$W/system.ext4" "ultar-data:linux:$W/data.ext4" >/dev/null
rm -rf "$R" "$W/data-root" "$W"/*.fat "$W"/*.ext4

# 3) Installation on VMware-like hardware
echo "== 3/5 installation (the kernel's installer, A/B layout)"
truncate -s "${DISK_MIB}M" "$W/disk.raw"
python3 tools/vm/vmware-vm.py install "$W/medium.img" "$W/disk.raw" "$LANG_CODE" "$W/install.log" || { tail -30 "$W/install.log"; fail "installation"; }
rm -f "$W/medium.img"

# 4) First start of the installed disk
echo "== 4/5 start of the installed disk"
python3 tools/vm/vmware-vm.py check "$W/disk.raw" raw nvme "$OUT/$NAME-raw" || fail "the installed disk does not come up (log: $OUT/$NAME-raw.log)"

# 5) VMware files
echo "== 5/5 vmdk, vmx, zip"
qemu-img convert -f raw -O vmdk -o subformat=monolithicSparse,adapter_type=lsilogic "$W/disk.raw" "$VM/$NAME.vmdk"
rm -f "$W/disk.raw"
sed -e "s/@NAME@/$NAME/g" tools/vm/UltarOS-v2.vmx.in >"$VM/$NAME.vmx"
if [ "${CHECK:-1}" != 0 ]; then
  # the checks run on a copy-on-write layer: the delivered .vmdk stays as the installer left it.
  # NVMe as in the .vmx, with the VMware SVGA II adapter (GOP frame buffer only); AHCI = install-vmware.ps1 -Disk sata
  for run in nvme:vmware ahci:std; do
    controller=${run%%:*}
    VGA=${run#*:} python3 tools/vm/vmware-vm.py check "$VM/$NAME.vmdk" vmdk "$controller" "$OUT/$NAME-vmdk-$controller" || fail "the .vmdk does not come up on $controller (log: $OUT/$NAME-vmdk-$controller.log)"
  done
fi
python3 tools/vm/vmware-vm.py zip "$VM" "$OUT/$NAME.zip"
ls -la "$VM" "$OUT/$NAME.zip"
[ -n "${VM_PASSWORD_FILE:-}" ] || echo "account $VM_USER: password in $OUT/$NAME.password (keep it; it is not in the zip)"
(cd "$OUT" && sha256sum "$NAME.zip" | tee "$NAME.zip.sha256")
echo "MAKE-VMWARE: ALLES OK ($OUT/$NAME.zip)"
