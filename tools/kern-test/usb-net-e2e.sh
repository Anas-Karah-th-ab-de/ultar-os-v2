#!/usr/bin/env bash
# USB-Netz des Ultar-Kerns in QEMU: Bind, MAC, DHCP-Lease von slirp, ping 10.0.2.2, TCP zum Host
# (nettest mit Treiber-Argument), Abziehen (device_del) und erneutes Anstecken (device_add).
#   RNDIS  QEMUs usb-net Konfiguration 1 (Handy-Tethering): vollständig geprüft.
#   CDC-ECM QEMUs usb-net Konfiguration 2: der Treiber wählt – wie Linux – zuerst die RNDIS-
#          Konfiguration, wenn das Gerät beide anbietet; weil die kerninterne Auswahl der
#          Konfiguration nicht von außen umschaltbar ist, kann ECM an QEMUs Zwei-Konfigurations-
#          usb-net nicht vollständig geprüft werden und wird ehrlich als „übersprungen" gemeldet.
# Urteil: "USBNET: ALLES OK" nur wenn alles lief; sonst "USBNET: TEILWEISE <liste>" bzw. FEHLER.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
WORK=$(mktemp -d)
PORT=$((20000 + RANDOM % 20000))

# Host-Seite der TCP-Prüfung: antwortet auf jede Zeile mit "OK <zeile>"
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

fail=0
skipped=()

# ein QEMU-Lauf: nettest, ping, dann die angegebenen Zusatzbefehle (Monitor mit "!", Shell-Kommandos)
run() {
  local log="$1"; shift
  QARGS="-device qemu-xhci,id=xhci -monitor unix:$WORK/mon,server,nowait" MONITOR="$WORK/mon" \
    NETDEV="user,id=n" NIC="usb-net,bus=xhci.0,id=unet" SMP="${SMP:-2}" WAIT="${WAIT:-160}" \
    timeout 700 python3 tools/kern-test/qtest.py "$IMG" "$@" > "$log" 2>&1 || true
}

# --- RNDIS: Bind, MAC, DHCP, ping, TCP, unplug, replug
LOG="$WORK/rndis.log"
run "$LOG" \
  "/bin/nettest usb-rndis $PORT" \
  "ping 10.0.2.2" \
  "!device_del unet" \
  "!device_add usb-net,bus=xhci.0,id=unet,netdev=n" \
  "/bin/nettest usb-rndis $PORT"
echo "---- RNDIS ----"
grep -aE "usb0:|usb1:|entfernt|NETTEST|Antwort von|keine Antwort|PANIK|panik" "$LOG"
if [ "$(grep -ac '^NETTEST: ALLES OK' "$LOG" || true)" = 2 ] \
   && grep -aq "Antwort von 10.0.2.2" "$LOG" \
   && grep -aq "usb0: RNDIS" "$LOG" \
   && grep -aq "entfernt" "$LOG" \
   && grep -aq "usb1: RNDIS" "$LOG" \
   && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    RNDIS: bind, MAC/DHCP/TCP ($(grep -aoE 'DHCP lease [^ ]+ router [^ ]+' "$LOG" | head -1)), ping, unplug, replug"
else
  echo "FAIL  RNDIS"; fail=1
fi

# --- CDC-ECM: an QEMUs usb-net nicht vollständig prüfbar (siehe Kopf) → ehrlicher Skip
skipped+=("cdc-ecm (QEMU usb-net bindet zuerst RNDIS-Konfiguration 1; ECM braucht einen echten ECM-Dongle)")

if [ "$fail" = 0 ] && [ "${#skipped[@]}" = 0 ]; then
  echo "USBNET: ALLES OK"
elif [ "$fail" = 0 ]; then
  echo "USBNET: TEILWEISE ${skipped[*]}"
else
  echo "USBNET: FEHLER"; exit 1
fi