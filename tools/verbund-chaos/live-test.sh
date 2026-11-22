#!/bin/bash
# Ultar-Verbund gegen den echten Server (ultar.code-craft.tech): zwei Geräte, die sich NICHT im Netz sehen
# (eigene Ports), verbinden sich nur über das Relais. Legt ein Testkonto „verbundtest-…“ an.
#   tools/verbund-chaos/live-test.sh [https://ultar.code-craft.tech]
set -u
S=${1:-https://ultar.code-craft.tech}
WS=$(echo "$S" | sed 's#^https://#wss://#; s#^http://#ws://#')/v1/verbund/relais
R=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${BIN:-$R/target/release}
T=$(mktemp -d /tmp/uvlive.XXXX)
export PATH=$BIN:$PATH ULTAR_VERBUND_LOG=1
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }
g() { local n=$1; shift; env ULTAR_VERBUND_DIR=$T/$n HOME=$T/$n ULTAR_RUN=$T/run$n ULTAR_EMPFANG=$T/empfang$n "$@"; }
PIDS=()
trap 'kill ${PIDS[@]} 2>/dev/null; wait 2>/dev/null; echo "Testordner: $T"' EXIT

NAME=verbundtest-$(head -c4 /dev/urandom | od -An -tx1 | tr -d ' \n')
TOK=$(curl -s -m 20 -X POST $S/v1/account/register -H 'content-type: application/json' -d "{\"username\":\"$NAME\",\"password\":\"Test-$NAME-pw\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("token",""))')
c "Konto auf $S angelegt ($NAME)" "[ -n '$TOK' ]"
[ -n "$TOK" ] || exit 1

i=0
for n in a b; do
  i=$((i+1)); mkdir -p $T/$n
  g $n ultar-verbund an >/dev/null; g $n ultar-verbund name live-$n >/dev/null
  g $n ultar-verbund relais $WS >/dev/null
  # eigene Ports: die beiden sehen sich im Netz NICHT – nur das Relais verbindet sie
  g $n python3 -c "import json,os;p=os.environ['ULTAR_VERBUND_DIR']+'/verbund.json';c=json.load(open(p));c['udp_port']=4781$i;c['tcp_port']=4782$i;json.dump(c,open(p,'w'))"
  g $n ultar-verbund konto token $TOK >/dev/null
done
g a ultar-verbund konto abgleichen > $T/abgleich.txt 2>&1
g b ultar-verbund konto abgleichen >> $T/abgleich.txt 2>&1
c "Kontogeräte gegenseitig vertraut" "grep -q '1 weitere' $T/abgleich.txt"
for n in a b; do g $n ultar-verbund dienst > $T/$n.log 2>&1 & PIDS+=($!); done
for i in $(seq 40); do g a ultar-verbund geraete 2>/dev/null | grep -q "live-b.*relais.*gekoppelt" && break; sleep 0.5; done
g a ultar-verbund geraete > $T/geraete.txt 2>&1
c "live-b über das Relais sichtbar" "grep -q 'live-b.*relais.*gekoppelt' $T/geraete.txt"

t0=$(date +%s%N)
r=$(g a ultar-verbund ausfuehren live-b -- sh -c 'echo $ULTAR_VERBUND_DIR' 2>&1)
c "Steuern über das Relais ($(( ($(date +%s%N)-t0)/1000000 )) ms)" "[ '$r' = '$T/b' ]"

g a ultar-verbund verteilt -n 40 --ohne-lokal --json -- ultar-verbund arbeit {} 20000 > $T/v.json 2> $T/v.err
c "40 Aufgaben über das Relais, alle korrekt" "python3 - $T/v.json <<'PY'
import sys,json,hashlib
rs=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
def a(i):
    h=hashlib.sha256(f'ultar-verbund/{i}'.encode()).digest()
    for _ in range(20000): h=hashlib.sha256(h).digest()
    return h[:8].hex()
assert len(rs)==40 and len({r['id'] for r in rs})==40
assert all(r['code']==0 and r['ausgabe'].strip()==a(int(r['id'].rsplit('-',1)[1])) for r in rs)
PY"

head -c 300000 /dev/urandom > $T/datei.bin
g a ultar-verbund senden live-b $T/datei.bin --nicht-oeffnen > /dev/null 2>&1
c "300-KB-Datei über das Relais, unverändert" "cmp -s $T/datei.bin $T/empfangb/datei.bin"

# Chaos: Dienst B mitten im Lauf hart beenden und neu starten → Ergebnisse trotzdem vollständig (Ersatz hier)
( sleep 1.5; kill -9 ${PIDS[1]}; sleep 2; g b ultar-verbund dienst >> $T/b.log 2>&1 & echo $! > $T/b2.pid ) &
BG=$!
g a ultar-verbund verteilt -n 120 --json -- ultar-verbund arbeit {} 3000000 > $T/v2.json 2> $T/v2.err
wait $BG
PIDS+=($(cat $T/b2.pid 2>/dev/null))
c "Absturz von live-b mitten im Lauf: 120/120 Ergebnisse ($(grep -o 'wiederholt [0-9]*, doppelt gestartet [0-9]*' $T/v2.err))" "[ \$(grep -c '\"code\":0' $T/v2.json) -eq 120 ] && grep -Eq 'wiederholt [1-9]|doppelt gestartet [1-9]' $T/v2.err"

curl -s -m 10 $S/v1/verbund/statistik > $T/stat.json
c "Server-Statistik: Rahmen weitergeleitet ($(cat $T/stat.json))" "grep -q '\"rahmen\":[1-9]' $T/stat.json"

echo; echo "$ok bestanden, $fehl fehlgeschlagen (Server $S, Konto $NAME)"
[ $fehl -eq 0 ]
