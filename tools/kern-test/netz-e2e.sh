#!/usr/bin/env bash
# The wired network on the Ultar kernel with each network card it drives in VMs: QEMU user network
# (router/host 10.0.2.2, DNS 10.0.2.3). Per card nettest (kernel/ultar-kern/user/nettest.c) checks
# /sys/class/net (MAC, carrier, operstate, speed, driver), the DHCP lease (address, router, DNS, lease
# time, default route), a DNS answer through the leased server, a TCP connection to a listener on the
# host, renewing the lease, a static address (and a bad one refused), DHCP again; then the cable is
# pulled and put back through the QEMU monitor (set_link): carrier 0, then 1 and a lease again.
#   e1000      q35, Intel 82540EM (VMware's "e1000")
#   virtio     q35, virtio-net (KVM/QEMU, cloud)
#   e1000e     pc (i440FX like VMware's 440BX), Intel 82574L (VMware's "e1000e")
#   vmxnet3    pc, VMware's paravirtual card (when this QEMU has the device)
# IMG=<esp image> (else scripts/build-kern.sh builds one); NETZ_NICS="e1000 virtio …" selects cases.
# Verdict line: "NETZ: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
WORK=$(mktemp -d)
# the host side of the TCP check: answers every line with "OK <line>"
PORT=$((20000 + RANDOM % 20000))
python3 - "$PORT" > "$WORK/listener.log" 2>&1 <<'PY' &
import socket, sys, threading
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(8)
def serve(c):
    with c:
        f = c.makefile("rb"); line = f.readline().strip()
        print("listener:", line.decode(errors="replace"), flush=True)
        c.sendall(b"OK " + line + b"\n")
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
PY
LISTENER=$!
trap 'kill $LISTENER 2>/dev/null; rm -rf "$WORK"' EXIT
sleep 0.5
has_dev() { qemu-system-x86_64 -device help 2>/dev/null | grep -q "\"$1\""; }
fail=0
run() {
  local name=$1 machine=$2 nic=$3 drv=$4 dev=$5 log="$WORK/$1.log" mon="$WORK/$1.mon"
  if ! has_dev "$nic"; then echo "skip  NETZ $name (QEMU has no $nic)"; return; fi
  MACHINE="$machine" DEV="$dev" NIC="$nic,id=nic0" MONITOR="$mon" QARGS="-monitor unix:$mon,server,nowait" \
    SMP="${SMP:-2}" WAIT="${WAIT:-120}" timeout 600 python3 tools/kern-test/qtest.py "$IMG" \
    "/bin/nettest $drv $PORT" "!set_link nic0 off" "/bin/nettest link 0 $drv" "!set_link nic0 on" "/bin/nettest link 1 $drv" \
    > "$log" 2>&1 || true
  local links
  links=$(grep -ac "NETTEST-LINK: ALLES OK" "$log" || true)
  if grep -aq "NETTEST: ALLES OK" "$log" && [ "$links" = 2 ] && ! grep -aqiE "kernel panic|PANIK" "$log"; then
    echo "ok    NETZ $name ($drv): $(grep -aoE 'DHCP lease [^ ]+ router [^ ]+' "$log" | head -1), DNS $(grep -aoE 'rcode [0-9]+, [0-9]+ answer' "$log" | head -1), TCP to the host, renew, static, cable out/in"
  else
    echo "FAIL  NETZ $name ($drv)"; grep -aE "FEHLER|NETTEST|\[(intel-eth|virtio-net|vmxnet3|netz)\]" "$log" | tail -30; fail=1
  fi
  if [ -n "${NETZ_LOG:-}" ]; then cp "$log" "$NETZ_LOG-$name.log"; fi
}
for c in ${NETZ_NICS:-e1000 virtio e1000e vmxnet3}; do
  case "$c" in
    e1000) run e1000 q35 e1000 gen-intel-eth virtio-blk-pci ;;
    virtio) run virtio q35 virtio-net-pci virtio-net virtio-blk-pci ;;
    e1000e) run e1000e pc e1000e gen-intel-eth "nvme,serial=ultar" ;;
    vmxnet3) run vmxnet3 pc vmxnet3 vmxnet3 "nvme,serial=ultar" ;;
  esac
done
[ "$fail" = 0 ] && echo "NETZ: ALLES OK" || { echo "NETZ: FEHLER"; exit 1; }
