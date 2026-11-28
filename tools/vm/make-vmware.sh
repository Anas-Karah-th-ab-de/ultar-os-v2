#!/usr/bin/env bash
# Ultar OS v2 as a VMware Workstation machine, built from this tree:
#   out/vmware/UltarOS-v2/UltarOS-v2.vmx     UEFI, 4 GiB, 4 processors, NVMe disk, e1000e (NAT), xHCI, no sound
#   out/vmware/UltarOS-v2/UltarOS-v2.vmdk    the installed disk (monolithicSparse, qemu-img)
#   out/vmware/UltarOS-v2.zip                both files (install on the PC: tools/vm/install-vmware.ps1)
# What the machine starts is what the tree holds today: loader (A/B) -> Ultar kernel -> ultar-init as PID 1 ->
# logd, bus, settingsd, devd, seat, authd, the app launcher and the shell's services -> ultar-compositor on the
# frame buffer, Arabic, locked at start, with the desktop session on it: ultar-lock, ultar-sysdialog,
# ultar-desktop (bar, sheets, overview), each as its own user; the system apps (Settings, Files, Store, Phone,
# browser) as signed packages installed with the launcher (docs/integration/vmware.md §6).
# One account: VM_USER (ultar) with the password in VM_PASSWORD_FILE, or a random one written to
# out/vmware/UltarOS-v2.password (mode 0600) - never a fixed password in a delivered image.
# Steps:
#   1. build the kernel, the loader and the static programs (BIN=<dir> uses prebuilt programs instead)
#   2. system tree (ext4) and an installation medium as tools/kern-test/ab-test.sh builds it
#   3. the kernel's own installer writes the A/B layout to an empty disk, in QEMU on VMware-like hardware
#      (medium on AHCI, target on NVMe, e1000e, no virtio) - the disk is exactly what the installer creates
#   4. the installed disk is started once (tools/vm/vmware-vm.py check): services, compositor, screenshot
#   5. qemu-img convert -O vmdk, the .vmx, and the .vmdk is started again on NVMe and on AHCI (CHECK=0 skips)
# INSTALLER_IMG=1 builds the public installation image instead (out/vmware/UltarOS-v2-installer.img, raw UEFI GPT
# for a USB stick): the same system tree plus ultar-installer-ui and ultar-firstboot, the medium's marker asks for
# the graphical installer session (session=init, docs/integration/installer-boot.md), NO account and no password
# in the image (the installer's account step creates it, ultar-firstboot writes it at the first start); its check
# boots it in QEMU with an empty NVMe disk until the installer reports its first step (screenshot). It also
# writes the VMware machine out/vmware/UltarOS-v2-installer.zip (medium as SATA disk, empty NVMe target of
# TARGET_GIB, default 32, sparse) to try the installation in VMware Workstation.
# EDITION=phone (with INSTALLER_IMG=1) builds the phone edition on the Ultar kernel for x86_64 phones and tablets
# (and QEMU): the phone's own session (ultar-mobile, ultar-mobile-lock, ultar-mobile-dialog) instead of the
# desktop's, edition marker "phone"; out/vmware/UltarOS-v2-phone-installer.img.
# TREE_ONLY=1 stops after the system tree (out/vmware/<name>-tree, no kernel, no medium, no account): used by
# tools/release/build-a30.sh for the Galaxy A30 on its Linux kernel, with BIN=<aarch64 programs> and TOOL_BIN=<host
# programs> (upkg-keygen, upkg-sign run here).
# Environment: DISK_MIB (4096), LANG_CODE (ar | de | en), KEEP=1 keeps out/vmware/work, SYSTEM_KEY and
# SYSTEM_KEY_PUB (the system signing key, hex files; default: a key made for this build only); for the QEMU runs
# SMP (4), MEM (1024), WAIT, ACCEL (tcg | kvm), see tools/vm/vmware-vm.py.
# Needs: rustc/cargo 1.97 (+ rust-src), gcc, make, qemu-system-x86, qemu-utils (qemu-img), ovmf, mtools,
# dosfstools, e2fsprogs, python3. Nothing is downloaded and nothing outside out/ is written.
# VMware Workstation itself is not started here: QEMU is the test bed (docs/integration/vmware.md).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0
export EDITION=${EDITION:-desktop}
case "$EDITION" in desktop | phone) ;; *) echo "MAKE-VMWARE: FEHLER (EDITION must be desktop or phone)"; exit 1 ;; esac
NAME=UltarOS-v2
[ "$EDITION" = phone ] && NAME=UltarOS-v2-phone
OUT=out/vmware
VM=$OUT/$NAME
W=$OUT/work
LANG_CODE=${LANG_CODE:-ar}
DISK_MIB=${DISK_MIB:-4096}
fail() { echo "MAKE-VMWARE: FEHLER ($*)"; exit 1; }
TREE_ONLY=${TREE_ONLY:-}
if [ -z "$TREE_ONLY" ]; then
  for tool in qemu-system-x86_64 qemu-img mke2fs debugfs mkfs.vfat mcopy mmd python3 cargo; do
    command -v "$tool" >/dev/null || fail "tool missing: $tool"
  done
  for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_VARS_4M.fd; do [ -f "$f" ] || fail "firmware missing: $f"; done
