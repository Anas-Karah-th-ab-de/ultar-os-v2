#!/bin/bash
# Ultar-Agent: Sprachmodell auf dem Gerät einrichten (läuft im Ubuntu-Bereich über ultar-compat).
#   agent-install.sh <Modell-Adresse | Pfad zu einer .gguf-Datei> [--rebuild | --model-only]
#   --model-only: nur das Modell holen – der Rechenkern ist der native Dienst ultar-llm (kein llama.cpp im Ubuntu-Bereich nötig)
#
# Ergebnis unter /data/ultar/agent:
#   llama-server   Startskript (setzt den Bibliothekspfad und ruft das eigentliche Programm auf)
#   model.gguf     das Sprachmodell
# Der Dienst „llm" (compat = true) startet beides; die Oberfläche spricht nur 127.0.0.1:8080 an.
#
# Reihenfolge – der erste Weg, der klappt, gewinnt:
#   1. OFFLINE: USB-Stick/Ordner mit „ultar-agent/" (llama-server bzw. llama-*.tar.gz und *.gguf)
#   2. fertiges llama.cpp aus den offiziellen Veröffentlichungen (GitHub ggml-org/llama.cpp)
#   3. llama.cpp auf dem Gerät selbst bauen (langsam, läuft aber auf jedem Prozessor)
# Schritt 2 wird geprüft: Stürzt das fertige Programm auf diesem Prozessor ab (ältere ARM-Kerne ohne
# die erwarteten Befehlssätze), folgt automatisch Schritt 3.
set -u
MODEL_SRC="${1:-}"
REBUILD="${2:-}"
DIR=/data/ultar/agent
BIN="$DIR/llama"
export DEBIAN_FRONTEND=noninteractive
mkdir -p "$DIR" "$BIN" || { echo "FEHLER: $DIR nicht beschreibbar"; exit 1; }

say() { echo "==> $*"; }
works() { [ -x "$BIN/llama-server" ] && LD_LIBRARY_PATH="$BIN" "$BIN/llama-server" --version >/dev/null 2>&1; }

need() { # fehlende Pakete nachinstallieren
  local miss=""
  for p in "$@"; do dpkg -s "$p" >/dev/null 2>&1 || miss="$miss $p"; done
  [ -z "$miss" ] && return 0
  say "Installiere:$miss"
  apt-get update -qq || true
  apt-get install -y --no-install-recommends $miss
}

case "$(uname -m)" in
  aarch64|arm64) ARCH=arm64 ;;
  x86_64) ARCH=x64 ;;
  *) ARCH="" ;;
esac

