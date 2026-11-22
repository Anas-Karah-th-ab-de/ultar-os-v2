# gemeinsame Hilfen für die Verbund-Tests
R=${R:-/home/claude/ultar-os}
BIN=${BIN:-$R/target/release}
SRV=${SRV:-$R/server/ultraos-server/target/release/ultraos-server}
T=${T:-/tmp/uvtest}
export PATH=$BIN:$PATH
export ULTAR_VERBUND_LOG=1

server_start() {
  mkdir -p $T/srv
  ip netns exec uvsrv env ULTRAOS_SECRET=$(printf '%064d' 7) ULTRAOS_DB=$T/srv/db.sqlite ULTRAOS_BIND=0.0.0.0:8080 RUST_LOG=info \
    $SRV >>$T/srv/log 2>&1 &
  echo $! > $T/srv/pid
  for _ in $(seq 50); do ip netns exec uvsrv curl -sf http://127.0.0.1:8080/v1/health >/dev/null && return 0; sleep 0.1; done
  echo "server startet nicht"; return 1
}
server_stop() { kill -9 $(cat $T/srv/pid) 2>/dev/null; wait $(cat $T/srv/pid) 2>/dev/null; true; }

uv() { local n=$1; shift; ip netns exec uv$n env ULTAR_VERBUND_DIR=$T/d$n HOME=$T/d$n "$@"; }

# Dienst mit Aufpasser (wie ultar-init: Neustart nach Absturz)
dienst_start() { # n [chaos]
  local n=$1 ch=${2:-}
  mkdir -p $T/d$n
  ( while [ ! -f $T/d$n/stopp ]; do
      ip netns exec uv$n env ULTAR_VERBUND_DIR=$T/d$n ULTAR_VERBUND_CHAOS="$ch" ULTAR_VERBUND_LOG=1 $BIN/ultar-verbund dienst >>$T/d$n/log 2>&1 &
      echo $! > $T/d$n/pid; wait $!; sleep 0.5
    done ) &
  echo $! > $T/d$n/aufpasser
}
dienst_stop() {
  local a=$(cat $T/d$1/aufpasser 2>/dev/null)
  touch $T/d$1/stopp; kill -9 $(cat $T/d$1/pid) 2>/dev/null
  for _ in $(seq 50); do kill -0 $a 2>/dev/null || break; sleep 0.1; kill -9 $(cat $T/d$1/pid) 2>/dev/null; done
  rm -f $T/d$1/stopp
}
dienst_kill() { kill -9 $(cat $T/d$1/pid) 2>/dev/null; true; }

pruefe() { # datei n iter  → prüft genau n korrekte, eindeutige Ergebnisse
  python3 - "$@" <<'PY'
import sys, json, hashlib
f, n, it = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
def arbeit(i):
    h = hashlib.sha256(f"ultar-verbund/{i}".encode()).digest()
    for _ in range(it): h = hashlib.sha256(h).digest()
    return h[:8].hex()
import os
cache = os.path.join(os.path.dirname(f), f"erwartet-{n}-{it}.json")
try: erw = json.load(open(cache))
except Exception:
    erw = [arbeit(i) for i in range(n)]; json.dump(erw, open(cache, "w"))
rs = [json.loads(l) for l in open(f) if l.strip()]
ids = [r["id"] for r in rs]
assert len(rs) == n, f"{len(rs)} Ergebnisse statt {n}"
assert len(set(ids)) == n, "doppelte Ergebnisse"
falsch = [r for r in rs if r["code"] != 0 or r["ausgabe"].strip() != erw[int(r["id"].rsplit("-",1)[1])]]
assert not falsch, f"{len(falsch)} falsch, z.B. {falsch[0]}"
knoten = {}
for r in rs: knoten[r["knoten"]] = knoten.get(r["knoten"], 0) + 1
print("OK", n, "korrekt; je Gerät", knoten)
PY
}