fi
case "$LANG_CODE" in ar | de | en) ;; *) fail "LANG_CODE must be ar, de or en" ;; esac
rm -rf "$W" "$VM" "$OUT/$NAME.zip" "$OUT/$NAME-installer.img" "$OUT/$NAME-tree"
mkdir -p "$W" "$VM"
cleanup() { [ -n "${KEEP:-}" ] || rm -rf "$W"; }
trap cleanup EXIT

# 1) Programs
echo "== 1/5 build"
if [ -z "$TREE_ONLY" ]; then
  env -u CARGO_TARGET_DIR bash scripts/build-kern.sh >"$W/build-kern.log" 2>&1 || { tail -20 "$W/build-kern.log"; fail "kernel build"; }
  env -u CARGO_TARGET_DIR bash scripts/build-loader.sh >"$W/build-loader.log" 2>&1 || { tail -20 "$W/build-loader.log"; fail "loader build"; }
fi
KERNEL=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
LOADER=kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi
# the session: service manager, bus, log, settings, devices, seat, compositor, lock, dialogs, shell
# the edition's own session programs (the phone and the desktop each have their own shell)
if [ "$EDITION" = phone ]; then
  SESSION_PROGRAMS="ultar-mobile-lock ultar-mobile-dialog ultar-mobile"; SESSION_CRATE=ultar-mobile
  SESSION_SERVICES="compositor mobile-lock mobile-dialog mobile"
else
  SESSION_PROGRAMS="ultar-lock ultar-sysdialog ultar-desktop"; SESSION_CRATE=ultar-desktop
  SESSION_SERVICES="compositor lock sysdialog desktop"
