#!/usr/bin/env bash
# Kernel gate, VMware-like hardware: the same image and the same markers as the first boot of ci.sh, but on
# the device set a VMware Workstation machine offers and WITHOUT any virtio device:
#   UEFI (OVMF), GOP frame buffer of a plain VGA adapter, the disk on AHCI (SATA) or NVMe, Intel network card
#   (e1000e / e1000), xHCI with a USB keyboard and a USB tablet, or only the i8042 (PS/2) keyboard and mouse.
# Runs (each must reach every marker; a virtio driver that binds anything is a failure):
#   q35-ahci-usb    q35, start disk on the built-in AHCI controller, e1000e, xHCI + usb-kbd + usb-tablet
#   q35-nvme-ps2    q35, start disk on NVMe, e1000, input only through the i8042
#   pc-nvme-svga    i440FX/PIIX (PCI through the ports only, as on VMware's 440BX), NVMe, e1000e, xHCI and the
#                   VMware SVGA II adapter: the kernel has no driver for that adapter and none for the PIIX IDE
#                   controller, so it must keep the firmware's GOP frame buffer; here the marker "every device
#                   has a driver" is replaced by "only the IDE controller and the display are without one"
# RUNS="q35-ahci-usb" limits the list; REPEAT=3 repeats every run; VL_LOG=<prefix> keeps the serial logs.
# IMG=<esp image> (default out/ultar-kern-esp.img, built when missing); BUSCK=<file> as in ci.sh.
# Needs what ci.sh needs. QEMU is the test bed: VMware Workstation itself is not started here.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-out/ultar-kern-esp.img}"
if [ ! -f "$IMG" ]; then
  IMG=1 bash scripts/build-kern.sh >/dev/null
  IMG=out/ultar-kern-esp.img
fi
if ! mdir -i "$IMG" ::/bin/busck >/dev/null 2>&1; then
  if [ -z "${BUSCK:-}" ]; then
    RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p ultar-bus --example selfcheck --target x86_64-unknown-linux-gnu
    BUSCK="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/examples/selfcheck"
  fi
  mcopy -o -i "$IMG" "$BUSCK" ::/bin/busck
fi
USB="-device qemu-xhci,id=xh -device usb-kbd,bus=xh.0 -device usb-tablet,bus=xh.0"
# The bind/unbind cycle of geraetetest works on an e1000 (8086:100e) and turns its whole driver off for a moment,
# so that card must be the only one of the Intel driver: the PS/2 run has it as its only card and runs the
# cycle; the e1000e runs have no e1000 and geraetetest skips the cycle there (everything else is checked).
WANT=("alle Geräte haben einen Treiber" "Ergebnis: ALLES OK" "BUS SELFCHECK OK" "ZELLTEST: ALLES OK" "RECHTETEST: ALLES OK"
  "GERAETETEST: ALLES OK" "WECKERTEST: ALLES OK" "SPAWNTEST: ALLES OK" "FENSTERTEST: ALLES OK")
fail=0
run() { # name machine disk-device network-card extra-arguments [display]
  local name=$1 machine=$2 dev=$3 nic=$4 extra=$5 vga=${6:-std} n log bad expect
  for n in $(seq 1 "${REPEAT:-1}"); do
    log="${VL_LOG:+$VL_LOG-$name-$n.log}"; log="${log:-$(mktemp)}"
    MACHINE="$machine" DEV="$dev" NIC="$nic" QARGS="-vga $vga $extra" SMP="${SMP:-2}" WAIT="${WAIT:-120}" \
      timeout 900 python3 tools/kern-test/qtest.py "$IMG" "geraete" "/bin/kerntest" "/bin/busck" "/bin/zelltest" \
      "/bin/rechtetest" "/bin/geraetetest" "/bin/weckertest" "/bin/spawntest" "/bin/fenstertest" > "$log" 2>&1 || true
    bad=""
    for expect in "${WANT[@]}"; do
      if [ "$vga" = vmware ] && [ "$expect" = "${WANT[0]}" ]; then
        # driverless: exactly the IDE controller (class 0101) and the display (class 0300), and the GOP picture is there
        grep -aq "^WERKSTATT " "$log" || bad="$bad [no report of the driverless devices]"
        if grep -a "^WERKSTATT " "$log" | grep -avqE '"klasse":"(0101|0300)[0-9a-f]{2}"'; then bad="$bad [another device has no driver]"; fi
        grep -aqE "\[kern\] Bildschirm [0-9]+x[0-9]+" "$log" || bad="$bad [no GOP frame buffer]"
      else
        grep -aq "$expect" "$log" || bad="$bad [$expect]"
      fi
    done
    if grep -aqiE "kernel panic|PANIK" "$log"; then bad="$bad [kernel panic]"; fi
    if grep -aqE "\[treiber\] .* -> virtio" "$log"; then bad="$bad [a virtio device is present]"; fi
    if [ -z "$bad" ]; then
      echo "ok    VMWARE-LIKE $name ($n): ${#WANT[@]} markers, $(grep -ao "\[kern\] bereit nach [0-9]* ms" "$log" | head -1)"
    else
      echo "FAIL  VMWARE-LIKE $name ($n):$bad"; grep -aE "\[treiber\]|FEHLER" "$log" | tail -30; tail -15 "$log"; fail=1
    fi
  done
}
for r in ${RUNS:-q35-ahci-usb q35-nvme-ps2 pc-nvme-svga}; do
  case "$r" in
    q35-ahci-usb) run "$r" q35 "ide-hd,bus=ide.0" e1000e "$USB" ;;
    q35-nvme-ps2) run "$r" q35 "nvme,serial=ultar" e1000 "" ;;
    pc-nvme-svga) run "$r" pc "nvme,serial=ultar" e1000e "$USB" vmware ;;
    *) echo "FAIL  VMWARE-LIKE unknown run: $r"; fail=1 ;;
  esac
done
[ "$fail" = 0 ] && echo "VMWARE-LIKE: ALLES OK" || { echo "VMWARE-LIKE: FEHLER"; exit 1; }
