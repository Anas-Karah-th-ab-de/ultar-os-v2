#!/bin/sh
# Claude auf Ultar OS (Desktop-Ausgabe) einrichten – läuft im Ubuntu-Bereich:
#   ultar-compat run -- sh /data/ultar/claude/claude-setup.sh   (bzw. Startmenü „Claude einrichten“)
#
# - Claude Code (offizieller Installer von Anthropic): Anmeldung mit dem eigenen Claude-Abo (Pro/Max)
#   per „/login“ im Browser – kein API-Schlüssel nötig.
# - Claude im Browser (claude.ai) als eigenes Fenster mit dauerhafter Anmeldung.
# - Entwicklungswerkzeuge für Apps: Linux/Ultar (Rust, C), Windows (MinGW-Querübersetzer, Test mit Wine),
#   Android (JDK 17, Android-Kommandozeilenwerkzeuge, Test mit Waydroid), Ultar-Abbilder (QEMU).
# Alles Persönliche liegt unter /data/ultar/home (bleibt bei Systemupdates erhalten).
set -e
export HOME=/data/ultar/home DEBIAN_FRONTEND=noninteractive
mkdir -p "$HOME" /data/ultar/dev
STEP=${1:-all}

claude_code() {
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends curl ca-certificates git ripgrep >/dev/null
  curl -fsSL https://claude.ai/install.sh | bash
  ln -sf "$HOME/.local/bin/claude" /usr/local/bin/claude
  echo "Claude Code installiert: $(claude --version 2>/dev/null)"
}

dev_tools() {
  [ "$(dpkg --print-architecture)" = amd64 ] && MINGW="gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 nsis wixl msitools" || MINGW=""
  apt-get install -y -qq --no-install-recommends build-essential pkg-config cmake clang lld git nodejs npm python3 python3-pip \
    qemu-system-x86 qemu-system-arm qemu-utils ovmf openjdk-17-jdk-headless unzip zip $MINGW >/dev/null
  if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  fi
  "$HOME/.cargo/bin/rustup" target add x86_64-pc-windows-gnu aarch64-unknown-linux-gnu aarch64-linux-android x86_64-unknown-linux-gnu 2>/dev/null || true
  # Android: Kommandozeilenwerkzeuge (sdkmanager), Plattform und Build-Tools
  A=$HOME/android; if [ ! -x "$A/cmdline-tools/latest/bin/sdkmanager" ]; then
    mkdir -p "$A/cmdline-tools" && cd /tmp
    curl -fsSLo cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip
    unzip -qo cmdtools.zip -d "$A/cmdline-tools" && mv "$A/cmdline-tools/cmdline-tools" "$A/cmdline-tools/latest" && rm -f cmdtools.zip
    yes | "$A/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$A" "platform-tools" "platforms;android-35" "build-tools;35.0.0" >/dev/null || true
  fi
  echo "Entwicklungswerkzeuge bereit (Rust, MinGW/Wine, Android SDK, QEMU)"
}

case "$STEP" in
  claude) claude_code ;;
  dev) dev_tools ;;
  all) claude_code; dev_tools ;;
esac
