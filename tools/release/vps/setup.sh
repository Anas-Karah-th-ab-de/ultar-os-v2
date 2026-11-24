#!/bin/bash
# Release server, one-time setup (idempotent): signing service, nginx location for the store,
# public verification files. Backup first; nginx is restored if its test fails.
# Usage: setup.sh <ultar-signd binary>   (run from the unpacked bundle directory)
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd)
BIN=$(realpath "${1:?ultar-signd binary}")
TS=$(date -u +%Y%m%d-%H%M%S)
B=/root/ultar-backup-$TS
mkdir -p "$B"
cp -a /etc/nginx/sites-available/ultar.conf "$B/ultar.conf"
cp -a /etc/nginx/snippets "$B/snippets"
tar -C /opt/ultar -czf "$B/web.tar.gz" web
[ -f /etc/systemd/system/ultar-signd.service ] && cp -a /etc/systemd/system/ultar-signd.service "$B/"
echo "SETUP: backup in $B"

bash "$D/install-signd.sh" "$BIN"

CONF=/etc/nginx/sites-available/ultar.conf
if ! grep -q 'location \^~ /store/' "$CONF"; then
  python3 - "$CONF" "$D/nginx-store.conf" <<'PY'
import sys
conf, snip = sys.argv[1], open(sys.argv[2]).read()
s = open(conf).read()
anchor = "    location /v1/account/ {"
if s.count(anchor) != 1:
    sys.exit("anchor not found once")
s = s.replace(anchor, snip.rstrip("\n") + "\n\n" + anchor)
open(conf, "w").write(s)
PY
fi
if ! nginx -t 2>"$B/nginx-test.log"; then
  cp -a "$B/ultar.conf" "$CONF"; nginx -t && systemctl reload nginx
  echo "SETUP: nginx test failed, restored"; cat "$B/nginx-test.log"; exit 1
fi
systemctl reload nginx
mkdir -p /opt/ultar/store/releases /opt/ultar/web/store /opt/ultar/web/downloads
/usr/local/lib/ultar-sign/ultar-signd ssh-signers /run/ultar-sign/sign.sock >/opt/ultar/web/downloads/ultar-allowed-signers.new
mv /opt/ultar/web/downloads/ultar-allowed-signers.new /opt/ultar/web/downloads/ultar-allowed-signers
chmod 0644 /opt/ultar/web/downloads/ultar-allowed-signers
echo "$B" >/opt/ultar/last-backup
echo "SETUP: OK"
