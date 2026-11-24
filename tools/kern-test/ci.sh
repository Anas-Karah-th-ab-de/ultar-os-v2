#!/usr/bin/env bash
# Kernel gate: build the Ultar kernel, boot it in QEMU and prove that
#   1. every device has a driver, 2. the ring-3 test program passes,
#   3. the system bus (ultar-bus) runs on the Ultar kernel,
#   4. the escape tests of the process isolation pass (zelltest: mount/pid namespaces, rights),
#   5. the same for symbolic links on an ext4 disk (zell-ext4.sh; skipped without mke2fs),
#   5a. vfork release at exec and flock (spawntest); durable ext4 writes of large, fragmented files
#       through a hard stop, honest ENOSPC/EIO from fsync and close (ext4-durable.sh),
#   5b. identities, file modes and owners, Unix socket nodes and peer credentials, fsync (rechtetest),
#       and modes/owners/data that reach a writable ext4 disk (rechte-ext4.sh; skipped without e2fsprogs),
#   5c. the device interfaces for ultar-devd (geraetetest: device list, driver register, bind/unbind/rebind of
#       an e1000, driver ABI, uevents, health, rights) and USB hot-plug events (geraete-hotplug.py:
#       a keyboard added and removed through the QEMU monitor), and ultar-devd binding a driver
#       package through them (devd-qemu.sh; DEVD=/UDRV= for prebuilt binaries),
#   5d. wake-ups (weckertest): posix_spawn/vfork release the parent at execve, shutdown/close wake blocked
#       Unix-socket readers, writers and accept, signalfd in threaded processes,
#   5e. what a compositor needs (fenstertest): memfd + seals, shared mappings across fork and processes,
#       SCM_RIGHTS, MSG_DONTWAIT, POLLOUT wake-ups, poll timeouts below a second, mremap, fb0 mmap + vblank,
#       owner/group/mode of device nodes, evdev capability ioctls, /sys/class/input; a USB keyboard that
#       comes and goes gets its own event node (geraete-hotplug.py),
#   5e2. power off and restart from a process end the machine (ausschalten.sh, AUSSCHALTEN: ALLES OK),
#   5f. the same programs on VMware-like hardware without virtio (vmware-like.sh),
#   5g. the wired network with e1000, virtio-net, e1000e and vmxnet3: /sys/class/net, DHCP lease, DNS, TCP
#       to the host, renew, static address, cable out and in (netz-e2e.sh, NETZ: ALLES OK), and the VMware machine's
#       disk driven only through emulated USB/PS/2 input: locked at start, unlock, start menu, five apps open
#       (tools/vm/vmware-input-e2e.sh, VMWARE-INPUT-E2E: ALLES OK; VMWARE_INPUT=0 skips it),
#   6. resource groups and memory shortage (grenztest: limits, dynamic protection, the attacks of the
#      security review – nothing may stop the machine), once with 512 MiB and once with 2 GiB.
#   7. the service manager starts the system services as their own users (init-e2e.sh, INIT: ALLES OK),
#   7d. the desktop session starts locked and unlocks with a typed password (desktop-e2e.sh,
#       DESKTOP: ALLES OK; the same on VMware-like hardware in vmware-like.sh),
#   7a. the app launcher runs apps as their own users in their cells, a malicious app stays in its cell
#       (launcher-e2e.sh, LAUNCHER: ALLES OK),
#   7b. the display and account services under the service manager (svc-e2e.sh, SVC: ALLES OK),
#   7c. the system services behind the desktop shell (dienste-e2e.sh, DIENSTE: ALLES OK),
#   7e. the phone: ultar-calld with the test-double modem and the phone app (telefon-e2e.sh, TELEFON: ALLES OK),
#   7f. installer mode: whole-disk nodes for the installer only (installer-disk.sh, INSTALLER-DISK: ALLES OK) and
#       the graphical installation from A to Z with the first start of the installed disk (installer-e2e.sh,
#       INSTALLER: ALLES OK; opt-in with INSTALLER_E2E=1, see docs/integration/installer-boot.md),
#   7g. Bluetooth: a USB controller (usb-redir + bt-usb.py) through /dev/ultar/hci0, ultar-btd sets it up as its own
#       user, its keyboards through /dev/ultar/uinput (bt-e2e.sh, BT: ALLES OK; virtual input devices alone:
#       bttest uinput in the first boot, BTTEST-UINPUT: ALLES OK),
#   8. the A/B start chain with the real updater (ab-test.sh; AB_TEST=0 skips it).
#   0. before the boots: regressions of the owner's VMware reports – Files' list view and the quick settings by
#      real clicks through the compositor, the launcher's page argument (ui-regress.sh, UI-REGRESS: ALLES OK).
# Needs: rustc-1.97 cargo-1.97 rust-1.97-src qemu-system-x86 ovmf mtools dosfstools gcc e2fsprogs
# BUSCK=<file>: use this prebuilt ultar-bus selfcheck instead of building it from the workspace.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG=1 bash scripts/build-kern.sh >/dev/null
# pure display-driver logic (Intel: register layout, takeover, DP/AUX/EDID, link training, C20 PLL, mode set)
if ! idout=$(cargo test -q --manifest-path drivers/kernel/intel-display/Cargo.toml 2>&1); then
  echo "FAIL  intel-display unit tests"; echo "$idout" | tail -30; exit 1
