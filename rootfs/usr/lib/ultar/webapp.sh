#!/bin/sh
# Ultar Web-App-Starter: wie web-session.sh, aber mit EIGENEM, dauerhaftem Profil je App.
# So bleiben WhatsApp-, YouTube-, Spotify- usw. Logins getrennt erhalten (nicht ein gemeinsames Browserprofil).
# Aufruf über ultar-compat: sh /run/ultar/webapp.sh <Adresse> [Maßstab] [mobil|desktop]
# Das Profil wird aus der Domain der Adresse abgeleitet (web.whatsapp.com -> whatsapp), liegt unter
# /data/ultar/webapps/<slug> und übersteht Neustarts. Endet der Browser, endet das Fenstersystem.
URL="${1:-about:blank}"
SCALE="${2:-1.0}"
KIND="${3:-}"

# --- App-Kennung (slug) + passende Vorgaben aus der Adresse ableiten ---
host=$(printf '%s' "$URL" | sed -e 's#^[a-z]*://##' -e 's#/.*##' | tr 'A-Z' 'a-z')
case "$host" in
  *whatsapp.com*)      slug=whatsapp;  DEFKIND=desktop ; APPUA=desktop ;;
  *web.telegram.org*)  slug=telegram;  DEFKIND=desktop ; APPUA=desktop ;;
  *youtube.com*)       slug=youtube;   DEFKIND=mobil   ; APPUA=mobile  ;;
  *open.spotify.com*)  slug=spotify;   DEFKIND=desktop ; APPUA=desktop ;;
  *)                   slug=$(printf '%s' "$host" | sed 's/[^a-z0-9]/_/g'); [ -z "$slug" ] && slug=browser; DEFKIND=mobil; APPUA=mobile ;;
esac
[ -z "$KIND" ] && KIND="$DEFKIND"

# --- Dauerhaftes, isoliertes Profil je App ---
PROFILE="/data/ultar/webapps/$slug"
mkdir -p "$PROFILE" 2>/dev/null || PROFILE="/root/.webapps/$slug"
mkdir -p "$PROFILE"
export HOME="$PROFILE"
export XDG_RUNTIME_DIR=/run/ultar-web
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_CONFIG_HOME="$PROFILE/.config" XDG_DATA_HOME="$PROFILE/.local/share" XDG_CACHE_HOME="$PROFILE/.cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"

# --- Eingabe/Anzeige über udev (wie web-session.sh) ---
export SYSTEMD_IGNORE_CHROOT=1
if ! pidof systemd-udevd >/dev/null 2>&1; then
  mkdir -p /run/udev
  /lib/systemd/systemd-udevd --daemon 2>/dev/null
fi
udevadm trigger --action=add --subsystem-match=input --subsystem-match=drm 2>/dev/null
udevadm settle -t 8 2>/dev/null

# --- Software-Darstellung (ohne GPU-Treiber lauffähig) ---
export LIBSEAT_BACKEND=builtin
export WLR_LIBINPUT_NO_DEVICES=1
export GDK_BACKEND=wayland
if [ "${ULTAR_WEB_GPU:-0}" != 1 ]; then
  export WLR_RENDERER=pixman GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

OSCALE=$(awk "BEGIN{z=int($SCALE*0.85+0.5); if(z<1)z=1; if(z>4)z=4; print z}")
if [ "$APPUA" = mobile ]; then
  UA="Mozilla/5.0 (Linux; Android 14; Mobile) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile Safari/605.1.15"
else
  UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15"
fi
export URL UA

# --- Fenstersystem: ohne Leiste, Browser maximiert, endet mit dem Browser ---
INI="$XDG_RUNTIME_DIR/weston.ini"
{
  printf '[core]\nshell=desktop-shell.so\nbackend=drm-backend.so\nidle-time=0\nrequire-input=false\n'
  [ "${ULTAR_WEB_GPU:-0}" != 1 ] && printf 'renderer=pixman\n'
  for c in /sys/class/drm/card*-*; do
    [ "$(cat "$c/status" 2>/dev/null)" = connected ] || continue
    n=${c##*/}; n=${n#card*-}
    printf '\n[output]\nname=%s\nscale=%s\n' "$n" "$OSCALE"
  done
  printf '\n[shell]\npanel-position=none\nlocking=false\nbackground-color=0xff0b0d11\nanimation=none\nstartup-animation=none\nclose-animation=none\n'
  printf '\n[autolaunch]\npath=%s\nwatch=true\n' "$XDG_RUNTIME_DIR/browser.sh"
} > "$INI"

# Epiphany im Web-App-Modus: eigenes Profil (--profile), ein fester Auftritt je App.
cat > "$XDG_RUNTIME_DIR/browser.sh" <<EOS
#!/bin/sh
exec epiphany --application-mode --profile="$PROFILE/epiphany" --new-window "$URL"
EOS
chmod +x "$XDG_RUNTIME_DIR/browser.sh"

mkdir -p /run/ultar/log
exec dbus-run-session -- sh -c '
  W=org.gnome.Epiphany.web:/org/gnome/epiphany/web/
  gsettings set org.gnome.Epiphany ask-for-default false 2>/dev/null
  gsettings set org.gnome.Epiphany restore-session-policy crashed 2>/dev/null
  gsettings set $W enable-adblock true 2>/dev/null
  gsettings set $W enable-itp true 2>/dev/null
  gsettings set $W ask-on-download true 2>/dev/null
  gsettings set $W default-zoom-level 1.0 2>/dev/null
  gsettings set $W user-agent "$UA" 2>/dev/null
  gsettings set org.gnome.desktop.interface color-scheme prefer-dark 2>/dev/null
  exec weston --config="$XDG_RUNTIME_DIR/weston.ini" --log=/run/ultar/log/weston.log
' 2>&1
