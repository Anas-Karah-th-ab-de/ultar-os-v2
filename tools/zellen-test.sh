#!/bin/bash
# Ultar-Zellen prüfen (root, Linux mit Netz-Namensräumen und nftables).
# Aufbau: „Internet“-Server 10.200.0.2 (eigener Namensraum), VPN-Zelle mit eigenem „Tunnel“ (10.201.0.2 → Server),
# Zellen: offen (frei), geheim (nur über VPN, Protokoll + Uploads, keine Geräteinfo), a ↔ b verknüpft, c fremd.
#   sudo tools/zellen-test.sh
set -u
R=$(cd "$(dirname "$0")/.." && pwd)
Z=$R/target/release/zelle
T=/tmp/zellentest
export ULTAR_ZELLEN=$T/zellen ULTAR_DATA=$T/data ULTAR_ZELLEN_DNS=10.200.0.2 ULTAR_NETZ_ERLAUBT=$T/netz-erlaubt
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }
ab() {
  for n in offen geheim vpn a b c; do $Z stopp $n >/dev/null 2>&1; done
  for n in $(ip netns list | awk '{print $1}' | grep '^z-'); do ip netns pids $n | xargs -r kill -9; ip netns del $n; done
  ip link | grep -o 'zh[0-9a-f]*' | sort -u | xargs -r -n1 ip link del 2>/dev/null
  ip rule | grep -o 'lookup 1[0-9][0-9][0-9]' | while read _ t; do ip rule del table $t; done
  ip netns pids znetz 2>/dev/null | xargs -r kill -9; ip netns del znetz 2>/dev/null; ip link del vznetz 2>/dev/null
  nft delete table inet ultar_zellen 2>/dev/null; nft delete table inet ultar_geraet 2>/dev/null; true
}
ab; trap ab EXIT
rm -rf $T; mkdir -p $T/data/apps.d

# ---- „Internet“: Server mit Web (zeigt Absender, nimmt Uploads an) und DNS
ip netns add znetz
ip link add vznetz type veth peer name e0 netns znetz
ip addr add 10.200.0.1/24 dev vznetz; ip link set vznetz up
ip -n znetz addr add 10.200.0.2/24 dev e0; ip -n znetz link set e0 up; ip -n znetz link set lo up
ip -n znetz route add default via 10.200.0.1
cat > $T/server.py <<'PY'
import http.server, socketserver, threading, socket, struct
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        b = self.client_address[0].encode(); self.send_response(200); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0)); d = self.rfile.read(n)
        open("/tmp/zellentest/uploads.txt", "ab").write(self.client_address[0].encode() + b" " + d + b"\n")
        self.send_response(200); self.send_header("Content-Length", "2"); self.end_headers(); self.wfile.write(b"ok")
def dns():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(("10.200.0.2", 53))
    while True:
        q, a = s.recvfrom(512); i = 12
        while q[i]: i += q[i] + 1
        r = q[:2] + b"\x81\x80" + q[4:6] + b"\x00\x01\x00\x00\x00\x00" + q[12:i + 5] + b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + bytes([10, 200, 0, 2])
        s.sendto(r, a)
threading.Thread(target=dns, daemon=True).start()
socketserver.ThreadingTCPServer.allow_reuse_address = True
socketserver.ThreadingTCPServer(("10.200.0.2", 80), H).serve_forever()
PY
ip netns exec znetz python3 $T/server.py &
sleep 1

holen='import urllib.request,sys; print(urllib.request.urlopen(sys.argv[1], timeout=4).read().decode())'
senden='import urllib.request,sys; print(urllib.request.urlopen(urllib.request.Request(sys.argv[1], data=sys.argv[2].encode()), timeout=4).read().decode())'

# ---- Zellen anlegen
$Z neu offen -- sh -c > /dev/null
$Z neu vpn --vpn-zelle tun0 -- sleep 3600 > /dev/null
$Z neu geheim --netz ueber:vpn --uploads -- sh -c > /dev/null
$Z neu a -- sh -c > /dev/null; $Z neu b -- sh -c > /dev/null; $Z neu c -- sh -c > /dev/null
c "Zellen angelegt" "[ \$($Z liste | wc -l) -eq 6 ]"

echo "== Netz frei"
r=$($Z start offen -- "python3 -c '$holen' http://10.200.0.2/" 2>/dev/null)
c "freie Zelle erreicht das Netz (Absender $r = Gerät)" "[ '$r' = '10.200.0.1' ]"

