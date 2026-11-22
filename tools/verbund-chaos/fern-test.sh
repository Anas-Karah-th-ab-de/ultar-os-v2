#!/bin/bash
# Fernbedienung, Weitergeben, Ton und Mining im Ultar-Verbund – mit Störungen.
#   geraet1 = Handy/PC mit Bediener, geraet2 = Gerät OHNE Bildschirm (Server/Platine), geraet3 = fremd
# Prüft: virtueller Bildschirm startet von selbst, Bildschirmfoto, Steuerung (öffnen/zurück/drehen),
# Browser-Ansicht (Schlüssel, Kacheln, Eingaben), Datei weitergeben + dort öffnen, Mithören/Durchsage,
# Helfer-Weiterleitung, Mining nur im Leerlauf (stoppt bei Bedienung und bei Verbund-Aufgaben),
# Rechte (fremdes Gerät, Teilen aus) und Chaos (Oberfläche abgeschossen, Netz getrennt).
#   sudo tools/verbund-chaos/fern-test.sh
set -u
D=$(cd "$(dirname "$0")" && pwd)
source $D/lib.sh
T=/tmp/uvfern; rm -rf $T; mkdir -p $T
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }
e() { local n=$1; shift; ip netns exec uv$n env ULTAR_VERBUND_DIR=$T/d$n HOME=$T/d$n ULTAR_RUN=$T/run$n ULTAR_DATA=$T/data$n ULTAR_DRY=1 \
      ULTAR_SHELL=$BIN/ultar-shell ULTAR_VIRTUELL=800x600 ULTAR_EMPFANG=$T/empfang$n ULTAR_VERBUND_LOG=1 "$@"; }
start() { # n [extra env …]
  local n=$1; shift
  mkdir -p $T/d$n $T/run$n $T/data$n
  ( while [ ! -f $T/d$n/stopp ]; do e $n env "$@" $BIN/ultar-verbund dienst >> $T/d$n/log 2>&1; sleep 0.5; done ) &
}
aufraeumen() { touch $T/d1/stopp $T/d2/stopp $T/d3/stopp 2>/dev/null; $D/netz.sh ab; pkill -f "ultar-shell --virtuell" 2>/dev/null; true; }
trap aufraeumen EXIT
$D/netz.sh ab; $D/netz.sh auf

# Testdaten: 1 s Ton (16 kHz Mono s16le), Fake-Miner
python3 -c "
import math,struct
open('$T/ton.raw','wb').write(b''.join(struct.pack('<h',int(8000*math.sin(i*440*6.2832/16000))) for i in range(16000)))"
printf '#!/bin/sh\necho $$ > %s/miner.pid\nexec sleep 3600\n' "$T" > $T/fakeminer; chmod +x $T/fakeminer

for n in 1 2 3; do mkdir -p $T/d$n; e $n ultar-verbund an >/dev/null; e $n ultar-verbund name geraet$n >/dev/null; done
start 1 ULTAR_TON_AUFNAHME="cat $T/ton.raw" ULTAR_TON_WIEDERGABE="tee $T/hier-gehoert.raw"
start 2 ULTAR_TON_AUFNAHME="cat $T/ton.raw" ULTAR_TON_WIEDERGABE="tee $T/durchsage.raw" ULTAR_MINER=$T/fakeminer
start 3
sleep 3
(e 2 env ULTAR_VERBUND_CODE=135246 ultar-verbund koppeln > $T/k.out 2>&1 &); sleep 0.7
e 1 ultar-verbund koppeln geraet2 135246 > /dev/null 2>&1
sleep 2

