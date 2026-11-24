#!/bin/bash
# Installs or updates the signing service on the release server (idempotent).
# Usage: install-signd.sh <path to ultar-signd binary>
# Creates: user ultar-sign (no login), group ultar-publish, /usr/local/lib/ultar-sign/ultar-signd,
# the systemd unit, and – only if missing – the three keys (os, system, store), each 0400.
set -euo pipefail
BIN=${1:?binary}
id ultar-sign >/dev/null 2>&1 || useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin ultar-sign
getent group ultar-publish >/dev/null || groupadd --system ultar-publish
install -d -o root -g root -m 0755 /usr/local/lib/ultar-sign
install -o root -g root -m 0755 "$BIN" /usr/local/lib/ultar-sign/ultar-signd
install -o root -g root -m 0644 "$(dirname "$0")/ultar-signd.service" /etc/systemd/system/ultar-signd.service
install -d -o ultar-sign -g ultar-sign -m 0700 /var/lib/ultar-sign /var/lib/ultar-sign/keys
for role in os system store; do
  if [ ! -e /var/lib/ultar-sign/keys/$role.key ]; then
    runuser -u ultar-sign -- /usr/local/lib/ultar-sign/ultar-signd keygen /var/lib/ultar-sign/keys $role > /var/lib/ultar-sign/$role.pub.new
    chown root:root /var/lib/ultar-sign/$role.pub.new
    mv /var/lib/ultar-sign/$role.pub.new /var/lib/ultar-sign/$role.pub
    chmod 0644 /var/lib/ultar-sign/$role.pub
    echo "new key: $role $(cat /var/lib/ultar-sign/$role.pub)"
  fi
done
chmod 0400 /var/lib/ultar-sign/keys/*.key
chown ultar-sign:ultar-sign /var/lib/ultar-sign/keys/*.key
systemctl daemon-reload
systemctl enable ultar-signd >/dev/null 2>&1
systemctl restart ultar-signd
sleep 1
systemctl is-active ultar-signd
