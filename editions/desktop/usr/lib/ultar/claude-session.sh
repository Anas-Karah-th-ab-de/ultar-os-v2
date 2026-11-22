#!/bin/sh
# Claude starten (im Ubuntu-Bereich, aus dem Desktop-Startmenü):
#   claude-session.sh code   Claude Code im Terminal, Arbeitsordner /data/ultar/dev
#   claude-session.sh web    claude.ai als eigenes Fenster (Anmeldung bleibt erhalten)
export HOME=/data/ultar/home PATH="/data/ultar/home/.local/bin:/data/ultar/home/.cargo/bin:/data/ultar/home/android/platform-tools:$PATH"
export ANDROID_HOME=/data/ultar/home/android
mkdir -p "$HOME" /data/ultar/dev
case "${1:-code}" in
  code)
    if ! command -v claude >/dev/null; then
      exec x-terminal-emulator -e sh -c 'echo "Claude wird eingerichtet …"; sh /data/ultar/claude/claude-setup.sh claude; cd /data/ultar/dev && claude; exec bash'
    fi
    cd /data/ultar/dev && exec x-terminal-emulator -e sh -c 'claude; exec bash' ;;
  web)
    for b in ultar-web epiphany-browser firefox chromium; do
      command -v $b >/dev/null || continue
      case $b in
        epiphany-browser) exec epiphany-browser --application-mode --profile="$HOME/.claude-web" https://claude.ai ;;
        *) exec $b https://claude.ai ;;
      esac
    done
    echo "Kein Browser im Ubuntu-Bereich" ;;
esac
