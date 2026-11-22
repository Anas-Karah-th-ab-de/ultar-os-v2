#!/bin/sh
# Startet librespot als Spotify-Connect-Gerät „Ultar" (offener Client, braucht ein Spotify-PREMIUM-Konto).
# KEIN Passwort nötig: über Spotify Connect (Zeroconf) im selben WLAN wählt man „Ultar" in der Spotify-App.
# Voraussetzung: das statische aarch64-librespot-Binary liegt unter /usr/lib/ultar/librespot (siehe docs/A30-WEBAPPS.md).
# Audio geht an die Standard-ALSA-Karte – funktioniert erst, wenn der Ton am A30 läuft (ABOX-DSP-Firmware, siehe Audio-Paket).
BIN=/usr/lib/ultar/librespot
[ -x "$BIN" ] || { echo "librespot fehlt: $BIN"; exit 1; }
CACHE=/data/ultar/spotify
mkdir -p "$CACHE" 2>/dev/null
exec "$BIN" \
  --name "Ultar" \
  --bitrate 160 \
  --cache "$CACHE" \
  --backend alsa \
  --device default \
  --disable-audio-cache \
  --zeroconf-port 0