fi
PROGRAMS="ultar-init uinit ultar-luks ultar-busd ultar-logd ulog ultar-settingsd usettings ultar-devd ultar-devd-seat udevctl ultar-compositor ultar-authd $SESSION_PROGRAMS"
# the services the shell talks to (wave 8) and the app launcher
PROGRAMS="$PROGRAMS ultar-launcher ulaunch ultar-sessiond ultar-powerd ultar-notifyd ultar-calendard ultar-audiod ultar-mediad ultar-captured ultar-netd ultar-btd ultar-volumed ultar-mesh ultar-agentd ultar-llm-bus ultar-llm-cloud ultar-displayd ultar-accountd ultar-calld"
# the system apps (installed as signed packages through the launcher, step 2)
PROGRAMS="$PROGRAMS ultar-settings ultar-files ultar-store ultar-phone ultar-web ultar-editor ultar-doc ultar-terminal"
INSTALLER_IMG=${INSTALLER_IMG:-}
[ -n "$INSTALLER_IMG" ] && PROGRAMS="$PROGRAMS ultar-installer-ui ultar-firstboot"
SERVICES_UP="logd bus settingsd devd seat authd launcher sessiond powerd notifyd calendard audiod mediad captured netd btd volumed displayd accountd call mesh agentd llmbus llmcloud"
# launcher id : program : manifest arguments (the launcher passes no environment of its own;
# the apps reach the compositor and the apps' bus socket, app.shared, through these). The phone draws its
# own window header: its manifest asks for ui.client-header (granted to system apps at installation).
UI_ARGS="\"--compositor\", \"/run/ultar/ui/compositor.sock\", \"--bus\", \"/run/ultar/appbus/bus.sock\", \"--lang\", \"$LANG_CODE\""
APPS="settings:ultar-settings:$UI_ARGS|files:ultar-files:$UI_ARGS|store:ultar-store:|phone:ultar-phone:$UI_ARGS|editor:ultar-editor:$UI_ARGS|doc:ultar-doc:$UI_ARGS|terminal:ultar-terminal:$UI_ARGS|browser:ultar-web:\"--socket\", \"/run/ultar/ui/compositor.sock\", \"--lang\", \"$LANG_CODE\""
if [ -z "${BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-luks -p ultar-busd -p ultar-logd -p ultar-settingsd -p ultar-devd -p ultar-compositor -p ultar-auth -p "$SESSION_CRATE" \
    -p ultar-launcher -p ultar-sessiond -p ultar-powerd -p ultar-notifyd -p ultar-calendard -p ultar-audiod -p ultar-mediad \
    -p ultar-captured -p ultar-netd -p ultar-btd -p ultar-volumed -p ultar-mesh -p ultar-agentd -p ultar-llm-bus -p ultar-llm-cloud -p ultar-displayd -p ultar-accountd -p ultar-call -p ultar-pkg \
    -p ultar-settings -p ultar-files -p ultar-store -p ultar-phone -p ultar-web -p ultar-editor -p ultar-doc -p ultar-terminal ${INSTALLER_IMG:+-p ultar-installer-ui} --features ultar-store/gui --bins \
    >"$W/build-programs.log" 2>&1 || { tail -20 "$W/build-programs.log"; fail "program build"; }
  BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
TOOL_BIN=${TOOL_BIN:-$BIN}
for p in $PROGRAMS; do [ -x "$BIN/$p" ] || fail "program missing: $BIN/$p"; done
for p in upkg-keygen upkg-sign; do [ -x "$TOOL_BIN/$p" ] || fail "program missing: $TOOL_BIN/$p"; done

# 2) System tree: the shipped configuration of rootfs/etc/ultar, the services this image has programs for
echo "== 2/5 system tree and installation medium"
R=$W/root
mkdir -p "$R"/{bin,sbin,usr/bin,usr/lib/ultar/drivers,usr/lib/ultar/apps,etc/ultar/services,data,tmp,run,root,home,var,proc,sys,dev,media}
cp "$BIN/ultar-init" "$R/sbin/"
for p in $PROGRAMS; do [ "$p" = ultar-init ] || cp "$BIN/$p" "$R/usr/bin/"; done
for f in init.toml ids.toml bus-policy.toml compositor-rights.toml devd-seat.toml; do cp "rootfs/etc/ultar/$f" "$R/etc/ultar/"; done
cp -r rootfs/etc/ultar/trusted-keys "$R/etc/ultar/"
for s in $SERVICES_UP; do cp "rootfs/etc/ultar/services/$s.toml" "$R/etc/ultar/services/"; done
# The launcher as shipped, but accepting what the Ultar kernel does not have yet: network
# namespaces (an app without the permission `network` gets no network of its own on Linux;
# here it has none anyway: no app has a bus right to the network services).
sed 's/^accept_missing = \[\]$/accept_missing = ["net"]/' rootfs/etc/ultar/launcher.toml >"$R/etc/ultar/launcher.toml"
grep -q '^accept_missing = \["net"\]$' "$R/etc/ultar/launcher.toml" || fail "launcher.toml: accept_missing"
# The system apps: each a package signed with the system key (profile app.system), installed
# over the bus by the administrator's tool (ulaunch, socket system.admin) at every start
# (same signer and version: the launcher keeps the app's user id and data). The private key
# is SYSTEM_KEY (a file with the hex key) or made for this build only and never leaves $W.
if [ -n "${SYSTEM_KEY:-}" ]; then
  KEY=$SYSTEM_KEY
  PUB=${SYSTEM_KEY_PUB:?SYSTEM_KEY_PUB: the public half of SYSTEM_KEY}
  cp "$PUB" "$W/system.pub"
else
  KEY=$W/system.key
  (umask 077; "$TOOL_BIN/upkg-keygen" >"$KEY" 2>"$W/keygen.err") || fail "upkg-keygen"
  tail -1 "$W/keygen.err" >"$W/system.pub"
fi
mkdir -p "$R/etc/ultar/trusted-keys/system" "$W/apps/payload"
cp "$W/system.pub" "$R/etc/ultar/trusted-keys/system/km-code-craft.ed25519"
printf 'Ultar OS system app (program in /usr/bin)\n' >"$W/apps/payload/README"
# the apps carry the workspace version (one version for the whole system)
APP_VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$APP_VERSION" ] || fail "workspace version"
IFS='|' read -r -a app_list <<<"$APPS"
for entry in "${app_list[@]}"; do
  id=${entry%%:*}; rest=${entry#*:}; prog=${rest%%:*}; args=${rest#*:}
  # what the app declares: the [permissions] table of its own manifest (apps/<crate>/ultar-app.toml,
  # one source with the release packages) - bus access (the launcher binds the apps' bus socket only
  # then and names the app to the broker as app.<id>), file areas, network, ui rights. An app without
  # a manifest (the browser) declares nothing and gets no bus.
  perms=""
  if [ -f "apps/$prog/ultar-app.toml" ]; then
    perms=$(awk '/^\[permissions\]/{p=1;next} /^\[/{p=0} p' "apps/$prog/ultar-app.toml")
  fi
  # What it opens: the [open] table of its own manifest (Files: trash/file/files, the editor:
  # files); without one nothing is handed over. Settings opens on a page (quick settings:
  # "Wi-Fi settings", the wired tile …): the launcher accepts settings://<page> and hands the
  # page over as `--area <page>`.
  open=""
  if [ -f "apps/$prog/ultar-app.toml" ] && grep -q '^\[open\]' "apps/$prog/ultar-app.toml"; then
    open=$'[open]\n'$(awk '/^\[open\]/{p=1;next} /^\[/{p=0} p' "apps/$prog/ultar-app.toml")$'\n'
  fi
  [ "$id" = settings ] && open=$'[open]\nschemes = ["settings"]\npage_arg = "--area"\n'
  cat >"$W/apps/$id.toml" <<MANIFEST
id = "$id"
version = "$APP_VERSION"
publisher = "km-code-craft"
kind = "native"
name = "app.$id.name"
exec = "/usr/bin/$prog"
args = [$args]

$open
[permissions]
$perms

[limits]
memory_mb = 384
MANIFEST
  "$TOOL_BIN/upkg-sign" "$W/apps/$id.toml" "$W/apps/payload" "$R/usr/lib/ultar/apps/$id.uap" "$KEY" >/dev/null \
    || fail "package $id"
  cat >"$R/etc/ultar/services/app-$id.toml" <<SERVICE
# Installs (or confirms) the system app $id with the launcher at every start: a registration step
# that ends at once (not the app itself; the app starts when the user opens it).
description = "registers the system app $id with the launcher (signed package; the app starts when opened)"
exec = "/usr/bin/ulaunch"
args = ["--bus", "/run/ultar/bus/system.sock", "install", "/usr/lib/ultar/apps/$id.uap"]
oneshot = true
after = ["launcher"]
# root with file_admin: the administrator's bus socket is root-only (0600, owner busd)
rights = ["file_admin"]

[cgroup]
"memory.max" = "32M"
"pids.max" = "8"
SERVICE
done
# The shipped session services are not started by the ordinary system (`manual = true`, or
# `[profiles] system = "manual"`) because the legacy ultar-shell would drive the same display; this image
# has no ultar-shell, so they start with the system. Nothing else of the files changes.
for s in $SESSION_SERVICES; do
  f="rootfs/etc/ultar/services/$s.toml"
  if grep -q '^manual = true$' "$f"; then
    grep -v '^manual = true$' "$f" >"$R/etc/ultar/services/$s.toml"
  elif grep -q '^system = "manual"$' "$f"; then
    sed 's/^system = "manual"$/system = "start"/' "$f" >"$R/etc/ultar/services/$s.toml"
  else
    fail "$s.toml: neither 'manual = true' nor 'system = \"manual\"'"
  fi
done
if [ -n "$INSTALLER_IMG" ]; then
  # the installer (profile installer only) and the first start (profile system); the authentication
  # and the settings service start after the first start has written the account and the settings
  # the installer was given
  for s in installer firstboot; do cp "rootfs/etc/ultar/services/$s.toml" "$R/etc/ultar/services/"; done
  for s in authd settingsd; do
    sed -i 's/^after = \["bus"\]$/after = ["bus", "firstboot"]/' "$R/etc/ultar/services/$s.toml"
    grep -q '^after = \["bus", "firstboot"\]$' "$R/etc/ultar/services/$s.toml" || fail "$s.toml: after"
  done
fi
printf '%s\n' "$EDITION" >"$R/etc/ultar/edition"
printf 'init\n' >"$R/etc/ultar/session"
printf '%s\n' "$LANG_CODE" >"$R/etc/ultar/sprache"
printf 'ultar\n' >"$R/etc/hostname"
ULTAR_VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
printf 'NAME="Ultar OS"\nPRETTY_NAME="Ultar OS %s"\nID=ultar\nVERSION_ID="%s"\nHOME_URL="https://code-craft.tech"\n' "$ULTAR_VERSION" "$ULTAR_VERSION" >"$R/etc/os-release"
mkdir -p "$R/usr/lib" && cp "$R/etc/os-release" "$R/usr/lib/os-release"
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
if [ -n "$TREE_ONLY" ]; then
  mv "$R" "$OUT/$NAME-tree"
  echo "MAKE-VMWARE: ALLES OK ($OUT/$NAME-tree)"
  exit 0
fi
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
# the user's file areas (ultar-launcher areas.rs makes the rest; it sets group and mode at start) with a
# first file in the home folder, so the Files app has something to list
mkdir -p "$W/data-root/ultar/areas/home"
printf 'Ultar OS\n' >"$W/data-root/ultar/areas/home/README.txt"
# the account of the lock screen and the system dialogs (ultar-authd passwd reads the password on stdin)
VM_USER=${VM_USER:-ultar}
PWFILE=${VM_PASSWORD_FILE:-}
if [ -n "$INSTALLER_IMG" ]; then
  : # no account: the installer asks for it
elif [ -z "$PWFILE" ]; then
  PWFILE=$OUT/$NAME.password; (umask 077; head -c 12 /dev/urandom | base64 | tr -d '/+=' >"$PWFILE")
elif [ "$(realpath "$PWFILE")" != "$(realpath -m "$OUT/$NAME.password")" ]; then
  # the image's password always lies beside the image (0600, never in the zip, kept on rebuilds)
  (umask 077; cp "$PWFILE" "$OUT/$NAME.password")
fi
if [ -z "$INSTALLER_IMG" ]; then
  pw=$(head -1 "$PWFILE")
  printf '%s\n%s\n' "$pw" "$pw" | "$BIN/ultar-authd" --dir "$W/data-root/ultar/auth" passwd "$VM_USER" --lang "$LANG_CODE" \
    >/dev/null 2>&1 || fail "account $VM_USER"
  unset pw
fi
mke2fs -q -t ext4 -L ultar-data -d "$W/data-root" "$W/data.ext4" 512M
root_owned "$W/data.ext4" "$W/data-root"
# the authentication service (813) owns its directory and files
auth_cmds=$(mktemp)
(cd "$W/data-root" && find ultar/auth | sed 's|^|/|') | while read -r p; do printf 'sif %s uid 813\nsif %s gid 813\n' "$p" "$p"; done >"$auth_cmds"
debugfs -w -f "$auth_cmds" "$W/data.ext4" >/dev/null 2>&1; rm -f "$auth_cmds"
# installation medium (GPT): installer, start partition with the loader, kernel, system, data
truncate -s 16M "$W/install.fat"; mkfs.vfat -n ULTAR-INST "$W/install.fat" >/dev/null
mmd -i "$W/install.fat" ::/EFI ::/EFI/BOOT; mcopy -i "$W/install.fat" "$KERNEL" ::/EFI/BOOT/BOOTX64.EFI
if [ -n "$INSTALLER_IMG" ]; then
  printf 'Ultar OS installation medium\nsession=init\n' >"$W/marker"
else
  printf 'Ultar OS installation medium\n' >"$W/marker"
fi; mcopy -i "$W/install.fat" "$W/marker" ::/ultar-installer
truncate -s 64M "$W/esp.fat"; mkfs.vfat -F 32 -n ULTAR-EFI "$W/esp.fat" >/dev/null
mmd -i "$W/esp.fat" ::/EFI ::/EFI/BOOT ::/ultar; mcopy -i "$W/esp.fat" "$LOADER" ::/EFI/BOOT/BOOTX64.EFI
printf 'Ultar OS\n' >"$W/inst"; mcopy -i "$W/esp.fat" "$W/inst" ::/ultar/installiert
printf '%s\n' "$LANG_CODE" >"$W/lang"; mcopy -i "$W/esp.fat" "$W/lang" ::/ultar/sprache
python3 tools/kern-test/gptimg.py "$W/medium.img" "ULTAR-INSTALL:esp:$W/install.fat" "ULTAR-EFI:esp:$W/esp.fat" \
  "ultar-kernel:raw:$KERNEL" "ultar-compat:linux:$W/system.ext4" "ultar-data:linux:$W/data.ext4" >/dev/null
rm -rf "$R" "$W/data-root" "$W"/*.fat "$W"/*.ext4
if [ -n "$INSTALLER_IMG" ]; then
  echo "== installer image: start in QEMU with an empty NVMe disk"
  mv "$W/medium.img" "$OUT/$NAME-installer.img"
  if [ "${CHECK:-1}" != 0 ]; then
    python3 tools/vm/vmware-vm.py installer "$OUT/$NAME-installer.img" "$OUT/$NAME-installer" || fail "the installer image does not reach the installer"
  fi
  (cd "$OUT" && sha256sum "$NAME-installer.img" | tee "$NAME-installer.img.sha256")
  # VMware machine to try the installation: the medium as SATA disk (VMware's AHCI, disks only), an empty
  # NVMe disk as the target (TARGET_GIB, sparse), no account; started from the medium because the target has
  # nothing bootable yet. After "Restart" at the end of the installation remove the SATA disk (README).
  IVM=$OUT/$NAME-installer
  rm -f "$OUT/$NAME-installer-target.raw"
  rm -rf "$IVM" "$OUT/$NAME-installer.zip"; mkdir -p "$IVM"
  qemu-img convert -f raw -O vmdk -o subformat=monolithicSparse,adapter_type=lsilogic \
    "$OUT/$NAME-installer.img" "$IVM/$NAME-medium.vmdk"
  qemu-img create -q -f vmdk -o subformat=monolithicSparse,adapter_type=lsilogic "$IVM/$NAME-target.vmdk" "${TARGET_GIB:-32}G"
  sed -e "s/@NAME@/$NAME-installer/g" \
      -e "s/^nvme0:0.fileName = .*/nvme0:0.fileName = \"$NAME-target.vmdk\"/" \
      -e 's/^sata0.present = "FALSE"/sata0.present = "TRUE"/' \
      -e 's/^displayName = .*/displayName = "Ultar OS v2 Installation – KM Code Craft"/' \
      tools/vm/UltarOS-v2.vmx.in >"$IVM/$NAME-installer.vmx"
  printf 'sata0:0.present = "TRUE"\nsata0:0.fileName = "%s"\n' "$NAME-medium.vmdk" >>"$IVM/$NAME-installer.vmx"
  grep -q "^nvme0:0.fileName = \"$NAME-target.vmdk\"$" "$IVM/$NAME-installer.vmx" || fail "installer .vmx: target disk"
  grep -q '^sata0.present = "TRUE"$' "$IVM/$NAME-installer.vmx" || fail "installer .vmx: medium disk"
  cat >"$IVM/README.txt" <<'TXT'
Ultar OS v2 - installation in VMware Workstation (KM Code Craft, https://code-craft.tech)

1. Open the .vmx (File > Open) or run tools/vm/install-vmware.ps1 -Installer -Zip <this zip>.
2. The machine starts the graphical installer from the medium (SATA disk). The target is the empty
   NVMe disk. Go through the steps; the account is created in the installer.
3. On the last step press "Restart". Then power the machine off, remove the SATA disk
   ("...-medium.vmdk") under VM > Settings and start it again: Ultar OS starts from the NVMe disk.
   The kernel log of every start is in serial.log beside the .vmx.
TXT
  python3 tools/vm/vmware-vm.py zip "$IVM" "$OUT/$NAME-installer.zip"
  (cd "$OUT" && sha256sum "$NAME-installer.zip" | tee "$NAME-installer.zip.sha256")
  echo "MAKE-VMWARE: ALLES OK ($OUT/$NAME-installer.img, $OUT/$NAME-installer.zip)"
  exit 0
fi

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
echo "account $VM_USER: password in $OUT/$NAME.password (keep it; it is not in the zip)"
(cd "$OUT" && sha256sum "$NAME.zip" | tee "$NAME.zip.sha256")
echo "MAKE-VMWARE: ALLES OK ($OUT/$NAME.zip)"
