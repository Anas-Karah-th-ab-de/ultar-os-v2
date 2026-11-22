#!/usr/bin/env bash
# Installiert/aktualisiert ultraos-server auf dem Server (Ubuntu 22.04/24.04) – als root ausführen:
#   ./install.sh                 baut aus dem Quellcode (cargo) und installiert
#   BIN=/pfad/ultraos-server ./install.sh   fertiges Binary verwenden
# Idempotent: zweiter Aufruf = Update mit Neustart; der Token-Schlüssel bleibt erhalten.
set -euo pipefail
cd "$(dirname "$0")"
QUELLE=..
if [ -z "${BIN:-}" ]; then
  command -v cargo >/dev/null || { echo "cargo fehlt (apt install cargo) oder BIN=… setzen"; exit 1; }
  (cd $QUELLE && cargo build --release)
  BIN=$QUELLE/target/release/ultraos-server
fi
install -Dm755 "$BIN" /usr/local/bin/ultraos-server.neu
mv /usr/local/bin/ultraos-server.neu /usr/local/bin/ultraos-server
install -d -m700 /etc/ultraos
if [ ! -f /etc/ultraos/secret.env ]; then
  echo "ULTRAOS_SECRET=$(openssl rand -hex 32)" > /etc/ultraos/secret.env
  chmod 600 /etc/ultraos/secret.env
  echo "neuer Token-Schlüssel in /etc/ultraos/secret.env"
fi
install -Dm644 ultraos.service /etc/systemd/system/ultraos.service
install -Dm755 ultraos-admin /usr/local/bin/ultraos-admin
if command -v caddy >/dev/null && ! grep -q ultar.code-craft.tech /etc/caddy/Caddyfile 2>/dev/null; then
  install -Dm644 Caddyfile /etc/caddy/Caddyfile && systemctl reload caddy || true
fi
systemctl daemon-reload
systemctl enable ultraos.service >/dev/null
systemctl restart ultraos.service
for _ in $(seq 30); do curl -sf http://127.0.0.1:8080/v1/health >/dev/null && break; sleep 0.3; done
curl -sf http://127.0.0.1:8080/v1/health >/dev/null && echo "ultraos-server läuft" || { journalctl -u ultraos -n 30 --no-pager; exit 1; }
curl -s http://127.0.0.1:8080/v1/verbund/statistik; echo
