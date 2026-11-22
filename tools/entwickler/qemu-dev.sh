#!/usr/bin/env bash
# Ultar-Kern in QEMU mit Entwickler-Zugang starten: SSH vom Rechner auf localhost:2222 → Gerät :2222.
# Serielle Konsole (Kernprotokoll ab dem ersten Takt) landet in out/qemu-dev.log.
#   tools/entwickler/qemu-dev.sh            Platte bauen (ENTWICKLER=1, MIT_SHELL=…) und starten (Hintergrund)
#   tools/entwickler/qemu-dev.sh stop       beenden
#   GUI=1 …                                 mit Bildschirm-Gerät (Oberfläche läuft, Bilder: ssh ultar-qemu …)
# Danach:  ssh -p 2222 -i ~/.ssh/ultar_dev_ed25519 root@127.0.0.1 kernlog
set -euo pipefail
cd "$(dirname "$0")/../.."
PIDF=out/qemu-dev.pid
if [ "${1:-}" = stop ]; then
  [ -f $PIDF ] && kill "$(cat $PIDF)" 2>/dev/null && echo "QEMU beendet"; rm -f $PIDF; exit 0
fi
[ -f $PIDF ] && kill -0 "$(cat $PIDF)" 2>/dev/null && { echo "läuft schon (PID $(cat $PIDF))"; exit 0; }
DISK=${DISK:-out/ultar-kern-dev.img}
if [ "${NEU:-1}" = 1 ]; then
  if [ "${GUI:-0}" = 1 ]; then export MIT_SHELL=1 DESKTOP_AUTOSTART=1 ROOT_MB=${ROOT_MB:-240}; fi
  ENTWICKLER=1 DISK=$DISK ROOT_MB=${ROOT_MB:-160} tools/kern-test/make-test-disk.sh
fi
VARS=out/qemu-dev-vars.fd; cp /usr/share/OVMF/OVMF_VARS_4M.fd $VARS
ANZEIGE=(-display none)
[ "${GUI:-0}" = 1 ] && ANZEIGE=(-display none -device VGA,xres=1280,yres=800)
qemu-system-x86_64 -machine q35 -m "${MEM:-2048}" -smp "${SMP:-4}" -cpu max "${ANZEIGE[@]}" \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive if=pflash,format=raw,file=$VARS \
  -drive file=$DISK,if=none,id=b,format=raw -device virtio-blk-pci,drive=b \
  -netdev "user,id=n,hostfwd=tcp:127.0.0.1:${PORT:-2222}-:2222" -device "${NIC:-virtio-net-pci},netdev=n" \
  -serial file:out/qemu-dev.log -daemonize -pidfile $PIDF
echo "QEMU läuft (PID $(cat $PIDF)), Konsole: out/qemu-dev.log, SSH: Port ${PORT:-2222}"
for i in $(seq 1 60); do grep -q "\[entwickler\]" out/qemu-dev.log 2>/dev/null && break; sleep 1; done
grep -a "\[entwickler\]" out/qemu-dev.log || echo "Entwickler-Zugang nicht gemeldet – siehe out/qemu-dev.log"