echo "== VPN-Zelle mit Killswitch"
$Z start vpn > $T/vpn.log 2>&1 &
for i in $(seq 50); do grep -q läuft $T/vpn.log && break; sleep 0.2; done; sleep 0.3
# „Tunnel“ der VPN-Zelle: eigene Leitung direkt zum Server (wie wg0 zum VPN-Anbieter)
ip link add tun0 type veth peer name vpnseite netns znetz
ip link set tun0 netns z-vpn
ip -n z-vpn addr add 10.201.0.2/24 dev tun0; ip -n z-vpn link set tun0 up
ip -n znetz addr add 10.201.0.1/24 dev vpnseite; ip -n znetz link set vpnseite up
ip -n z-vpn route replace default via 10.201.0.1 dev tun0
skript="python3 -c '$holen' http://10.200.0.2/; python3 -c '$senden' http://10.200.0.2/hoch geheimer-text-123; python3 -c 'import socket; print(socket.gethostbyname(\"ziel.beispiel\"))'"
$Z start geheim -- "$skript" > $T/geheim.txt 2>/dev/null
c "Zelle hinter dem VPN geht nur über den Tunnel (Absender $(head -1 $T/geheim.txt))" "[ \"\$(head -1 $T/geheim.txt)\" = '10.201.0.2' ]"
c "Upload kam über den Tunnel an" "grep -q '^10.201.0.2 geheimer-text-123' $T/uploads.txt"
c "DNS über den Wächter" "grep -q '10.200.0.2' <(sed -n 3p $T/geheim.txt)"
$Z protokoll geheim --json > $T/prot.jsonl
c "Protokoll: Verbindung, Ziel und Bytes" "grep -q '\"ziel\":\"10.200.0.2:80\"' $T/prot.jsonl && grep -q '\"hoch\":[1-9]' $T/prot.jsonl"
c "Protokoll: Upload-Inhalt (Klartext) festgehalten" "grep -q 'POST /hoch' $T/prot.jsonl"
c "Protokoll: DNS-Name festgehalten" "grep -q '\"name\":\"ziel.beispiel\"' $T/prot.jsonl"
ip -n z-vpn link set tun0 down
$Z start geheim -- "python3 -c '$holen' http://10.200.0.2/" > $T/kill1.txt 2>&1
c "Killswitch: Tunnel weg → kein Verkehr, kein Umweg" "! grep -q '^10\.' $T/kill1.txt"
$Z stopp vpn > /dev/null
$Z start geheim -- "python3 -c '$holen' http://10.200.0.2/" > $T/kill2.txt 2>&1
c "Killswitch: VPN-Zelle gestoppt → weiterhin kein Verkehr" "! grep -q '^10\.' $T/kill2.txt"

echo "== Privatsphäre"
mkdir -p $T/zellen/c/daten && echo geheim-c > $T/zellen/c/daten/notiz
$Z start geheim -- 'grep -m1 "model name" /proc/cpuinfo; cat /etc/machine-id; hostname; id -u; ls /data; ls /dev | tr "\n" " "; echo; ls /sys/class/dmi/id 2>/dev/null | wc -l; cat /data/zelle/../../zellentest/zellen/c/daten/notiz 2>&1; ls /tmp | wc -l; cat /proc/1/cmdline | tr "\0" " "' > $T/privat.txt 2>/dev/null
c "neutraler Prozessor" "grep -q 'Ultar Prozessor' $T/privat.txt"
c "eigene Geräte-ID (nicht die des Geräts)" "! grep -qx \"\$(cat /etc/machine-id 2>/dev/null || echo x)\" $T/privat.txt && [ \$(sed -n 2p $T/privat.txt | wc -c) -eq 33 ]"
c "Rechnername „ultar“" "[ \"\$(sed -n 3p $T/privat.txt)\" = ultar ]"
c "eigene Benutzerkennung, kein root" "[ \$(sed -n 4p $T/privat.txt) -ge 60000 ]"
c "fremder Speicher unsichtbar (/data nur eigener)" "[ \"\$(sed -n 5p $T/privat.txt)\" = zelle ]"
c "keine Kamera/Ortung/Platten in /dev" "! sed -n 6p $T/privat.txt | grep -Eq 'video|ttyACM|sda|nvme|vda'"
c "keine DMI-Geräteinfo" "[ \"\$(sed -n 7p $T/privat.txt)\" = 0 ]"
c "Daten anderer Zellen unerreichbar" "! grep -q geheim-c $T/privat.txt"

echo "== Verknüpfen"
$Z verknuepfen a b > /dev/null
$Z start a -- 'echo von-a > /data/gemeinsam/b/nachricht' > /dev/null 2>&1
r=$($Z start b -- 'cat /data/gemeinsam/a/nachricht' 2>/dev/null)
c "gemeinsamer Ordner a ↔ b" "[ '$r' = von-a ]"
$Z start b -- 'cd /data/zelle && echo hallo-b > i.html && timeout 12 python3 -m http.server 8000 >/dev/null 2>&1' > /dev/null 2>&1 & BPID=$!
sleep 3
r=$($Z start a -- "python3 -c '$holen' http://b:8000/i.html" 2>/dev/null)
c "a ruft b beim Namen auf" "[ '$r' = hallo-b ]"
r=$($Z start c -- "python3 -c '$holen' http://$(ip -n z-b -4 addr show e0 | grep -o '10\.88\.[0-9.]*' | head -1):8000/i.html" 2>&1)
c "nicht verknüpfte Zelle c erreicht b nicht" "! echo '$r' | grep -q hallo-b"
wait $BPID

