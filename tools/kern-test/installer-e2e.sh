#!/usr/bin/env bash
# The graphical installation from A to Z (tools/kern-test/installer-e2e.py): an installation medium whose
# marker asks for the installer session (session=init: ultar-init with the profile installer, the compositor
# and ultar-installer-ui, no desktop), an empty second disk; the installer is driven by keys through the
# QEMU monitor (language -> … -> disk -> account -> summary -> install), then the installed disk starts alone
# and its first start creates the account the installer was given (ultar-firstboot).
# Verdict line: "INSTALLER: ALLES OK".
# BIN=<dir> with static ultar-init, ultar-busd, ultar-logd, ultar-devd, ultar-devd-seat, ultar-compositor,
# ultar-installer-ui and ultar-firstboot (else they are built here); KEEP_LOG=<dir> keeps the serial logs.
# Needs what make-vmware.sh needs (qemu-system-x86, ovmf, mtools, dosfstools, e2fsprogs, python3).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
fail() { echo "INSTALLER: FEHLER ($*)"; exit 1; }
env -u CARGO_TARGET_DIR bash scripts/build-kern.sh >"$W/build-kern.log" 2>&1 || { tail -20 "$W/build-kern.log"; fail "kernel build"; }
env -u CARGO_TARGET_DIR bash scripts/build-loader.sh >"$W/build-loader.log" 2>&1 || { tail -20 "$W/build-loader.log"; fail "loader build"; }
KERNEL=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
LOADER=kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi
PROGRAMS="ultar-init ultar-busd ultar-logd ultar-devd ultar-devd-seat ultar-compositor ultar-installer-ui ultar-firstboot"
if [ -z "${BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-devd -p ultar-compositor -p ultar-installer-ui --bins \
    >"$W/build-programs.log" 2>&1 || { tail -20 "$W/build-programs.log"; fail "program build"; }
  BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
for p in $PROGRAMS; do [ -x "$BIN/$p" ] || fail "program missing: $BIN/$p"; done

# The system tree: the same tree is the medium's live system (profile installer) and, copied by the
# installer, the installed system (profile system).
R=$W/root
mkdir -p "$R"/{bin,sbin,usr/bin,etc/ultar/services,data,tmp,run,root,home,var,proc,sys,dev,media}
cp "$BIN/ultar-init" "$R/sbin/"
for p in $PROGRAMS; do [ "$p" = ultar-init ] || { cp "$BIN/$p" "$R/usr/bin/"; strip "$R/usr/bin/$p" 2>/dev/null || true; }; done
strip "$R/sbin/ultar-init" 2>/dev/null || true
for f in init.toml ids.toml bus-policy.toml compositor-rights.toml devd-seat.toml; do cp "rootfs/etc/ultar/$f" "$R/etc/ultar/"; done
cp -r rootfs/etc/ultar/trusted-keys "$R/etc/ultar/"
for s in logd bus devd seat compositor installer firstboot; do cp "rootfs/etc/ultar/services/$s.toml" "$R/etc/ultar/services/"; done
# test only: after the first start, list the accounts on the console
cat >"$R/etc/ultar/services/accountcheck.toml" <<'EOF'
exec = "/usr/bin/ultar-firstboot"
args = ["--list", "--auth-dir", "/data/ultar/auth"]
oneshot = true
console = true
after = ["firstboot"]
# Identity: root with the right file_admin, like firstboot (the account file is authd's, mode 0600, in a
# directory init made for firstboot; root without a right may not read it)
rights = ["file_admin"]
EOF
printf 'desktop\n' >"$R/etc/ultar/edition"
printf 'init\n' >"$R/etc/ultar/session"
printf 'en\n' >"$R/etc/ultar/sprache"
printf 'ultar\n' >"$R/etc/hostname"
cp rootfs/etc/os-release "$R/etc/os-release"
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
root_owned() { # image source-dir
  local cmds; cmds=$(mktemp)
  { printf 'sif / uid 0\nsif / gid 0\n'
    (cd "$2" && find . -mindepth 1 | sed 's|^\.||') | while read -r p; do printf 'sif %s uid 0\nsif %s gid 0\n' "$p" "$p"; done
  } >"$cmds"
  debugfs -w -f "$cmds" "$1" >/dev/null 2>&1
  rm -f "$cmds"
}
SYS_MIB=$(($(du -sm "$R" | cut -f1) * 13 / 10 + 16))
mke2fs -q -t ext4 -L ultar-compat -d "$R" "$W/system.ext4" "${SYS_MIB}M"
root_owned "$W/system.ext4" "$R"
mkdir -p "$W/data-root/ultar"
mke2fs -q -t ext4 -L ultar-data -d "$W/data-root" "$W/data.ext4" 64M
root_owned "$W/data.ext4" "$W/data-root"
# installation medium: the marker asks for the graphical installer session
truncate -s 16M "$W/install.fat"; mkfs.vfat -n ULTAR-INST "$W/install.fat" >/dev/null
mmd -i "$W/install.fat" ::/EFI ::/EFI/BOOT; mcopy -i "$W/install.fat" "$KERNEL" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS installation medium\nsession=init\n' >"$W/marker"; mcopy -i "$W/install.fat" "$W/marker" ::/ultar-installer
truncate -s 64M "$W/esp.fat"; mkfs.vfat -F 32 -n ULTAR-EFI "$W/esp.fat" >/dev/null
mmd -i "$W/esp.fat" ::/EFI ::/EFI/BOOT ::/ultar; mcopy -i "$W/esp.fat" "$LOADER" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS\n' >"$W/inst"; mcopy -i "$W/esp.fat" "$W/inst" ::/ultar/installiert
printf 'en\n' >"$W/lang"; mcopy -i "$W/esp.fat" "$W/lang" ::/ultar/sprache
python3 tools/kern-test/gptimg.py "$W/medium.img" "ULTAR-INSTALL:esp:$W/install.fat" "ULTAR-EFI:esp:$W/esp.fat" \
  "ultar-kernel:raw:$KERNEL" "ultar-compat:linux:$W/system.ext4" "ultar-data:linux:$W/data.ext4" >/dev/null
rm -rf "$R" "$W/data-root" "$W"/*.fat "$W"/*.ext4
truncate -s "${DISK_MIB:-2048}M" "$W/target.raw"
PREFIX="${KEEP_LOG:-$W}/installer-e2e"
if out=$(SMP="${SMP:-2}" timeout 3600 python3 tools/kern-test/installer-e2e.py "$W/medium.img" "$W/target.raw" "$PREFIX"); then
  echo "$out"; echo "INSTALLER: ALLES OK"
else
  echo "$out"; [ -f "$PREFIX-install.log" ] && grep -aE "\[installer-ui\]|\[installer\]|ultar-init.*(refused|exit|failed)|PANIK" "$PREFIX-install.log" | tail -25
  echo "INSTALLER: FEHLER"; exit 1
fi