echo "== Bildschirm ohne Bildschirm"
e 1 ultar-verbund foto geraet2 $T/a.bmp > $T/foto.txt 2>&1
c "Gerät ohne Bildschirm: virtueller Bildschirm startet, Foto 800×600" "grep -q '800×600' $T/foto.txt && [ \$(stat -c%s $T/a.bmp) -gt 1000000 ]"
c "virtueller Bildschirm im Protokoll" "grep -q 'virtueller Bildschirm gestartet' $T/d2/log"
bunt() { python3 -c "
import sys;b=open('$1','rb').read()[54:];print(len(set(b[i:i+3] for i in range(0,len(b),3*97))))"; }
c "Bild hat Inhalt (nicht leer)" "[ \$(bunt $T/a.bmp) -gt 5 ]"

e 1 ultar-verbund bildschirm geraet2 oeffne settings > $T/akt.txt 2>&1; sleep 1
e 1 ultar-verbund foto geraet2 $T/b.bmp > /dev/null 2>&1
c "Steuern: Einstellungen öffnen ändert das Bild" "! cmp -s $T/a.bmp $T/b.bmp"
e 1 ultar-verbund fern geraet2 zurueck > /dev/null 2>&1; sleep 1
e 1 ultar-verbund foto geraet2 $T/c.bmp > /dev/null 2>&1
c "Eingabe: Zurück-Taste aus der Ferne" "! cmp -s $T/b.bmp $T/c.bmp"
e 1 ultar-verbund bildschirm geraet2 drehen 90 > /dev/null 2>&1; sleep 1
e 1 ultar-verbund foto geraet2 $T/d.bmp > $T/foto2.txt 2>&1
c "Anzeige-Steuerung: Drehen 90° (600×800)" "grep -q '600×800' $T/foto2.txt"
e 1 ultar-verbund bildschirm geraet2 drehen 0 > /dev/null 2>&1
r=$(e 1 ultar-verbund bildschirm geraet2 'rm -rf /' 2>&1)
c "fremde Befehle als Aktion abgelehnt" "[[ \"$r\" == *'unbekannte Aktion'* ]]"
r=$(e 1 ultar-verbund bildschirm geraet2 helfer wie spät ist es 2>&1)
c "Helfer-Anweisung weitergeleitet" "[[ \"$r\" == *'ausgeführt: helfer'* ]]"

echo "== Browser-Ansicht"
(e 1 ultar-verbund anzeigen geraet2 --web 127.0.0.1:47881 > $T/url.txt 2> $T/anz.err &); sleep 2
URL=$(head -1 $T/url.txt); TOK=${URL##*t=}
cat > $T/web.py <<'PY'
import sys, urllib.request, struct
basis, tok = sys.argv[1], sys.argv[2]
def post(p, body=b""):
    r = urllib.request.urlopen(urllib.request.Request(basis + p, data=body, method="POST"), timeout=30); return r.status, r.read()
def kacheln(b):
    w, h, nr = struct.unpack("<IIQ", b[:16]); p = 17; n = 0
    while p + 12 <= len(b):
        x, y, kw, kh, l = struct.unpack("<HHHHI", b[p:p+12]); p += 12 + l; n += 1
        assert x + kw <= w and y + kh <= h
    assert p == len(b); return w, h, nr, n
s, html = urllib.request.urlopen(basis + "/?t=" + tok, timeout=10).status, urllib.request.urlopen(basis + "/?t=" + tok).read()
assert b"canvas" in html
try:
    urllib.request.urlopen(basis + "/?t=falsch", timeout=10); print("FEHLER kein Schutz"); sys.exit(1)
except urllib.error.HTTPError as e: assert e.code == 403
_, b = post("/bild?t=%s&voll=1" % tok); w, h, nr, n = kacheln(b)
assert (w, h) == (800, 600) and n == 13 * 10, (w, h, n)
_, b = post("/bild?t=%s&voll=0" % tok, b"oeffne settings\n"); # Steuerzeile über Eingaben wird gefiltert? nein: oeffne ist erlaubt
_, b2 = post("/bild?t=%s&voll=0" % tok, b"zurueck\n");
_, b3 = post("/bild?t=%s&voll=0" % tok, b"ab 0.5 0.5\nauf\n")
s, t = post("/aktion?t=%s" % tok, b"hell 60"); assert s == 200, t
s, t = post("/datei?t=%s&name=..%%2F..%%2Fboese.txt" % tok, b"hallo verbund"); assert s == 200 and b"/boese.txt" in t and b".." not in t, t
print("WEB OK", w, h, n)
PY
ip netns exec uv1 python3 $T/web.py http://127.0.0.1:47881 $TOK > $T/web.txt 2>&1
c "Browser-Ansicht: Schlüssel, ganze Kacheln, Eingaben, Datei, Aktion" "grep -q 'WEB OK' $T/web.txt"
c "Datei aus dem Browser landet sicher im Empfang" "[ \"\$(cat $T/empfang2/boese.txt 2>/dev/null)\" = 'hallo verbund' ]"

echo "== Weitergeben"
cp $T/a.bmp "$T/Urlaub Bild.bmp"
e 1 ultar-verbund weitergeben geraet2 "$T/Urlaub Bild.bmp" > $T/wg.txt 2>&1; sleep 1
c "Bild weitergegeben, gleich, dort geöffnet" "cmp -s '$T/Urlaub Bild.bmp' '$T/empfang2/Urlaub Bild.bmp' && grep -q 'datei von geraet1' $T/d2/log"
e 1 ultar-verbund foto geraet2 $T/e.bmp > /dev/null 2>&1
c "Ziel zeigt das empfangene Bild an" "! cmp -s $T/c.bmp $T/e.bmp"
e 1 ultar-verbund weitergeben geraet2 "$T/Urlaub Bild.bmp" > /dev/null 2>&1
c "zweite Übergabe überschreibt nichts" "[ -f '$T/empfang2/Urlaub Bild (2).bmp' ]"
r=$(e 2 ultar-verbund weitergeben geraet1 2>&1)
c "App weitergeben: Ziel ohne Oberfläche meldet es sauber" "[[ \"$r\" == *'keine Oberfläche'* ]]"
# umgekehrt: geraet1 gibt „diese App“ an geraet2 (virtuelle Oberfläche) – geraet2 öffnet die Ansicht von geraet1
r=$(e 1 ultar-verbund weitergeben geraet2 2>&1); sleep 4
c "App weitergeben: Ziel öffnet die Ansicht, Quelle teilt ihren Bildschirm" "[[ \"$r\" == *'zeige geraet1'* ]] && grep -q 'bildschirm geteilt mit geraet2' $T/d1/log"

echo "== Ton"
e 1 env ULTAR_TON_WIEDERGABE="tee $T/hier-gehoert.raw" ultar-verbund hoeren geraet2 3 > /dev/null 2> $T/hoer.err
c "Mithören: Mikrofon von geraet2 kommt hier unverändert an" "cmp -s $T/ton.raw $T/hier-gehoert.raw"
e 1 env ULTAR_TON_AUFNAHME="cat $T/ton.raw" ultar-verbund sprechen geraet2 3 > /dev/null 2> $T/sprech.err; sleep 1
c "Durchsage: hier gesprochen, dort abgespielt" "cmp -s $T/ton.raw $T/durchsage.raw"

echo "== Mining im Leerlauf"
W=44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A
r=$(e 1 ultar-verbund mining waehrung eth 2>&1)
c "Ethereum ehrlich abgelehnt (Proof-of-Stake)" "[[ \"$r\" == *'Proof-of-Stake'* ]]"
e 1 ultar-verbund mining wallet $W > /dev/null; e 1 ultar-verbund mining leerlauf 0 >/dev/null
e 1 ultar-verbund mining an > /dev/null
e 1 ultar-verbund mining alle --ohne-einrichten > $T/mining-alle.txt 2>&1
c "ein Klick: Einstellungen auf allen Geräten" "grep -q geraet2 $T/mining-alle.txt && grep -q $W $T/d2/mining.json"
for i in $(seq 20); do [ -f $T/miner.pid ] && kill -0 $(cat $T/miner.pid) 2>/dev/null && break; sleep 0.5; done
c "Miner startet, wenn das Gerät ruht" "[ -f $T/miner.pid ] && kill -0 \$(cat $T/miner.pid) 2>/dev/null"
# Leerlaufzeit wieder auf 5 min; Bedienung aus der Ferne zählt als Benutzung
e 2 python3 -c "import json;p='$T/d2/mining.json';k=json.load(open(p));k['leerlauf_min']=5;json.dump(k,open(p,'w'))"
e 1 ultar-verbund fern geraet2 'ab 0.5 0.5' auf > /dev/null 2>&1
for i in $(seq 24); do kill -0 $(cat $T/miner.pid) 2>/dev/null || break; sleep 0.5; done
c "Bedienung stoppt den Miner sofort" "! kill -0 \$(cat $T/miner.pid) 2>/dev/null"
e 2 python3 -c "import json;p='$T/d2/mining.json';k=json.load(open(p));k['leerlauf_min']=0;json.dump(k,open(p,'w'))"
rm -f $T/miner.pid
for i in $(seq 20); do [ -f $T/miner.pid ] && break; sleep 0.5; done
e 1 ultar-verbund verteilt -n 4 --ohne-lokal -- sleep 3 > /dev/null 2>&1 &
VP=$!; sleep 1.5
c "Verbund-Aufgabe hat Vorrang vor dem Mining" "! kill -0 \$(cat $T/miner.pid 2>/dev/null || echo 0) 2>/dev/null"
wait $VP

echo "== Rechte und Chaos"
r=$(e 3 ultar-verbund foto 10.77.0.2:47801 $T/x.bmp 2>&1)
c "fremdes Gerät sieht den Bildschirm nicht" "[[ \"$r\" == *'nicht gekoppelt'* ]] && [ ! -f $T/x.bmp ]"
e 2 ultar-verbund erlauben bildschirm aus > /dev/null
r=$(e 1 ultar-verbund foto geraet2 $T/y.bmp 2>&1)
c "Teilen ausgeschaltet → abgelehnt" "[[ \"$r\" == *'nicht erlaubt'* ]]"
e 2 ultar-verbund erlauben bildschirm an > /dev/null
pkill -9 -f "ultar-shell --virtuell"; sleep 1
e 1 ultar-verbund foto geraet2 $T/f.bmp > $T/foto3.txt 2>&1
c "Oberfläche abgeschossen → startet beim nächsten Zugriff neu" "grep -q '800×600' $T/foto3.txt"
( sleep 1; ip link set v-uv2 down; sleep 4; ip link set v-uv2 up ) &
sleep 2.5
cat > $T/web2.py <<'PY'
import sys, urllib.request, time
basis, tok = sys.argv[1], sys.argv[2]; t = time.time(); ok = False
while time.time() - t < 40:
    try:
        r = urllib.request.urlopen(urllib.request.Request(basis + "/bild?t=%s&voll=1" % tok, data=b"", method="POST"), timeout=15)
        if r.status == 200 and len(r.read()) > 1000: ok = True; break
    except Exception as e: time.sleep(1)
print("WIEDER" if ok else "WEG", round(time.time() - t, 1))
PY
ip netns exec uv1 python3 $T/web2.py http://127.0.0.1:47881 $TOK > $T/web2.txt 2>&1
c "Netz getrennt: Browser-Ansicht verbindet sich selbst neu ($(cat $T/web2.txt))" "grep -q WIEDER $T/web2.txt"

echo; echo "$ok bestanden, $fehl fehlgeschlagen"
[ $fehl -eq 0 ]