echo "== Speicher dynamisch"
$Z start c -- 'head -c 30000000 /dev/urandom > /data/zelle/gross.bin' > /dev/null 2>&1
mb=$($Z liste --json | python3 -c "import json,sys;print([z['belegt_mb'] for z in json.load(sys.stdin) if z['name']=='c'][0])")
c "Zelle wächst nach Bedarf ($mb MB belegt, keine feste Größe)" "[ $mb -ge 28 ]"

echo "== Grafikkarten"
S=$T/sys; for i in 0 1; do d=$S/class/drm/card$i/device; mkdir -p $d/drm/renderD12$((8+i)); echo 0x1002 > $d/vendor; done
echo $((16<<30)) > $S/class/drm/card0/device/mem_info_vram_total; echo $((15<<30)) > $S/class/drm/card0/device/mem_info_vram_used
echo $((8<<30)) > $S/class/drm/card1/device/mem_info_vram_total; echo $((1<<30)) > $S/class/drm/card1/device/mem_info_vram_used
r=$(ULTAR_SYS=$S $Z gpu --json)
c "zwei Karten erkannt, auto wählt die freiere (Karte 1)" "echo '$r' | python3 -c 'import json,sys;d=json.load(sys.stdin);assert len(d[\"karten\"])==2 and d[\"wahl\"][\"karten\"]==[1]'"
$Z gpu modus alle > /dev/null; r=$(ULTAR_SYS=$S $Z gpu --json); $Z gpu modus auto > /dev/null
c "Modus „alle“: beide Karten, Aufteilung nach Speicher 16:8" "echo '$r' | grep -q '\"LLAMA_ARG_TENSOR_SPLIT\",\"16,8\"'"

echo "== Empfehlung statt doppelt"
printf 'id = "firefox"\nname = "Firefox"\nformat = "deb"\npackage = "firefox"\nsource = "firefox.deb"\nrun = ["firefox"]\n' > $T/data/apps.d/firefox.toml
r=$($Z empfehlung "ChromeSetup.exe" 2>&1)
c "zweiter Browser: Empfehlung, vorhandenen in neuer Zelle zu nutzen" "echo '$r' | grep -q 'zelle neu <name> --app firefox'"

echo "== Kiri (Docker-API) – Verknüpfen von Containern"
cat > $T/api.py <<'PY'
import socket, os, json, threading
p = "/tmp/zellentest/docker.sock"
try: os.unlink(p)
except OSError: pass
s = socket.socket(socket.AF_UNIX); s.bind(p); s.listen(8)
netze, aufrufe = [], []
def antw(c, code, obj):
    b = json.dumps(obj).encode(); c.sendall(b"HTTP/1.1 %d OK\r\nContent-Type: application/json\r\nContent-Length: %d\r\n\r\n" % (code, len(b)) + b)
while True:
    c, _ = s.accept(); d = b""
    while b"\r\n\r\n" not in d: d += c.recv(4096)
    kopf, rest = d.split(b"\r\n\r\n", 1); z = kopf.decode().split("\r\n")[0]; m, pfad = z.split()[:2]
    n = int([l.split(":")[1] for l in kopf.decode().split("\r\n") if l.lower().startswith("content-length")][0])
    while len(rest) < n: rest += c.recv(4096)
    aufrufe.append(m + " " + pfad); open("/tmp/zellentest/api.log", "a").write(m + " " + pfad + " " + rest.decode() + "\n")
    if pfad.startswith("/containers/json"): antw(c, 200, [{"Id": "abc123def4567890", "Names": ["/datenbank"], "Image": "postgres:16", "State": "running"}, {"Id": "fff", "Names": ["/web"], "Image": "nginx:1", "State": "running"}])
    elif pfad == "/networks": antw(c, 200, [{"Name": n} for n in netze])
    elif pfad == "/networks/create": netze.append(json.loads(rest)["Name"]); antw(c, 201, {"Id": "n1"})
    elif "/connect" in pfad or pfad == "/volumes/create": antw(c, 200, {})
    elif pfad == "/version": antw(c, 200, {"Version": "0.2.1", "Platform": {"Name": "Kiri"}})
    else: antw(c, 404, {"message": "?"})
    c.close()
