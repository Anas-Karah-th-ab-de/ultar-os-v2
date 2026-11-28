#! /bin/bash
# Ultar-Kern mit QEMUs USB-Netz (RNDIS) prüfen: nach dem Boot `dhcp` und `netz` an die Kern-Shell
# schicken und sehen, ob über die USB-Netzkarte eine Adresse geholt wird. Nur usb-net (kein virtio-net),
# damit die USB-Karte die aktive ist.
# TRACE=1 sammelt zusätzlich QEMU-Ereignisspuren (usb_xhci*, usb_packet*) in $TRACE_LOG, um die
# Full-Speed-Bulk-IN-Ursache des nicht bestätigten Empfangs zu finden (siehe docs/integration/usb-net.md).
set -u
cd "$(dirname "$0")/../.."
OUT=out/ultar-kern-esp.img
LOG=${LOG:-/tmp/usb-net-qemu.log}
TRACE_LOG=${TRACE_LOG:-/tmp/usb-net-trace.log}
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/un-vars.fd
TRACE_ARGS=()
if [ "${TRACE:-0}" = 1 ]; then
  TRACE_ARGS=(-trace 'usb_xhci*' -trace 'usb_packet*' -D "$TRACE_LOG")
fi
( sleep 20; printf 'dhcp\n'; sleep 6; printf 'netz\n'; sleep 3 ) | \
  timeout ${DAUER:-40} qemu-system-x86_64 -machine q35 -m 1024 -smp 4 -cpu max -no-reboot \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file=/tmp/un-vars.fd \
    -drive file=$OUT,if=none,id=b,format=raw -device virtio-blk-pci,drive=b \
    -device qemu-xhci,id=xhci -netdev user,id=un -device usb-net,netdev=un,bus=xhci.0 \
    "${TRACE_ARGS[@]}" -display none -serial stdio -monitor none > "$LOG" 2>&1
echo "---- LOG"
grep -a -e usb-net -e DHCP -e "IP " "$LOG"
if [ "${TRACE:-0}" = 1 ]; then
  echo "---- TRACE"
  grep -a -e usb_xhci -e usb_packet "$TRACE_LOG" | tail -80
fi
