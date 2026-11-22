# ultar-llm – Sprachmodell im Rust-Kern (Handy und Desktop)

KM Code Craft · Stufe 2 aus `claude/LLM-RUST.md`: das lokale Sprachmodell läuft als eigener Dienst von Ultar OS,
**ohne Ubuntu-Bereich, ohne Python, ohne dynamische Bibliotheken**. llama.cpp ist statisch eingebunden
(`llama-cpp-2`, inkl. libstdc++); ein Programm, das auf dem Galaxy A30 genauso läuft wie auf einem PC.

| | |
|---|---|
| Crate | `crates/ultar-llm` (Bibliothek `ultar_llm` + Programm `ultar-llm`) |
| Dienst | `/etc/ultar/services/llm.toml` → `/usr/bin/ultar-llm serve` (gestartet von `ultar-init`) |
| Schnittstelle | OpenAI-kompatibel auf `127.0.0.1:8080` **und** Unix-Socket `/run/ultar/llm.sock` |
| Modelle | `*.gguf` in `/data/ultar/models`, `/data/ultar/agent` (Desktop zusätzlich `~/.local/share/ultar/models`, `/var/lib/ultar/models`) |
| Einstellungen | `/etc/ultar/llm.toml` → `/data/ultar/llm.toml` → `~/.config/ultar/llm.toml` |
| Fertige Programme | `bins/a30/ultar-llm.gz` (aarch64, statisch), `bins/desktop/ultar-llm-x86_64.gz` (x86-64-v3, statisch) |

Der Agent der Oberfläche (`agent.rs`) und der Helfer sprechen unverändert `127.0.0.1:8080/v1/chat/completions` –
nur der Dienst dahinter ist jetzt der eigene. Der alte Weg (llama-server im Ubuntu-Bereich) bleibt als
`llm-compat` erhalten. Mit vorhandenem `/usr/bin/ultar-llm` lädt „Sprachmodell installieren" nur noch das Modell.

## Was der Dienst selbst erledigt
- **Laden bei Bedarf:** Modell erst bei der ersten Anfrage (per mmap), danach im Speicher.
- **Prompt-Zwischenspeicher:** der gemeinsame Anfang mit der letzten Anfrage bleibt im KV-Cache – der lange
  Systemprompt des Agenten wird nur einmal gerechnet (`timings.cached_tokens` in der Antwort).
- **Gültiges JSON erzwungen:** `response_format: {"type":"json_object"}` → Grammatik; ein kleines Modell kann
  keinen kaputten Werkzeugaufruf liefern. Eigene GBNF über `"grammar"`.
- **Qwen3 ohne Denkphase:** bei JSON oder `/no_think` wird der leere `<think>`-Block vorgegeben (schneller).
- **Geräteschutz:** entlädt nach Leerlauf (Handy 5 min, PC 30 min), bei Speicherdruck (PSI), nimmt unter 10 % Akku
  (ohne Ladegerät) und über 75 °C keine Anfragen an (HTTP 503 mit Grund). Vor dem Laden Speicherprüfung.
- **Threads nach Gerät:** Handy = schnelle Kerne (2–4) fürs Erzeugen, alle (≤ 8) fürs Einlesen; PC = physische Kerne.
  `big_cores_only` legt alles auf die schnellen Kerne. Priorität `nice 5` auf dem Handy – Oberfläche bleibt flüssig.
- **Mehrere Modelle:** `"model": "<datei-ohne-.gguf>"` wählt ein anderes Modell; das alte wird vorher freigegeben.
- **Abbruch:** schließt der Aufrufer einen Stream, hört der Rechenkern sofort auf.
- **Sicherheit:** TCP nur auf Loopback; fremde Adressen nur mit `allow_remote = true` **und** `api_key`.
  Socket mit Rechten 0660.

## Endpunkte
| Pfad | Zweck |
|---|---|
| `GET /health` | `{"status":"ok","engine":"idle|loading|ready|busy","model":…}` |
| `GET /v1/models` | alle gefundenen Modelle |
| `POST /v1/chat/completions` | Chat, `stream: true` als Server-Sent Events, `usage` + `timings` |
| `POST /v1/completions` | reiner Text |
| `GET /ultar/status` | Gerät (Akku, Temperatur, Speicher, Sperrgrund), Einstellungen, Tempo der letzten Anfrage |
| `POST /ultar/load` `{"model":…}` / `POST /ultar/unload` | vorladen / Speicher freigeben |

## Befehlszeile
```
ultar-llm info                         # Gerät, wirksame Einstellungen, gewähltes Modell
ultar-llm status | models | unload | load [MODELL]
ultar-llm ask "Wie spät ist es in Damaskus?" [--json] [--stats]
ultar-llm chat                         # Gespräch im Terminal
ultar-llm bench [--tokens 64]          # Tempo je Thread-Zahl messen, Empfehlung für llm.toml
ultar-llm serve --model mock:          # Test-Engine ohne Modell (Schnittstellen-Tests, CI)
```

## Desktop
- Auf Ultar-Desktop startet `ultar-init` denselben Dienst. Programme im Desktop (VS Code mit Continue/Cline,
  Browser-Tools …) nutzen ihn als OpenAI-Server: Basis-Adresse `http://127.0.0.1:8080/v1`, beliebiger Schlüssel.
- Auf einem normalen Ubuntu-PC: `scripts/package-ultar-llm.sh` baut `ultar-llm_<ver>_amd64.deb`
  (mit systemd-Dienst `ultar-llm.service` bzw. Nutzer-Dienst). Modelle nach `/var/lib/ultar/models`.
- Grafikkarte: Features `gpu-vulkan` / `gpu-cuda` (dann nicht mehr vollständig statisch). Standard ist CPU.

## Bauen
```
# PC (x86_64, statisch, AVX2-Grundlage)
RUSTFLAGS="-C target-feature=+crt-static -C target-cpu=x86-64-v3" cargo build --release -p ultar-llm --target x86_64-unknown-linux-gnu
# Handy (aarch64, statisch, ARMv8.0+NEON – läuft auf A30 und jedem 64-Bit-ARM)
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
RUSTC_BOOTSTRAP=1 RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build --release -p ultar-llm -Z build-std=std,panic_abort --target aarch64-unknown-linux-gnu
# ohne Rechenkern (nur Schnittstelle + Test-Engine)
cargo test -p ultar-llm --no-default-features
```
Wichtig: `llama-cpp-2` mit Feature `static-stdcxx` – sonst wird libstdc++ dynamisch gebunden und das mit
`+crt-static` gebaute Programm stürzt beim Start ab.

## Aufs A30 bringen (ohne Neu-Flashen)
```
upush2.ps1 -Local bins\a30\ultar-llm.gz -Dev /data/ultar/ultar-llm.gz
ucmd.ps1 "gzip -dc /data/ultar/ultar-llm.gz > /usr/bin/ultar-llm && chmod 755 /usr/bin/ultar-llm"
# Modell: Qwen3-0.6B-Q4_K_M.gguf nach /data/ultar/models
ucmd.ps1 "ultar-llm bench --tokens 32"
```

## Modelle laden, Empfehlung, Bilder (22.11.2026)

Katalog, Empfehlung nach Gerät und Herunterladen von Hugging Face/Ollama (`src/models.rs`), Bilder über llama.cpp-mtmd
(Feature `vision`, jetzt Standard; Projektor `mmproj-<id>.gguf` neben dem Modell) – Beschreibung und Tabelle in
`docs/SYSTEM-ASSISTENT.md`. Neue Befehle: `catalog`, `recommend`, `pull`, `ask --image`; Schnittstelle `GET /ultar/catalog`.
