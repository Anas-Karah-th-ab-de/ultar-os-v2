#!/usr/bin/env bash
# The VMware machine's disk, driven ONLY through emulated input devices (tools/vm/vmware-input.py gate):
# boot -> the lock screen covers everything (the desktop is never visible before the unlock) -> a key opens
# the sign-in, a wrong password keeps it locked, the password unlocks -> the start button opens the start
# menu -> Settings, Files, Store and the browser from the bar and the phone from the app grid each open their
# window -> the clock in the bar turns (the screen is not static). Keys and pointer go through QEMU's
# emulated devices (QMP input-send-event), as VMware's reach the kernel's drivers; no test hook of the system.
# Runs: usb (q35, xHCI with USB keyboard and tablet, as the .vmx, firmware screen 1280x800), ps2 (i440FX,
# PS/2 keyboard and the VMware absolute pointer through the i8042, VMware SVGA II, no USB) and usb-hd (as usb,
# firmware screen 1920x1080: the session must fill whatever size the firmware set). REPEAT=3 repeats each.
# On the lock screen the run also checks what the Lock board shows: the whole frame buffer covered, brand,
# avatar, name and password field drawn (Arabic and Latin text), the accessibility menu only on a click and
# with the names of its switches.
# VMDK=<disk> uses a built disk (its account's password in VM_PASSWORD_FILE, digits only: the lock screen
# starts with the Arabic layout); else tools/vm/make-vmware.sh builds one with the test password.
# Verdict line: "VMWARE-INPUT-E2E: ALLES OK". Logs: out/vmware/input-<run>-<n>.log (+ -log/ on failure).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
OUT=out/vmware
mkdir -p "$OUT"
if [ -z "${VMDK:-}" ]; then
  # the test password (digits), kept beside the image like every image's password
  PW=$OUT/test.password
  (umask 077; printf '24680135792\n' >"$PW")
  ACCEL=${ACCEL:-$([ -w /dev/kvm ] && echo kvm || echo tcg)} VM_PASSWORD_FILE="$PW" CHECK=0 \
    bash tools/vm/make-vmware.sh >"$OUT/input-build.log" 2>&1 || { tail -20 "$OUT/input-build.log"; echo "VMWARE-INPUT-E2E: FEHLER build"; exit 1; }
  VMDK=$OUT/UltarOS-v2/UltarOS-v2.vmdk
  VM_PASSWORD_FILE=$PW
fi
ULOG_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/ulog"
fail=0
for run in ${INPUT_RUNS:-usb ps2 usb-hd}; do
  for n in $(seq 1 "${REPEAT:-1}"); do
    prefix=$OUT/input-$run-$n
    input=${run%-hd}; vga=""
    [ "$run" = usb-hd ] && vga="VGA,xres=1920,yres=1080"
    if INPUT=$input VGA_DEVICE="$vga" PASSWORD_FILE="$VM_PASSWORD_FILE" ULOG="$ULOG_BIN" timeout 1500 \
        python3 tools/vm/vmware-input.py gate "$VMDK" vmdk "$prefix" >"$prefix.out" 2>&1 \
        && grep -aq "^VMWARE-INPUT: ALLES OK" "$prefix.out"; then
      echo "ok    VMWARE-INPUT $run ($n)"
    else
      echo "FAIL  VMWARE-INPUT $run ($n)"; grep -aE "VMWARE-INPUT|screen|window|logs:" "$prefix.out" | tail -20; fail=1
    fi
  done
done
[ "$fail" = 0 ] && echo "VMWARE-INPUT-E2E: ALLES OK" || { echo "VMWARE-INPUT-E2E: FEHLER"; exit 1; }
