#!/bin/bash
# Ultar-Verbund auf dem eigenen Kern: Ultar-Kern (QEMU, tap-Netz) + Linux-Gerät (Namespace) im selben Netz.
# Prüft: Kern-Erkennung (UDP 47800 im Kern), ultar-verbund-Dienst läuft auf dem Ultar-Kern, Kopplung per Code,
# verteiltes Rechnen auf den Ultar-Kern mit korrekten Ergebnissen, harter Kern-Schalter AUS (Ports gesperrt).
#   sudo tools/verbund-chaos/kern-test.sh
set -u
D=$(cd "$(dirname "$0")" && pwd); R=$(cd $D/../.. && pwd)
source $D/lib.sh
T=/tmp/uvkern; rm -rf $T; mkdir -p $T/d1
ab() { ip netns pids uvk1 2>/dev/null | xargs -r kill -9; ip netns del uvk1 2>/dev/null; ip link del v-uvk1 2>/dev/null; ip link del uvkbr 2>/dev/null; ip link del uvktap 2>/dev/null; true; }
ab; trap ab EXIT
ip link add uvkbr type bridge; ip link set uvkbr up
ip tuntap add uvktap mode tap; ip link set uvktap master uvkbr up
ip netns add uvk1; ip link add v-uvk1 type veth peer name e0 netns uvk1
ip link set v-uvk1 master uvkbr up
ip -n uvk1 addr add 10.79.0.1/24 dev e0; ip -n uvk1 link set e0 up; ip -n uvk1 link set lo up
ip -n uvk1 route add default dev e0
k() { ip netns exec uvk1 env ULTAR_VERBUND_DIR=$T/d1 ULTAR_VERBUND_LOG=1 PATH=$BIN:$PATH "$@"; }
k ultar-verbund an >/dev/null; k ultar-verbund name linux1 >/dev/null
k ultar-verbund dienst > $T/d1/log 2>&1 &
DPID=$!

# Linux-Seite, zeitversetzt zur Kern-Seite
(
  sleep 40
  # warten, bis sich der Ultar-Kern im Netz meldet (höchstens 90 s)
  for i in $(seq 90); do
    k ultar-verbund geraete > $T/geraete.txt 2>&1
    grep -q ultar-kern $T/geraete.txt && break; sleep 1
  done
  date +%T >> $T/zeiten.txt; echo "gesehen nach $((40+i)) s" >> $T/zeiten.txt
  for i in $(seq 20); do
    k ultar-verbund koppeln ultar-kern 246810 > $T/koppeln.txt 2>&1 && break; sleep 2
  done
  sleep 3
  k ultar-verbund verteilt -n 24 --ohne-lokal --json -- ultar-verbund arbeit {} 100000 > $T/verteilt.json 2> $T/verteilt.err
  k ultar-verbund ausfuehren ultar-kern -- uname -sr > $T/steuern.txt 2>&1
  k ultar-verbund ausfuehren 10.79.0.2:47801 -- echo per-ip > $T/vor-aus.txt 2>&1
  touch $T/linux-fertig
  # Kern-Seite per Steuern weiterschalten: sie schaltet den Verbund dann im Kern AUS
  k ultar-verbund ausfuehren ultar-kern -- bash -c 'echo x > /root/uv/weiter' >/dev/null 2>&1
  sleep 12
  timeout 10 ip netns exec uvk1 env ULTAR_VERBUND_DIR=$T/d1 PATH=$BIN:$PATH ultar-verbund ausfuehren 10.79.0.2:47801 -- echo durch > $T/nach-aus.txt 2>&1; echo "rc=$?" >> $T/nach-aus.txt
) &

cd $R
[ -f out/ultar-kern-disk.img ] || tools/kern-test/make-test-disk.sh
NETDEV="tap,id=n,ifname=uvktap,script=no,downscript=no" NIC=e1000 SMP=4 MEM=2048 WAIT=300 \
python3 tools/kern-test/qtest.py out/ultar-kern-disk.img \
  "ipfest 10.79.0.2/24 10.79.0.1" \
  "verbund an" \
  "netz" \
  "@" "@" "@" \
  "verbund" \
  "bash -c 'export HOME=/root ULTAR_VERBUND_DIR=/root/uv PATH=/usr/bin:/bin; ultar-verbund an; ultar-verbund name ultar-kern; rm -f /root/uv/weiter; (ultar-verbund dienst > /root/uv.log 2>&1 &); sleep 4; ULTAR_VERBUND_CODE=246810 ultar-verbund koppeln; ultar-verbund geraete; ultar-verbund status; until grep -q x /root/uv/weiter 2>/dev/null; do sleep 1; [ \$SECONDS -gt 240 ] && break; done; rm -f /root/uv/weiter; cat /proc/ultar/verbund; echo aus > /proc/ultar/verbund; cat /proc/ultar/verbund; sleep 30; echo ENDE'" \
  > $T/qemu.txt 2>&1
kill $DPID 2>/dev/null; wait 2>/dev/null

echo "== Ergebnisse"
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }
c "Kern erkennt Linux-Gerät (UDP 47800 im Kern)" "grep -q 'gefunden: UV-.*linux1' $T/qemu.txt"
c "ultar-verbund-Dienst läuft auf dem Ultar-Kern" "grep -q 'ultar-kern' $T/geraete.txt"
c "Kopplung Linux ↔ Ultar-Kern per Code" "grep -q 'gekoppelt mit „ultar-kern“' $T/koppeln.txt"
c "verteiltes Rechnen auf dem Ultar-Kern korrekt" "(cd $T && pruefe verteilt.json 24 100000)"
c "Steuern: Befehl lief auf dem Ultar-Kern" "grep -qi 'linux' $T/steuern.txt"
c "Gegenprobe: vor AUS per IP erreichbar" "grep -q '^per-ip' $T/vor-aus.txt"
c "Kern-Schalter AUS sperrt den Verbund" "grep -q 'Verbund-Ports gesperrt' $T/qemu.txt && ! grep -q '^durch' $T/nach-aus.txt && grep -Eqi 'timed out|refused|reset|rc=124' $T/nach-aus.txt"
echo "$ok bestanden, $fehl fehlgeschlagen"
[ $fehl -eq 0 ]
