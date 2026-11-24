#!/usr/bin/env bash
# Bluetooth on the Ultar kernel with a USB controller. QEMU 8.2 has no `usb-bt-dongle`: tools/kern-test/bt-usb.py
# is the controller, behind `-device usb-redir` on a qemu-xhci (usbredir protocol, full-speed device of class
# E0/01/01). In the guest:
#   1. bttest hci – /dev/ultar/hci0: rights, one opener, HCI Reset, Read BD_ADDR, an event cut over 17 USB packets,
#      ACL both ways, refused records, O_NONBLOCK and poll (BTTEST-HCI: ALLES OK);
#   2. btd-e2e – ultar-init starts the shipped logd, bus and btd; btd (own user, only its rights) sets the
#      controller up through the node, bluetooth.state says present + devices supported, and btd's input sink
#      creates a keyboard through /dev/ultar/uinput that the seat reads (BT: ALLES OK).
# The controller's log must show btd's own setup commands (Set Event Mask, Write SSP Mode, Write LE Host Supported)
# after HCI Reset and Read BD_ADDR. Verdict line: "ok    BT: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# BT_BIN=<dir> with static builds of the programs and examples/btd-e2e (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
PROGRAMS="ultar-init ultar-busd ultar-logd ultar-btd"
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${BT_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-btd --bins
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-btd --example btd-e2e
  BT_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); EMU=""
trap 'if [ -n "$EMU" ]; then kill "$EMU" 2>/dev/null || true; fi; rm -rf "$WORK"' EXIT
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in $PROGRAMS examples/btd-e2e; do
  cp "$BT_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/bt-e2e.log}"; ELOG="$WORK/controller.log"
python3 tools/kern-test/bt-usb.py "$WORK/bt.sock" "$ELOG" & EMU=$!
for _ in $(seq 50); do [ -S "$WORK/bt.sock" ] && break; sleep 0.1; done
QARGS="-device qemu-xhci,id=xh -chardev socket,id=btc,path=$WORK/bt.sock -device usb-redir,chardev=btc,bus=xh.0" \
  SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-300}" timeout 900 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/bttest hci" "/bin/btd-e2e --bin /bin --work /tmp/bt" > "$LOG" 2>&1 || true
# btd's setup after the test's own commands: Reset, Read BD_ADDR, Set Event Mask, Write SSP Mode, Write LE Host Supported
setup=$(grep -a "BTEMU cmd" "$ELOG" | sed -n '/0xfc77/,$p' | tr '\n' ' ')
btd_setup=0
for op in 0x0c03 0x1009 0x0c01 0x0c56 0x0c6d; do echo "$setup" | grep -q "cmd $op" || btd_setup=1; done
if grep -aq "BTTEST-HCI: ALLES OK" "$LOG" && grep -aq "^BT: ALLES OK" "$LOG" && [ "$btd_setup" = 0 ] \
   && grep -aqE "exit btd pid=[0-9]+ code=0 " "$LOG" && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    BT: ALLES OK (USB controller node, ultar-btd set it up as its own user, input through uinput)"
  if [ "${VERBOSE:-0}" = 1 ]; then grep -aE "^BT|BTTEST" "$LOG"; fi
  exit 0
else
  echo "FAIL  BT"; grep -aE "^(FEHLER|BT)|BTTEST|ultar-init|refused|exit |\[bt\]" "$LOG" | tail -40
  echo "controller:"; tail -20 "$ELOG"; exit 1
fi