# ---------- 1. Offline (USB) ----------
OFF=""
for d in /media/*/ultar-agent /media/ultar-agent /data/ultar-agent /data/Download/ultar-agent; do
  [ -d "$d" ] && { OFF="$d"; break; }
done
if [ -n "$OFF" ]; then
  say "Offline-Quelle gefunden: $OFF"
  if [ -f "$OFF/llama-server" ]; then
    cp -f "$OFF"/llama-server "$BIN/" && chmod +x "$BIN/llama-server"
    cp -f "$OFF"/*.so* "$BIN/" 2>/dev/null || true
  else
    for t in "$OFF"/llama-*.tar.gz; do
      [ -f "$t" ] || continue
      tar -xzf "$t" -C "$BIN" --strip-components=1 2>/dev/null || tar -xzf "$t" -C "$BIN"
      break
    done
  fi
  [ -z "$MODEL_SRC" ] && MODEL_SRC="$(ls "$OFF"/*.gguf 2>/dev/null | head -n1)"
fi

if [ "$REBUILD" = "--model-only" ]; then
  say "Rechenkern: nativer Dienst ultar-llm – lade nur das Modell"
else
# ---------- 2. Fertiges Programm ----------
if [ "$REBUILD" != "--rebuild" ] && ! works && [ -n "$ARCH" ]; then
  need curl ca-certificates libgomp1
  say "Suche aktuelle llama.cpp-Veröffentlichung ($ARCH)"
  URL="$(curl -fsSL https://api.github.com/repos/ggml-org/llama.cpp/releases/latest \
        | grep -o "https://[^\"]*bin-ubuntu-$ARCH\.tar\.gz" | head -n1)"
  if [ -n "$URL" ]; then
    say "Lade $URL"
    if curl -fL --retry 3 -o /tmp/llama.tar.gz "$URL"; then
      rm -rf "$BIN"/*
      tar -xzf /tmp/llama.tar.gz -C "$BIN" --strip-components=1 2>/dev/null || tar -xzf /tmp/llama.tar.gz -C "$BIN"
      # je nach Version liegen die Programme in einem Unterordner
      S="$(find "$BIN" -name llama-server -type f | head -n1)"
      if [ -n "$S" ] && [ "$(dirname "$S")" != "$BIN" ]; then cp -af "$(dirname "$S")"/. "$BIN"/; fi
      chmod +x "$BIN/llama-server" 2>/dev/null
    fi
    rm -f /tmp/llama.tar.gz
  else
    say "Keine fertige Fassung gefunden"
  fi
fi

# ---------- 3. Selbst bauen ----------
if ! works; then
  say "Fertiges Programm läuft auf diesem Prozessor nicht – baue llama.cpp auf dem Gerät (dauert)"
  need build-essential cmake git ca-certificates libgomp1 || { echo "FEHLER: Bauwerkzeuge nicht installierbar"; exit 1; }
  SRC=/var/tmp/llama.cpp
  rm -rf "$SRC"
  git clone --depth 1 https://github.com/ggml-org/llama.cpp "$SRC" || { echo "FEHLER: Quelltext nicht ladbar"; exit 1; }
  # GGML_NATIVE=OFF: nur Befehle, die jeder 64-Bit-Prozessor dieser Familie kann
  cmake -S "$SRC" -B "$SRC/build" -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DBUILD_SHARED_LIBS=OFF \
        -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=ON \
    && cmake --build "$SRC/build" --target llama-server -j"$(nproc)" \
    || { echo "FEHLER: Bau fehlgeschlagen"; exit 1; }
  rm -rf "$BIN"/*
  cp -f "$SRC/build/bin/llama-server" "$BIN/" && rm -rf "$SRC"
fi
works || { echo "FEHLER: llama-server läuft nicht"; exit 1; }
say "llama.cpp bereit: $(LD_LIBRARY_PATH="$BIN" "$BIN/llama-server" --version 2>&1 | head -n1)"

# Startskript (der Dienst ruft immer diesen Pfad auf)
cat > "$DIR/llama-server" <<'EOF'
#!/bin/sh
D=/data/ultar/agent/llama
export LD_LIBRARY_PATH="$D${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# Kerne: höchstens 4 (auf Handys die schnellen Kerne), sonst wird das Gerät träge und heiß
T=$(nproc 2>/dev/null || echo 4); [ "$T" -gt 4 ] && T=4
exec "$D/llama-server" --threads "$T" "$@"
EOF
chmod +x "$DIR/llama-server"
fi

# ---------- Modell ----------
if [ -n "$MODEL_SRC" ]; then
  if [ -f "$MODEL_SRC" ]; then
    say "Kopiere Modell $MODEL_SRC"
    cp -f "$MODEL_SRC" "$DIR/model.gguf.part" && mv -f "$DIR/model.gguf.part" "$DIR/model.gguf"
  else
    need curl ca-certificates
    say "Lade Modell $MODEL_SRC"
    # -C -: unterbrochene Übertragung fortsetzen
    curl -fL --retry 5 --retry-delay 3 -C - -o "$DIR/model.gguf.part" "$MODEL_SRC" \
      && mv -f "$DIR/model.gguf.part" "$DIR/model.gguf" \
      || { echo "FEHLER: Modell nicht geladen (erneut starten setzt fort)"; exit 1; }
  fi
fi
[ -s "$DIR/model.gguf" ] || { echo "FEHLER: kein Modell vorhanden"; exit 1; }
# GGUF-Kennung prüfen, damit keine Fehlerseite als Modell gilt
[ "$(head -c 4 "$DIR/model.gguf")" = "GGUF" ] || { echo "FEHLER: model.gguf ist keine GGUF-Datei"; rm -f "$DIR/model.gguf"; exit 1; }
say "Fertig: $(du -h "$DIR/model.gguf" | cut -f1) Modell, Agent kann gestartet werden"
