#!/bin/bash
# Chaos-Engineering für den Ultar-Verbund (Geräte + Relais-Server).
# Baut ein Testnetz aus Linux-Namespaces (4 Geräte im Heimnetz, 1 Gerät unterwegs, Relais-Server),
# rechnet verteilte Aufgaben und stört dabei gezielt: Abstürze, Netztrennung, Relais-Ausfall,
# kaputte/verlorene/verzögerte Rahmen, Ausschalten mitten im Lauf, Totalausfall, Angriffe.
# Invariante jedes Laufs: GENAU n Ergebnisse, jedes korrekt (SHA-256-Kette unabhängig nachgerechnet).
#   sudo tools/verbund-chaos/chaos.sh            (braucht root für Namespaces)
set -u
D=$(cd "$(dirname "$0")" && pwd)
source $D/lib.sh
N=${N:-120}; IT=${IT:-500000}
rm -rf $T; mkdir -p $T
BER=$T/bericht.txt; : > $BER
ok=0; fehl=0
ergebnis() { # name status text
  printf "%-14s %-6s %s\n" "$1" "$2" "$3" | tee -a $BER
  if [ "$2" = OK ]; then ok=$((ok+1)); else fehl=$((fehl+1)); fi
}
lauf() { # name [verteilt-optionen …] → rechnet N Aufgaben auf uv1 und prüft
  local name=$1; shift
  local t0=$(date +%s%N)
  uv 1 env "${CLIENT_ENV[@]}" ultar-verbund verteilt -n $N --json "$@" -- ultar-verbund arbeit {} $IT > $T/$name.json 2> $T/$name.err
  local ms=$(( ($(date +%s%N)-t0)/1000000 ))
  local p; p=$(pruefe $T/$name.json $N $IT 2>&1)
  if [ $? -eq 0 ]; then ergebnis $name OK "$ms ms – $(grep -o 'wiederholt.*' $T/$name.err | head -1)"; else ergebnis $name FEHLER "$p"; fi
}
CLIENT_ENV=(X=1)

aufraeumen() {
  for n in 1 2 3 4 5; do [ -f $T/d$n/aufpasser ] && dienst_stop $n; done 2>/dev/null
  server_stop 2>/dev/null; $D/netz.sh ab
}
trap aufraeumen EXIT

