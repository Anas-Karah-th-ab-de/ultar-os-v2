#!/bin/bash
# Testnetz für den Ultar-Verbund: zwei getrennte Netze (Heimnetz 10.77.0.0/24, fremdes Netz 10.78.0.0/24),
# der Relais-Server (ultraos-server) hängt in beiden. uv1..uv4 = Heimnetz, uv5 = unterwegs (nur über Relais).
#   netz.sh auf | ab
set -e
case "$1" in
auf)
  ip link add uvbr1 type bridge 2>/dev/null || true
  ip link add uvbr2 type bridge 2>/dev/null || true
  ip link set uvbr1 up; ip link set uvbr2 up
  verbinde() { # ns brücke ip
    ip netns add $1
    ip link add v-$1 type veth peer name e0 netns $1
    ip link set v-$1 master $2 up
    ip -n $1 addr add $3/24 dev e0
    ip -n $1 link set e0 up
    ip -n $1 link set lo up
  }
  for i in 1 2 3 4; do verbinde uv$i uvbr1 10.77.0.$i; done
  verbinde uv5 uvbr2 10.78.0.5
  ip netns add uvsrv
  ip link add v-srv1 type veth peer name e1 netns uvsrv
  ip link add v-srv2 type veth peer name e2 netns uvsrv
  ip link set v-srv1 master uvbr1 up; ip link set v-srv2 master uvbr2 up
  ip -n uvsrv addr add 10.77.0.100/24 dev e1; ip -n uvsrv addr add 10.78.0.100/24 dev e2
  ip -n uvsrv link set e1 up; ip -n uvsrv link set e2 up; ip -n uvsrv link set lo up
  # Rundruf braucht eine Route für 255.255.255.255
  for i in 1 2 3 4 5; do ip -n uv$i route add default dev e0 2>/dev/null || true; done
  ;;
ab)
  # erst alle Prozesse in den Namensräumen beenden – sonst leben die Räume samt Kabeln weiter
  for n in uv1 uv2 uv3 uv4 uv5 uvsrv; do ip netns pids $n 2>/dev/null | xargs -r kill -9; ip netns del $n 2>/dev/null || true; done
  for l in v-uv1 v-uv2 v-uv3 v-uv4 v-uv5 v-srv1 v-srv2; do ip link del $l 2>/dev/null || true; done
  ip link del uvbr1 2>/dev/null || true; ip link del uvbr2 2>/dev/null || true
  ;;
esac