fi
echo "ok    intel-display unit tests ($(echo "$idout" | grep -oE '[0-9]+ passed' | head -1))"
# regressions of the owner's VMware reports: Files list view, quick settings by real clicks, launcher pages
bash tools/kern-test/ui-regress.sh
if [ -z "${BUSCK:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p ultar-bus --example selfcheck --target x86_64-unknown-linux-gnu
  BUSCK="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/examples/selfcheck"
fi
mcopy -o -i out/ultar-kern-esp.img "$BUSCK" ::/bin/busck
LOG="${LOG:-$(mktemp)}"
# a second network card (e1000) is the device the bind/unbind cycle of geraetetest works on
QARGS="${QARGS:-} -netdev user,id=n2 -device e1000,netdev=n2" SMP="${SMP:-2}" timeout 300 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "geraete" "/bin/kerntest" "/bin/busck" "/bin/zelltest" "/bin/rechtetest" "/bin/geraetetest" "/bin/weckertest" "/bin/spawntest" "/bin/fenstertest" "/bin/bttest uinput" > "$LOG" 2>&1 || true
fail=0
for expect in "alle Geräte haben einen Treiber" "Ergebnis: ALLES OK" "BUS SELFCHECK OK" "ZELLTEST: ALLES OK" "RECHTETEST: ALLES OK" "GERAETETEST: ALLES OK" "WECKERTEST: ALLES OK" "SPAWNTEST: ALLES OK" "FENSTERTEST: ALLES OK" "BTTEST-UINPUT: ALLES OK"; do
  if grep -q "$expect" "$LOG"; then echo "ok    $expect"; else echo "FAIL  $expect"; fail=1; fi
done
if grep -qiE "kernel panic|PANIK" "$LOG"; then echo "FAIL  kernel panic"; fail=1; fi
grep -o "\[kern\] bereit nach [0-9]* ms" "$LOG" || true
[ "$fail" = 0 ] || { tail -40 "$LOG"; exit 1; }
# Many devices: 57 extra PCI functions (seven multifunction slots, one behind a PCIe root port) and four USB
# devices make the generated tables larger than a page; geraetetest reads them in pieces. With this device set
# QEMU 8.2 with one host thread per vCPU (MTTCG) itself crashed (SIGSEGV, exit 139) in about one boot of three at
# SMP=4 while the guest was in its PCI scan, so this boot runs the vCPUs on one host thread (the guest still
# has $SMP processors).
MANY="-device pcie-root-port,id=rp1,chassis=1,slot=1 -device virtio-rng-pci,bus=rp1 -device qemu-xhci,id=xh -netdev user,id=n2 -device e1000,netdev=n2"
for slot in 10 11 12 13 14 15 16; do for fn in 0 1 2 3 4 5 6 7; do
  mf=""; [ "$fn" != 0 ] || mf=",multifunction=on"
  MANY="$MANY -device virtio-rng-pci,addr=0x$slot.$fn$mf"; done; done
for d in usb-kbd usb-mouse usb-tablet usb-kbd; do MANY="$MANY -device $d,bus=xh.0"; done
MLOG="${MLOG:-$(mktemp)}"
QARGS="$MANY -accel tcg,thread=single" SMP="${SMP:-2}" timeout 400 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "geraete" "/bin/geraetetest" > "$MLOG" 2>&1 || true
big=$(grep -aoE "Tabellen: devices [0-9]+ Bytes" "$MLOG" | grep -oE "[0-9]+" | head -1)
if grep -aq "GERAETETEST: ALLES OK" "$MLOG" && grep -aq "alle Geräte haben einen Treiber" "$MLOG" && [ "${big:-0}" -gt 4096 ]; then
  echo "ok    GERAETETEST: ALLES OK with many devices (device table $big bytes, read in pieces)"
else
  echo "FAIL  GERAETETEST with many devices (device table ${big:-0} bytes)"; grep -aE "FEHLER|GERAETETEST" "$MLOG" | tail -20; exit 1
fi
# Power off and restart from a process end the machine (ACPI first, UEFI ResetSystem as fallback), no exception.
if bash tools/kern-test/ausschalten.sh > "${AUS_LOG:-/tmp/ausschalten.log}" 2>&1; then
  echo "ok    AUSSCHALTEN: ALLES OK"
else
  echo "FAIL  AUSSCHALTEN"; tail -30 "${AUS_LOG:-/tmp/ausschalten.log}"; exit 1
fi
# The same image on VMware-like hardware without any virtio device (AHCI/NVMe, e1000/e1000e, xHCI or PS/2,
# VMware SVGA II): tools/kern-test/vmware-like.sh. VMWARE_LIKE=0 skips it.
if [ "${VMWARE_LIKE:-1}" = 0 ]; then
  echo "skip  VMWARE-LIKE (VMWARE_LIKE=0)"
else
  IMG=out/ultar-kern-esp.img SMP="${SMP:-2}" bash tools/kern-test/vmware-like.sh
fi
# The wired network on every card the kernel drives in VMs (netz-e2e.sh). NETZ=0 skips it.
if [ "${NETZ:-1}" = 0 ]; then
  echo "skip  NETZ (NETZ=0)"
else
  NLOG="${NETZ_E2E_LOG:-$(mktemp)}"
  if IMG=out/ultar-kern-esp.img SMP="${SMP:-2}" bash tools/kern-test/netz-e2e.sh > "$NLOG" 2>&1 && grep -q "^NETZ: ALLES OK" "$NLOG"; then
    grep -E "^(ok|skip)" "$NLOG"; echo "ok    NETZ: ALLES OK"
  else
    echo "FAIL  NETZ"; tail -40 "$NLOG"; exit 1
  fi
fi
HLOG="${HLOG:-$(mktemp)}"
SMP="${SMP:-2}" WAIT=180 timeout 360 python3 tools/kern-test/geraete-hotplug.py out/ultar-kern-esp.img > "$HLOG" 2>&1 || true
if grep -q "GERAETETEST-USB: ALLES OK" "$HLOG" && ! grep -qiE "kernel panic|PANIK" "$HLOG"; then
  echo "ok    GERAETETEST-USB: ALLES OK"
else
  echo "FAIL  GERAETETEST-USB"; tail -30 "$HLOG"; exit 1
fi
# ultar-devd on the kernel: reads the device tree and binds a driver through it (DEVD=/UDRV= prebuilt)
bash tools/kern-test/devd-qemu.sh
if command -v mke2fs >/dev/null; then
  bash tools/kern-test/zell-ext4.sh
else
  echo "skip  ZELLTEST-EXT4 (mke2fs missing)"
fi
if command -v mke2fs >/dev/null && command -v debugfs >/dev/null; then
  bash tools/kern-test/rechte-ext4.sh
else
  echo "skip  RECHTETEST-EXT4 (e2fsprogs missing)"
fi
# durable ext4 writes (extent trees, honest fsync/close) through a hard stop, 1 KiB and 4 KiB blocks
if command -v mke2fs >/dev/null && command -v debugfs >/dev/null && command -v e2fsck >/dev/null; then
  EXT4_BLOCK=1024 bash tools/kern-test/ext4-durable.sh
  EXT4_BLOCK=4096 bash tools/kern-test/ext4-durable.sh
else
  echo "skip  EXT4TEST (e2fsprogs missing)"
fi
# Resource groups: the test brings the machine to its memory limits on purpose, so it gets its own boots
# (GRENZ_MEM: sizes in MiB; GRENZ_LOG: keep the logs as <GRENZ_LOG>-<size>.log).
for mem in ${GRENZ_MEM:-512 2048}; do
  GLOG="${GRENZ_LOG:+$GRENZ_LOG-$mem.log}"; GLOG="${GLOG:-$(mktemp)}"
  SMP="${SMP:-2}" MEM=$mem WAIT=1500 timeout 1800 python3 tools/kern-test/qtest.py out/ultar-kern-esp.img "/bin/grenztest" > "$GLOG" 2>&1 || true
  if grep -q "GRENZTEST: ALLES OK" "$GLOG" && ! grep -qiE "kernel panic|PANIK" "$GLOG"; then
    echo "ok    GRENZTEST: ALLES OK ($mem MiB, $(grep -c '^ok ' "$GLOG") checks)"
  else
    echo "FAIL  GRENZTEST ($mem MiB)"; grep -E "FEHLER|PANIK|GRENZTEST" "$GLOG" | tail -40; tail -15 "$GLOG"; exit 1
  fi
done
# The service manager with the shipped system services, each as its own user.
IMG=out/ultar-kern-esp.img bash tools/kern-test/init-e2e.sh
# The app launcher: apps in cells with their own users, groups and permissions; a malicious app.
IMG=out/ultar-kern-esp.img bash tools/kern-test/launcher-e2e.sh
# The desktop session: compositor, authd, lock screen, system dialogs and the shell as their own users;
# locked with Super+L and unlocked with the test password through the virtual keyboard (DESKTOP: ALLES OK).
IMG=out/ultar-kern-esp.img bash tools/kern-test/desktop-e2e.sh
# The display and account services: keep-or-revert across service crashes, accounts as their own ids.
IMG=out/ultar-kern-esp.img bash tools/kern-test/svc-e2e.sh
# The system services behind the shell: power, session, notifications, calendar, audio; power off via init.
IMG=out/ultar-kern-esp.img bash tools/kern-test/dienste-e2e.sh
# The phone: the real call daemon with the test-double modem and the phone app (TELEFON: ALLES OK).
IMG=out/ultar-kern-esp.img bash tools/kern-test/telefon-e2e.sh
# Installer mode: whole-disk nodes only on an installation medium, only for the installer's identity, the
# boot disk never a target (installer-disk.sh, INSTALLER-DISK: ALLES OK).
IMG=out/ultar-kern-esp.img SMP="${SMP:-2}" bash tools/kern-test/installer-disk.sh | tail -3
# The graphical installation from A to Z on VMware-like hardware: medium + empty disk, the installer driven
# by keys, then the installed disk alone with the account it was given (installer-e2e.sh). Opt-in
# (INSTALLER_E2E=1) until the kernel's keyboard path stops losing typed characters under QEMU (TCG): the
# account step cannot be typed reliably yet (docs/integration/installer-boot.md, open point 1).
if [ "${INSTALLER_E2E:-0}" = 0 ]; then
  echo "skip  INSTALLER (INSTALLER_E2E=1 runs it; open point in docs/integration/installer-boot.md)"
else
  ILOG="${INSTALLER_LOG:-$(mktemp)}"
  if SMP="${SMP:-2}" bash tools/kern-test/installer-e2e.sh > "$ILOG" 2>&1 && grep -q "^INSTALLER: ALLES OK" "$ILOG"; then
    echo "ok    INSTALLER: ALLES OK ($(grep -o 'installed through[^)]*' "$ILOG"))"
  else
    echo "FAIL  INSTALLER"; tail -30 "$ILOG"; exit 1
  fi
fi
# Bluetooth: USB controller node, ultar-btd on it as its own user, virtual input devices.
IMG=out/ultar-kern-esp.img bash tools/kern-test/bt-e2e.sh
# A/B start chain (installer, loader, slot table, raw partitions, the real updater, fallback, power loss):
# its own installation and about 25 starts. AB_TEST=0 skips it.
if [ "${AB_TEST:-1}" = 0 ]; then
  echo "skip  AB-TEST (AB_TEST=0)"
elif command -v mke2fs >/dev/null && command -v mkfs.vfat >/dev/null && command -v mcopy >/dev/null; then
  ABLOG="${AB_LOG:-$(mktemp)}"
  SMP="${SMP:-2}" timeout 3600 bash tools/kern-test/ab-test.sh > "$ABLOG" 2>&1 || true
  if grep -q "^AB-TEST: ALLES OK" "$ABLOG"; then
    echo "ok    $(grep '^AB-TEST:' "$ABLOG")"
  else
    echo "FAIL  AB-TEST"; grep -E "^(FAIL|AB-TEST)" "$ABLOG" | tail -20; tail -15 "$ABLOG"; exit 1
  fi
else
  echo "FAIL  AB-TEST (e2fsprogs, dosfstools or mtools missing)"; exit 1
fi
