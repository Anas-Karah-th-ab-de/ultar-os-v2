#!/usr/bin/env bash
# Intel display driver test on the Beelink GTi15 Ultra without touching its boot: the integrated
# GPU (00:02.0, alone in IOMMU group 0) is taken from Linux' i915, passed to the Ultar kernel under
# KVM with VFIO, and ALWAYS given back to i915 when the script ends (also on errors and Ctrl-C).
# The kernel runs its self test; its verdict lines are printed at the end (INTELTEST: ALLES OK for
# the display, GTTEST: ALLES OK for the render/compute side with the GuC firmware upload).
#
# Usage (on the target, as root):  igpu-passthrough.sh <ultar-kern.efi> [bin-dir]
#   LIMIT=<seconds>  hard time limit of the VM (default 240)
#   REGS=0           skip the register snapshots (taken with i915 bound and after its unbind)
#   OPTIN=0          boot without the driver's opt-in: it must only read (verdict READ-ONLY OK)
#   PKGROOT=<dir>   a directory tree given to the kernel as a second, read-only disk (ext4): it
#                   becomes `/`, e.g. a tree with `pkg/<package>` from ultar-paketbau
#   TESTPROG=<file> one more program to run after the graphics tests: first line the command line,
#                   further lines NAME=value for its environment or `spur` (log its system calls)
#   GUC=<file>|0     GuC firmware for the GT test (default: the host's /lib/firmware/i915/mtl_guc_70.bin,
#                    also .zst/.xz); 0 = without (GTTEST then skips the upload)
# Needs: qemu-system-x86_64 (KVM), OVMF 4M, mtools, dosfstools; intel_reg (igt-gpu-tools) for REGS.
#
# What it does NOT do: stop any service, change the boot, or touch other PCI devices. It refuses to
# start when a process holds the GPU's DRM nodes (a desktop session), because unbinding i915 would
# take its display away.
set -euo pipefail
DEV=0000:00:02.0
EFI="${1:?usage: igpu-passthrough.sh <ultar-kern.efi> [bin-dir]}"
BINS="${2:-}"
W="$(mktemp -d /tmp/igpu-pt.XXXXXX)"
SYS=/sys/bus/pci/devices/$DEV
REGS_LIST="0x45400 0x45404 0x45504 0x46000 0x46070 0x44fe8 0x44300 0x70008 0x60400 0x46140 0x64600 0x64630 \
0x16f800 0x16f804 0x16f808 0x16f810 0x16f860 0x16150c 0xc4000 0xc4034 0x16fe50 0x16f870 0x108040 0x70180 0x7019c \
0x701ac 0x7027c 0x70240 0x7017c 0x45780 0x45270 0x60540 0x60544 0x604e0 0x44200 0x190010 0x60050 0x70040 0x70048"

[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }
[ -r "$EFI" ] || { echo "kernel image $EFI missing"; exit 1; }
for c in qemu-system-x86_64 mcopy mkfs.vfat; do command -v "$c" >/dev/null || { echo "$c missing"; exit 1; }; done
[ -e /dev/kvm ] || { echo "/dev/kvm missing"; exit 1; }
[ "$(basename "$(readlink -f "$SYS/iommu_group")")" = 0 ] || echo "note: 00:02.0 is not in IOMMU group 0"
others=$(ls "$SYS/iommu_group/devices" | grep -v "^$DEV$" || true)
[ -z "$others" ] || { echo "IOMMU group of $DEV has other devices ($others) - refusing"; exit 1; }

drv_of() { basename "$(readlink -f "$SYS/driver" 2>/dev/null)" 2>/dev/null || echo none; }
ORIG=$(drv_of)
[ "$ORIG" = i915 ] || [ "$ORIG" = xe ] || { echo "GPU driver is '$ORIG', expected i915 - refusing"; exit 1; }

