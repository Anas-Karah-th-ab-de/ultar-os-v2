#!/bin/bash
# Angriffsfläche des Ultar-Kerns prüfen: Kern in QEMU (tap-Netz), Angreifer im Linux-Namensraum.
# Phase 1 (Brandmauer zu, Dienst lauscht auf 8080/8081): Portscan aller 65535 Ports, UDP/Ping, 30 000 kaputte
#   Rahmen, ARP-Flut, SYN-Flut → nichts darf erreichbar sein, der Kern darf nicht abstürzen.
# Phase 2 (Port 8080 geöffnet): echte Verbindung geht, SYN-Flut auf den offenen Port, blind eingeschleuste RSTs
#   → der Dienst bleibt erreichbar. Danach: Speicher stabil, Kern antwortet.
#   sudo tools/kern-test/angriff-test.sh
set -u
R=$(cd "$(dirname "$0")/../.." && pwd); cd $R
T=/tmp/angriff; rm -rf $T; mkdir -p $T
A0="ip netns exec uvk1 python3 $R/tools/kern-test/angriff.py"
A() { local t=$(date +%s); $A0 "$@"; echo "$(( $(date +%s) - t )) s  $*" >> $T/zeiten.txt; }
ab() { ip netns pids uvk1 2>/dev/null | xargs -r kill -9; ip netns del uvk1 2>/dev/null; ip link del v-uvk1 2>/dev/null; ip link del uvkbr 2>/dev/null; ip link del uvktap 2>/dev/null; true; }
ab; trap ab EXIT
ip link add uvkbr type bridge; ip link set uvkbr up
ip tuntap add uvktap mode tap; ip link set uvktap master uvkbr up
ip netns add uvk1; ip link add v-uvk1 type veth peer name e0 netns uvk1; ip link set v-uvk1 master uvkbr up
ip -n uvk1 addr add 10.79.0.1/24 dev e0; ip -n uvk1 link set e0 up; ip -n uvk1 link set lo up
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }
j() { python3 -c "import json,sys;d=json.load(open('$1'));print($2)"; }

# Kern-Seite: Dienst starten, Speicher vorher, Phase-1-Zeit, Port öffnen, Phase-2-Zeit, Speicher nachher
NETDEV="tap,id=n,ifname=uvktap,script=no,downscript=no" NIC=e1000 SMP=4 MEM=1024 WAIT=330 \
python3 tools/kern-test/qtest.py out/ultar-kern-disk.img \
  "ipfest 10.79.0.2/24 10.79.0.1" \
  "sicherheit" \
  "bash -c 'head -2 /proc/meminfo; /boot/efi/bin/lauscher & sleep 150; echo offen 8080 > /proc/ultar/firewall; echo PHASE\$((1+1)); sleep 55; head -2 /proc/meminfo; cat /proc/ultar/sicherheit; echo KERN-\$((6*7))-LEBT'" \
  > $T/qemu.txt 2>&1 &
Q=$!

# Phase 1 beginnt, wenn der Dienst lauscht
for i in $(seq 120); do grep -q "LAUSCHER BEREIT" $T/qemu.txt && break; sleep 1; done
sleep 2
A scan 10.79.0.2 > $T/scan1.json
A fuzz 10.79.0.2 e0 15000 > $T/fuzz.json
A arpflut 10.79.0.2 e0 10000 > $T/arp.json
A synflut 10.79.0.2 e0 10000 8080 > $T/syn1.json
A hallo 10.79.0.2 8080 > $T/hallo1.json
grep -q "^PHASE2" $T/qemu.txt && echo "WARNUNG: Phase 1 dauerte zu lange" > $T/zeit.txt
# Phase 2: der Kern öffnet nach 150 s Port 8080 (von außen gibt es keinen Weg hinein – auch nicht für den Test)
for i in $(seq 300); do grep -q "^PHASE2" $T/qemu.txt && break; sleep 1; done
sleep 1
A hallo 10.79.0.2 8080 8 > $T/hallo2.json
A synflut 10.79.0.2 e0 10000 8080 > $T/syn2.json
A hallo 10.79.0.2 8080 8 > $T/hallo3.json
A rstflut 10.79.0.2 e0 3000 8080 > $T/rst.json
A scan 10.79.0.2 > $T/scan2.json
wait $Q

echo "== Ergebnisse"
c "Prozessor-Schutz SMEP/SMAP/UMIP aktiv" "grep -q 'prozessor SMEP SMAP UMIP' $T/qemu.txt"
c "Portscan (65535 TCP): nichts offen, keine Antwort (gefiltert)" "[ \"\$(j $T/scan1.json 'len(d[\"tcp_offen\"]),d[\"tcp_geschlossen_rst\"]')\" = '0 0' ]"
c "lauschender Dienst trotzdem unsichtbar (8080/8081)" "[ ! -f $T/zeit.txt ] && [ \"\$(j $T/scan1.json 'd[\"udp_antwort\"]')\" = '[]' ] && grep -q fehler $T/hallo1.json"
c "keine Ping-Antwort" "[ \"\$(j $T/scan1.json 'd[\"ping\"]')\" = 'False' ]"
c "15 000 kaputte Rahmen, 10 000 ARP, 10 000 SYN, ARP-Flut, SYN-Flut überstanden" "grep -q '^PHASE2' $T/qemu.txt && ! grep -aq 'PANIK\|AUSNAHME' $T/qemu.txt"
c "geöffneter Port erreichbar" "grep -q '\"hallo\"' $T/hallo2.json"
c "nach SYN-Flut auf den offenen Port weiter erreichbar" "grep -q '\"hallo\"' $T/hallo3.json"
c "3000 blind eingeschleuste RST trennen die Verbindung nicht" "grep -q '\"hallo\"' $T/rst.json"
c "nach dem Öffnen nur 8080 sichtbar" "[ \"\$(j $T/scan2.json 'd[\"tcp_offen\"]')\" = '[8080]' ]"
M1=$(grep -a MemFree $T/qemu.txt | head -1 | awk '{print $2}'); M2=$(grep -a MemFree $T/qemu.txt | tail -1 | awk '{print $2}')
c "Speicher stabil nach allen Angriffen (${M1} → ${M2} kB frei)" "[ -n '$M1' ] && [ -n '$M2' ] && [ \$(( $M1 - $M2 )) -lt 20000 ]"
c "Kern lebt und antwortet" "grep -aq KERN-42-LEBT $T/qemu.txt"
grep -a "verworfen" $T/qemu.txt | tail -1
echo "$ok bestanden, $fehl fehlgeschlagen"
[ $fehl -eq 0 ]