PY
python3 $T/api.py & API=$!
sleep 0.5
export ULTAR_KIRI_SOCKET=$T/docker.sock
r=$($Z empfehlung "postgres:17-alpine" 2>&1)
c "Container: zweite Datenbank → vorhandenen „datenbank“ verknüpfen" "echo '$r' | grep -q 'verknuepfen-container <neu> datenbank'"
r=$($Z verknuepfen-container web datenbank 2>&1)
c "Container verknüpft (gemeinsames Netz + Volume)" "grep -q 'POST /networks/create' $T/api.log && [ \$(grep -c '/connect' $T/api.log) -eq 2 ] && grep -q 'POST /volumes/create' $T/api.log"
r=$($Z kiri status 2>&1); kill $API 2>/dev/null
c "Kiri-Status über die Docker-API" "echo '$r' | grep -q 'Kiri 0.2.1'"

echo "== Systemweites Protokoll"
python3 -c "import socket,time;s=socket.create_connection(('10.200.0.2',80));time.sleep(4)" & SPID=$!
sleep 1; $Z systemprotokoll --runden 1 > /dev/null 2>&1
c "systemweit: Verbindung mit Programmname protokolliert" "grep -q '\"ziel\":\"10.200.0.2:80\"' $T/zellen/system-protokoll.jsonl && grep -q 'python3' $T/zellen/system-protokoll.jsonl"
wait $SPID

echo "== Internet-Regeln: Gerät offline, nur erlaubte Zellen/Dienste online"
$Z netz geraet aus > /dev/null
hol() { timeout 6 python3 -c "$holen" http://10.200.0.2/ 2>/dev/null; }
c "Gerät selbst offline (z. B. Paketquellen)" "[ -z \"\$(hol)\" ]"
r=$($Z start offen -- "python3 -c '$holen' http://10.200.0.2/" 2>/dev/null)
c "erlaubte Zelle weiter online (Weiterleitung läuft)" "[ '$r' = '10.200.0.1' ]"
r=$(python3 -c "import os; os.setgid(47900); exec(open('/dev/stdin').read())" <<<"import urllib.request; print(urllib.request.urlopen('http://10.200.0.2/', timeout=4).read().decode())" 2>/dev/null)
c "erlaubter Systemdienst (Netz-Gruppe) online" "[ '$r' = '10.200.0.1' ]"
r=$(python3 -c "import os,subprocess; os.setgid(47900); print(subprocess.run(['python3','-c','import os; os.setgid(os.getuid()); import urllib.request; print(urllib.request.urlopen(\"http://10.200.0.2/\", timeout=4).read().decode())'],capture_output=True,text=True).stdout)" 2>/dev/null)
c "dessen Kindbefehle (Gruppe abgegeben) offline – z. B. keine Paket-Installation per Fernsteuerung" "[ -z \"\$(echo $r)\" ]"
$Z netz erlauben verbund > /dev/null
c "Erlaubt-Liste für ultar-init geschrieben" "grep -qx verbund $T/netz-erlaubt"
$Z netz vorgabe aus > /dev/null; $Z neu neue -- sh -c > /dev/null
c "Vorgabe „aus“: neue Zelle ohne Netz" "$Z zeige neue | grep -q 'art = \"aus\"'"
$Z netz geraet an > /dev/null; $Z netz vorgabe frei > /dev/null
c "Gerät wieder online" "[ \"\$(hol)\" = 10.200.0.1 ]"

echo "== Vorgegebene Identität (Ort, Zeitzone, Sprache)"
mkdir -p /run/ultar
$Z neu ident --ort 33.5138,36.2765 --zeitzone Asia/Damascus --sprache ar -- sh -c > /dev/null
$Z start ident -- 'date +%z; echo $LANG; cat /run/ultar/gnss.nmea | head -1; grep -c 33.5138 /data/zelle/.mozilla/firefox/ultar.default/user.js' > $T/ident.txt 2>/dev/null
c "Zeitzone Damaskus (+0300)" "[ \"\$(sed -n 1p $T/ident.txt)\" = +0300 ]"
c "Sprache arabisch" "sed -n 2p $T/ident.txt | grep -q '^ar_'"
c "Ortung liefert nur den vorgegebenen Ort" "sed -n 3p $T/ident.txt | grep -q 'GPGGA,120000.00,3330.828[0-9],N,03616.59'"
c "Browser (Firefox) bekommt den vorgegebenen Ort" "[ \"\$(sed -n 4p $T/ident.txt)\" = 1 ]"

echo "== Sprache der Befehlszeile"
c "zelle auf Arabisch" "LANG=ar_SY.UTF-8 $Z netz | grep -q 'إنترنت الجهاز نفسه'"
c "arabische Befehlswörter" "LANG=ar $Z قائمة | grep -q ident"
c "zelle auf Englisch" "LANG=en $Z netz | grep -q 'Internet for the device itself: on'"

echo; echo "$ok bestanden, $fehl fehlgeschlagen"
[ $fehl -eq 0 ]
