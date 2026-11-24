#!/bin/bash
# Replaces the website /opt/ultar/web with a new version, keeping downloads/ and store/.
# The old site stays as /opt/ultar/web.old-<time> (rollback-web.sh) and is also archived in
# /root/ultar-backup-<time>/web.tar.gz.  Usage: deploy-web.sh <site-dir>
set -euo pipefail
SRC=$(realpath "${1:?site dir}")
TS=$(date -u +%Y%m%d-%H%M%S)
B=/root/ultar-backup-$TS; mkdir -p "$B"
tar -C /opt/ultar -czf "$B/web.tar.gz" web
NEW=/opt/ultar/web.new; rm -rf "$NEW"; mkdir -p "$NEW"
cp -a "$SRC"/. "$NEW"/
cp -a /opt/ultar/web/downloads "$NEW/"
[ -e /opt/ultar/web/store ] && cp -a /opt/ultar/web/store "$NEW/"
chown -R root:root "$NEW"; find "$NEW" -type d -exec chmod 0755 {} +; find "$NEW" -type f -exec chmod 0644 {} +
mv /opt/ultar/web "/opt/ultar/web.old-$TS" && mv "$NEW" /opt/ultar/web
ls -1d /opt/ultar/web.old-* | sort | head -n -3 | xargs -r rm -rf
echo "DEPLOY-WEB: OK (previous: /opt/ultar/web.old-$TS, archive $B/web.tar.gz)"