# refuse if a process (desktop session) holds the DRM nodes of this GPU (checked through /proc,
# fuser is not installed everywhere)
# a stopped service may need a while to let go of the GPU: look again for up to 90 s
for try in $(seq 1 30); do
holders=""
for n in "$SYS"/drm/card* "$SYS"/drm/renderD*; do
  [ -e "$n" ] || continue
  node=/dev/dri/$(basename "$n")
  for fd in /proc/[0-9]*/fd/*; do
    [ "$(readlink "$fd" 2>/dev/null)" = "$node" ] && { holders="$holders $node(pid $(echo "$fd" | cut -d/ -f3))"; break; }
  done
done
  [ -z "$holders" ] && break
  sleep 3
done
[ -z "$holders" ] || { echo "processes use$holders - stop the desktop session first; nothing was changed"; exit 1; }

dump_regs() {   # $1 = label
  [ "${REGS:-1}" = 1 ] && command -v intel_reg >/dev/null || return 0
  echo "--- registers ($1)"
  for r in $REGS_LIST; do
    v=$(intel_reg read "$r" 2>/dev/null | grep -oE '0x[0-9a-f]{8}' | tail -1 || true)
    printf '%s=%s ' "$r" "${v:-?}"
  done
  echo
}

N0=$(dmesg | wc -l)   # kernel log lines before the test (for the wedge check)
RESET_METHOD=$(cat "$SYS/reset_method" 2>/dev/null || true)
echo "reset methods before: ${RESET_METHOD:-none}"
PM_CONTROL=$(cat "$SYS/power/control" 2>/dev/null || echo auto)

give_back() {
  set +e
  pkill -f "qemu-system-x86_64.*$DEV" 2>/dev/null
  sleep 1
  [ -w "$SYS/reset_method" ] && echo default > "$SYS/reset_method"
  [ -w "$SYS/power/control" ] && echo "$PM_CONTROL" > "$SYS/power/control"
  if [ "$(drv_of)" != "$ORIG" ]; then
    [ "$(drv_of)" = none ] || echo "$DEV" > "$SYS/driver/unbind"
    echo > "$SYS/driver_override"
    # function level reset before i915 takes the GPU again: without it the GuC/GSC firmware areas
    # stay locked from the first load and i915 declares the GT wedged (seen on the first run)
    if [ -w "$SYS/reset" ]; then echo 1 > "$SYS/reset" && echo "function level reset done" || echo "WARNING: reset failed"; fi
    echo "$DEV" > /sys/bus/pci/drivers_probe
    sleep 2
    [ "$(drv_of)" = "$ORIG" ] || { modprobe "$ORIG"; echo "$DEV" > "/sys/bus/pci/drivers/$ORIG/bind" 2>/dev/null; sleep 2; }
  fi
  echo "driver after the test: $(drv_of) (was $ORIG)"
  sleep 3
  if dmesg | tail -n +"$((N0 + 1))" | grep -qiE "wedged|Failed to initialize GPU"; then
    echo "WARNING: i915 reports the GT wedged after the rebind - the GPU render side needs a host reboot"
  else
    echo "i915 rebound cleanly (no wedged GT)"
  fi
  rm -rf "$W"
}
trap give_back EXIT INT TERM

# boot medium with the self-test trigger (the kernel writes its log into it)
truncate -s 64M "$W/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$W/esp.img" >/dev/null
mmd -i "$W/esp.img" ::/EFI ::/EFI/BOOT ::/bin ::/ultar-kern ::/ultar
mcopy -i "$W/esp.img" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
# opt-in: without it the driver only reads the hardware (docs/integration/intel-display.md §1)
if [ "${OPTIN:-1}" != 0 ]; then
  printf 'on\n' > "$W/intel-display"; mcopy -i "$W/esp.img" "$W/intel-display" ::/ultar/intel-display
fi
# GuC firmware for GTTEST (docs/integration/intel-gt.md): Intel's file, only loaded by the kernel
GUC="${GUC:-}"
if [ -z "$GUC" ]; then
  for f in /lib/firmware/i915/mtl_guc_70.bin /lib/firmware/i915/mtl_guc_70.bin.zst /lib/firmware/i915/mtl_guc_70.bin.xz; do
    [ -r "$f" ] && { GUC=$f; break; }
  done
fi
if [ -n "$GUC" ] && [ "$GUC" != 0 ]; then
  case "$GUC" in
    *.zst) zstd -dqc "$GUC" > "$W/mtl_guc_70.bin" ;;
    *.xz) xz -dc "$GUC" > "$W/mtl_guc_70.bin" ;;
    *) cp "$GUC" "$W/mtl_guc_70.bin" ;;
  esac
  if [ -s "$W/mtl_guc_70.bin" ]; then
    mmd -i "$W/esp.img" ::/firmware; mcopy -i "$W/esp.img" "$W/mtl_guc_70.bin" ::/firmware/mtl_guc_70.bin
    echo "GuC firmware: $GUC ($(stat -c %s "$W/mtl_guc_70.bin") bytes, sha256 $(sha256sum "$W/mtl_guc_70.bin" | cut -c1-16))"
  else
    echo "GuC firmware: $GUC could not be unpacked - GTTEST will skip the upload"
  fi
else
  echo "GuC firmware: none - GTTEST will skip the upload"
fi
EXTRA_DISK=""
if [ -n "${PKGROOT:-}" ]; then
  command -v mkfs.ext4 >/dev/null || { echo "mkfs.ext4 missing"; exit 1; }
  mb=$(( $(du -sm "$PKGROOT" | cut -f1) * 13 / 10 + 64 ))
  truncate -s "${mb}M" "$W/root.img"
  mkfs.ext4 -q -L pakete -O ^metadata_csum,^64bit,^has_journal -d "$PKGROOT" "$W/root.img"
  EXTRA_DISK="-drive file=$W/root.img,if=none,id=r,format=raw,readonly=on -device virtio-blk-pci,drive=r,addr=06.0"
  echo "package disk: $PKGROOT (${mb} MB, ext4, read only)"
fi
if [ -n "${TESTPROG:-}" ]; then mcopy -i "$W/esp.img" "$TESTPROG" ::/ultar/testprogramm; echo "extra program: $(head -1 "$TESTPROG")"; fi
if [ -n "$BINS" ]; then for f in "$BINS"/*; do mcopy -i "$W/esp.img" "$f" ::/bin/; done; fi
head -c 262144 /dev/zero | tr '\0' ' ' > "$W/log.txt"; mcopy -i "$W/esp.img" "$W/log.txt" ::/ultar-kern/selbsttest.txt
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd"

dump_regs "i915 bound"
# keep the display state Linux leaves after unbinding i915 (power wells, CDCLK, DBUF): no function
# level reset when QEMU opens the device and no runtime suspend while it is unbound. Both are
# restored by give_back. (After an FLR the display core would need a full re-initialisation.)
[ -w "$SYS/reset_method" ] && echo "" > "$SYS/reset_method" 2>/dev/null || echo "note: reset_method not writable - QEMU may reset the GPU"
echo on > "$SYS/power/control"
modprobe vfio-pci
echo vfio-pci > "$SYS/driver_override"
echo "$DEV" > "$SYS/driver/unbind"
dump_regs "unbound"
echo "$DEV" > /sys/bus/pci/drivers_probe
[ "$(drv_of)" = vfio-pci ] || { echo "vfio-pci did not take $DEV"; exit 1; }

run_vm() {   # $1 = extra vfio-pci options; ends QEMU as soon as the verdict line is in the log
  : > "$W/serial.log"
  timeout "${LIMIT:-240}" qemu-system-x86_64 -enable-kvm -machine q35 -cpu host -smp 4 -m 4096 \
    -display none -vga none -nodefaults -no-reboot \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive "if=pflash,format=raw,file=$W/vars.fd" \
    -drive "file=$W/esp.img,if=none,id=b,format=raw" -device virtio-blk-pci,drive=b,addr=05.0 \
    $EXTRA_DISK -device "vfio-pci,host=$DEV,id=igd,addr=02.0,rombar=0$1" \
    -serial "file:$W/serial.log" -monitor none 2> "$W/qemu.err" &
  local qp=$!
  while kill -0 "$qp" 2>/dev/null; do
    # GTTEST runs after INTELTEST; with a bin-dir that holds dritest, the program that uses the
    # render node runs right after it and is the last test that uses this device
    local done_re="GTTEST: (ALLES OK|FEHLER|no Intel)"
    [ -n "$BINS" ] && [ -e "$BINS/dritest" ] && done_re="DRITEST: |GTTEST: no Intel|Programm dritest: nicht"
    [ -n "${TESTPROG:-}" ] && done_re="Zusatzprogramm beendet|GTTEST: no Intel"
    if grep -aqE "$done_re|AUSNAHME|PANIK" "$W/serial.log" 2>/dev/null; then sleep 2; kill "$qp" 2>/dev/null; break; fi
    sleep 1
  done
  wait "$qp" 2>/dev/null || true
}
# IGD assignment without guest firmware support (OVMF has no Intel GOP): no legacy VGA ranges; the
# OpRegion is offered to the guest (fw_cfg) - if this QEMU refuses the option, run without it
run_vm ",x-igd-legacy-mode=off,x-igd-opregion=on"
if ! grep -aq "Ultar" "$W/serial.log" 2>/dev/null; then
  echo "QEMU with IGD options failed: $(head -3 "$W/qemu.err" | tr '\n' ' ') - retrying without them"
  run_vm ""
fi
echo "--- QEMU: $(head -3 "$W/qemu.err" | tr '\n' ' ')"
echo "--- kernel (intel-display, intel-gt, INTELTEST, GTTEST)"
grep -aE "intel-display|intel-gt|INTELTEST|GTTEST|treiber\] 00:02|panic|PANIK|Ausnahme" "$W/serial.log" | head -170 || true
if [ "${OPTIN:-1}" = 0 ]; then
  # without opt-in: the probe reports the read-only readout and no device is bound
  if grep -aq "not driven" "$W/serial.log" && grep -aq "INTELTEST: no Intel display device bound" "$W/serial.log"; then
    echo "VERDICT: INTELTEST: READ-ONLY OK"
  else
    echo "VERDICT: read-only boot not confirmed"; tail -30 "$W/serial.log"
  fi
elif grep -aq "INTELTEST: ALLES OK" "$W/serial.log"; then
  echo "VERDICT: INTELTEST: ALLES OK"
else
  echo "VERDICT: $(grep -aE 'INTELTEST: (FEHLER|no Intel)' "$W/serial.log" | tail -1 || echo 'no verdict (see serial tail below)')"
  echo "--- serial tail"; tail -30 "$W/serial.log"
fi
if [ "${OPTIN:-1}" != 0 ]; then
  echo "VERDICT: $(grep -aE 'GTTEST: (ALLES OK|FEHLER|no Intel)' "$W/serial.log" | tail -1 || echo 'GTTEST: no verdict')"
  if [ -n "$BINS" ] && [ -e "$BINS/dritest" ]; then
    echo "--- dritest (a program on /dev/dri/renderD128)"
    sed -n '/Programm dritest/,/DRITEST: /p' "$W/serial.log" | head -140
    echo "VERDICT: $(grep -aE 'DRITEST: ' "$W/serial.log" | tail -1 || echo 'DRITEST: no verdict')"
  fi
fi
if [ -n "${TESTPROG:-}" ]; then
  echo "--- extra program"
  sed -n '/Zusatzprogramm .*:$/,/Zusatzprogramm beendet/p' "$W/serial.log" | head -"${EXTRA_LINES:-400}"
  # the program's own verdict line (e.g. VKRECHNEN: ALLES OK ...), if it prints one
  echo "VERDICT: $(sed -n '/Zusatzprogramm .*:$/,/Zusatzprogramm beendet/p' "$W/serial.log" | grep -aE '^[A-Z]+: (ALLES OK|FEHLER)' | tail -1 | grep . || echo 'extra program: no verdict')"
  [ -n "${SERIAL_OUT:-}" ] && cp "$W/serial.log" "$SERIAL_OUT"
fi