echo "== Aufbau"; $D/netz.sh ab; $D/netz.sh auf
server_start || exit 1
TOK=$(ip netns exec uvsrv curl -s -X POST http://127.0.0.1:8080/v1/account/register -H 'content-type: application/json' -d '{"username":"anas","password":"geheim123"}' | jq -r .token)
TOK2=$(ip netns exec uvsrv curl -s -X POST http://127.0.0.1:8080/v1/account/register -H 'content-type: application/json' -d '{"username":"fremd","password":"geheim456"}' | jq -r .token)
for n in 1 2 3 4 5; do mkdir -p $T/d$n; uv $n ultar-verbund an >/dev/null; uv $n ultar-verbund name geraet$n >/dev/null; done
for n in 1 2 3; do uv $n ultar-verbund relais ws://10.77.0.100:8080/v1/verbund/relais >/dev/null; done
uv 5 ultar-verbund relais ws://10.78.0.100:8080/v1/verbund/relais >/dev/null
for n in 1 2 3 5; do uv $n ultar-verbund konto token $TOK >/dev/null; done
for n in 1 2 3 5; do uv $n ultar-verbund konto abgleichen >/dev/null; done
for n in 1 2 3 4 5; do dienst_start $n; done
sleep 4
uv 1 ultar-verbund geraete | tee $T/geraete.txt
grep -q "geraet5.*relais.*gekoppelt" $T/geraete.txt && grep -q "geraet2.*10.77.0.2.*gekoppelt" $T/geraete.txt \
  && ergebnis entdeckung OK "Heimnetz per Rundruf, geraet5 über Relais, Konto-Geräte vertraut" || ergebnis entdeckung FEHLER "$(cat $T/geraete.txt)"

echo "== 1 Grundlauf"
lauf grundlauf

echo "== 2 Steuern über Relais"
out=$(uv 1 ultar-verbund ausfuehren geraet5 -- sh -c 'echo $ULTAR_VERBUND_DIR' 2>&1)
[ "$out" = "$T/d5" ] && ergebnis steuern OK "Befehl lief auf geraet5 (anderes Netz) über Relais" || ergebnis steuern FEHLER "$out"

echo "== 3 Kopplung per Code (falscher Code, richtiger Code)"
(uv 1 env ULTAR_VERBUND_CODE=482913 ultar-verbund koppeln > $T/k.out 2>&1 &); sleep 0.5
a=$(uv 4 ultar-verbund koppeln geraet1 000000 2>&1); b=$(uv 4 ultar-verbund koppeln geraet1 482913 2>&1)
c=$(uv 4 ultar-verbund ausfuehren geraet1 -- echo gekoppelt 2>&1)
[[ "$a" == *"falscher Code"* && "$b" == *"gekoppelt mit"* && "$c" == gekoppelt ]] && ergebnis kopplung OK "falscher Code abgewiesen, richtiger gekoppelt" || ergebnis kopplung FEHLER "$a | $b | $c"

echo "== 4 Absturz (kill -9) von zwei Geräten mitten im Lauf"
( sleep 1; dienst_kill 2; sleep 1; dienst_kill 3 ) & BG=$!; lauf absturz; wait $BG

echo "== 5 Netztrennung (Kabel ab) für 4 s"
( sleep 1; ip link set v-uv2 down; ip link set v-uv3 down; sleep 4; ip link set v-uv2 up; ip link set v-uv3 up ) & BG=$!; lauf trennung; wait $BG
sleep 3

echo "== 6 Relais-Server stürzt ab und kommt wieder"
( sleep 1; server_stop; sleep 2; server_start >/dev/null ) & BG=$!; lauf relais_weg; wait $BG
sleep 4

echo "== 7 Gestörte Leitungen: Abbrüche, kaputte Bytes, Verzögerung, Abstürze in Aufgaben"
for n in 2 3 5; do dienst_stop $n; dienst_start $n "abbruch=0.02,kaputt=0.02,verzoegerung=25,absturz=0.03"; done
sleep 4
CLIENT_ENV=(ULTAR_VERBUND_CHAOS="abbruch=0.01,kaputt=0.01,verzoegerung=10")
lauf stoerung
CLIENT_ENV=(X=1)
for n in 2 3 5; do dienst_stop $n; dienst_start $n; done
sleep 4

echo "== 8 Nutzer schaltet ein Gerät mitten im Lauf AUS"
( sleep 1; uv 2 ultar-verbund aus >/dev/null ) & BG=$!; lauf ausschalten; wait $BG
sleep 11
if uv 1 ultar-verbund geraete | grep -q "geraet2.*10.77"; then ergebnis aus_unsichtbar FEHLER "geraet2 noch sichtbar"; else
  r=$(uv 1 ultar-verbund ausfuehren 10.77.0.2:47801 -- echo x 2>&1); ergebnis aus_unsichtbar OK "geraet2 unsichtbar, Verbindung abgelehnt (${r:0:40})"; fi
uv 2 ultar-verbund an >/dev/null; sleep 3

echo "== 9 Totalausfall aller anderen Geräte → dieses Gerät rechnet allein weiter"
( sleep 1; dienst_stop 2; dienst_stop 3; dienst_stop 5 ) & BG=$!; lauf totalausfall; wait $BG
for n in 2 3 5; do dienst_start $n; done; sleep 4

echo "== 10 Sicherheit"
uv 1 ultar-verbund entkoppeln geraet4 >/dev/null
r=$(uv 4 ultar-verbund ausfuehren geraet1 -- echo boese 2>&1)
[[ "$r" == *"nicht gekoppelt"* ]] && ergebnis entkoppelt OK "entkoppeltes Gerät abgewiesen" || ergebnis entkoppelt FEHLER "$r"
(uv 1 env ULTAR_VERBUND_CODE=135790 ultar-verbund koppeln > $T/k2.out 2>&1 &); sleep 0.5
for i in 1 2 3 4 5; do uv 4 ultar-verbund koppeln geraet1 99999$i >/dev/null 2>&1; done
r=$(uv 4 ultar-verbund koppeln geraet1 135790 2>&1)
[[ "$r" != *"gekoppelt mit"* ]] && ergebnis rateschutz OK "nach 5 Fehlversuchen ist der Code verworfen" || ergebnis rateschutz FEHLER "$r"
PK1=$(uv 1 sh -c 'od -An -tx1 -j32 -N32 $ULTAR_VERBUND_DIR/schluessel | tr -d " \n"')
code=$(ip netns exec uvsrv curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:8080/v1/verbund/geraet -H "authorization: Bearer $TOK2" -H 'content-type: application/json' -d "{\"pk\":\"$PK1\",\"name\":\"klau\"}")
[ "$code" = 409 ] && ergebnis fremdkonto OK "fremdes Konto kann Geräteschlüssel nicht übernehmen (409)" || ergebnis fremdkonto FEHLER "http $code"
code=$(ip netns exec uvsrv curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/v1/verbund/geraete -H "authorization: Bearer falsch.token")
[ "$code" = 401 ] && ergebnis ohne_konto OK "ohne gültiges Token keine Geräteliste (401)" || ergebnis ohne_konto FEHLER "http $code"
grep -q "verbund:" $T/srv/log && ! grep -aq "ultar-verbund/" $T/srv/log && ergebnis e2e OK "Server-Log enthält keine Klartext-Inhalte" || ergebnis e2e FEHLER "Klartext im Server-Log?"

echo "== 11 Leistung"
uv 1 ultar-verbund bench -n 48 -a 200000 > $T/bench.txt 2>&1 && ergebnis bench OK "$(grep Beschleunigung $T/bench.txt)" || ergebnis bench FEHLER "$(tail -2 $T/bench.txt)"
cat $T/bench.txt

echo; echo "== Ergebnis: $ok bestanden, $fehl fehlgeschlagen"; cat $BER
ip netns exec uvsrv curl -s http://127.0.0.1:8080/v1/verbund/statistik; echo
[ $fehl -eq 0 ]
