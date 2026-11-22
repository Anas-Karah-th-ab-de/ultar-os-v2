#!/usr/bin/env bash
# Ultar OS – Entwickler-Zugang vom Linux-/WSL-Rechner (OpenSSH). Gegenstück zu ultar-dev.ps1 (Windows).
#
#   tools/entwickler/ultar-dev.sh einrichten <adresse> [port] [name]   Schlüssel + SSH-Eintrag (Standard: name ultar, port 2222)
#   tools/entwickler/ultar-dev.sh einrichten 127.0.0.1 2222 ultar-qemu  QEMU (tools/entwickler/qemu-dev.sh)
#   tools/entwickler/ultar-dev.sh info | log | bild [ziel.png] | hilfe | schluessel
#   tools/entwickler/ultar-dev.sh tippen 640 400 | text "hallo" | taste enter | oeffne settings
#   tools/entwickler/ultar-dev.sh hin <datei> [ziel] | her <pfad> [ziel] | weiterleiten [port]
#   tools/entwickler/ultar-dev.sh <beliebiger befehl auf dem Gerät>
#   Gerät wählen: ULTAR_GERAET=ultar-qemu …
set -euo pipefail
NAME=${ULTAR_GERAET:-ultar}
KEY=$HOME/.ssh/ultar_dev_ed25519
CFG=$HOME/.ssh/config
befehl=${1:-hilfe}; shift || true

case "$befehl" in
  einrichten)
    adr=${1:?adresse fehlt}; port=${2:-2222}; NAME=${3:-$NAME}
    mkdir -p "$HOME/.ssh"; chmod 700 "$HOME/.ssh"
    [ -f "$KEY" ] || ssh-keygen -q -t ed25519 -N "" -C "ultar-dev@$(hostname)" -f "$KEY"
    touch "$CFG"; chmod 600 "$CFG"
    alt=$(awk -v n="$NAME" '$0=="# >>> ultar-dev " n {s=1; next} $0=="# <<< ultar-dev " n {s=0; next} !s' "$CFG")
    { printf '# >>> ultar-dev %s\nHost %s\n    HostName %s\n    Port %s\n    User root\n    IdentityFile %s\n    IdentitiesOnly yes\n    UserKnownHostsFile %s\n    StrictHostKeyChecking accept-new\n    ServerAliveInterval 15\n    ConnectTimeout 10\n# <<< ultar-dev %s\n\n' \
        "$NAME" "$NAME" "$adr" "$port" "$KEY" "$HOME/.ssh/ultar_known_hosts" "$NAME"; printf '%s\n' "$alt"; } > "$CFG.neu"
    mv "$CFG.neu" "$CFG"
    echo "SSH-Eintrag '$NAME' → $adr:$port angelegt ($CFG)."
    echo
    echo "Öffentlicher Schlüssel – auf dem Gerät erlauben (Einstellungen → System → Entwickleroptionen) oder beim Bauen:"
    echo "  ENTWICKLER=1 ENTWICKLER_SCHLUESSEL=$KEY.pub scripts/build-initramfs.sh   bzw. tools/entwickler/qemu-dev.sh"
    echo
    cat "$KEY.pub"
    ;;
  schluessel) cat "$KEY.pub" ;;
  log) exec ssh "$NAME" kernlog ;;
  bild)
    ziel=${1:-ultar-bildschirm.png}
    r=$(ssh "$NAME" bildschirm)
    scp -q "$NAME:${r%% *}" "$ziel" && echo "$ziel (${r##* })"
    ;;
  hin) scp -q "${1:?datei}" "$NAME:${2:-/root/}" ;;
  her) scp -q "$NAME:${1:?pfad}" "${2:-.}" ;;
  weiterleiten) p=${1:-8080}; echo "127.0.0.1:$p → Gerät 127.0.0.1:$p (Strg+C beendet)"; exec ssh -N -L "$p:127.0.0.1:$p" "$NAME" ;;
  hilfe) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; echo; echo "Auf dem Gerät eingebaut:"; ssh "$NAME" ultar-hilfe 2>/dev/null || true ;;
  *) exec ssh "$NAME" "$befehl" "$@" ;;
esac
