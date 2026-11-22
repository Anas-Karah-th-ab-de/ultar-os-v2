#!/bin/sh
# Ultar-Browser: startet im Ubuntu-Bereich ein kleines Fenstersystem (Weston, Wayland) mit dem Browser (GNOME Web, WebKit)
# im Vollbild. Aufruf über ultar-compat: sh /run/ultar/web-session.sh <Adresse>
# Aufruf: web-session.sh <Adresse> [Maßstab, z. B. 2.0] [mobil|desktop]
# Endet der Browser, endet auch das Fenstersystem – die Ultar-Oberfläche übernimmt den Bildschirm wieder.
URL="${1:-about:blank}"
SCALE="${2:-1.0}"
KIND="${3:-mobil}"
export HOME=/root
export XDG_RUNTIME_DIR=/run/ultar-web
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_CONFIG_HOME=/root/.config XDG_DATA_HOME=/root/.local/share XDG_CACHE_HOME=/root/.cache

# Gerätekennzeichnung für Eingabe und Bildschirm: Das Fenstersystem findet Touch, Maus und Tastatur nur über udev.
# (udev verweigert im Ubuntu-Bereich sonst die Arbeit, weil es ein chroot erkennt)
export SYSTEMD_IGNORE_CHROOT=1
if ! pidof systemd-udevd >/dev/null 2>&1; then
  mkdir -p /run/udev
  /lib/systemd/systemd-udevd --daemon 2>/dev/null
fi
udevadm trigger --action=add --subsystem-match=input --subsystem-match=drm 2>/dev/null
udevadm settle -t 8 2>/dev/null
if [ -n "$ULTAR_WEB_DEBUG" ]; then
  echo "udevd: $(pidof systemd-udevd)"; ls /run/udev 2>&1 | head; ls /run/udev/data 2>/dev/null | head -20
  for d in /dev/input/event*; do echo "$d: $(udevadm info -q property "$d" 2>&1 | grep -E 'ID_INPUT|ID_SEAT|TAGS' | tr '\n' ' ')"; done
fi

# Ohne Grafikchip-Treiber lauffähig: alles in Software (langsamer, aber auf jedem Gerät gleich).
# Mit ULTAR_WEB_GPU=1 werden die Software-Vorgaben weggelassen.
export LIBSEAT_BACKEND=builtin
# Ohne erkannte Eingabegeräte trotzdem starten (sie können später dazukommen)
export WLR_LIBINPUT_NO_DEVICES=1
export GDK_BACKEND=wayland
if [ "${ULTAR_WEB_GPU:-0}" != 1 ]; then
  export WLR_RENDERER=pixman GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
# Die Abschottung von WebKit (bubblewrap) braucht Benutzer-Namensräume, die es im Ubuntu-Bereich als root nicht gibt.
# Bis der Browser als eigener Benutzer läuft, ist sie abgeschaltet.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
# Vorgaben bei jedem Start in derselben Sitzung setzen: keine Rückfragen, Tracking-Schutz und Werbefilter an,
# Schrift und Seiten im Maßstab des Geräts, auf Handys die mobile Fassung der Webseiten.
# Maßstab des Fenstersystems (ganzzahlig): Handy mit 2 px/dp -> 2. Damit sind Leisten, Knöpfe und Seiten
# fingertauglich, und Webseiten sehen die Breite eines Handys (360 Punkte).
OSCALE=$(awk "BEGIN{z=int($SCALE*0.85+0.5); if(z<1)z=1; if(z>4)z=4; print z}")
UA=""
[ "$KIND" = mobil ] && UA="Mozilla/5.0 (Linux; Android 14; Mobile) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile Safari/605.1.15"
export URL UA
# Einstellungen des Fenstersystems: ohne Leiste, Browserfenster maximiert (seine Kopfzeile mit Adresse und
# Schließen-Knopf bleibt sichtbar), Software-Darstellung, Maßstab je Bildschirm.
# Endet der Browser, endet Weston (watch=true).
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
cat > "$XDG_RUNTIME_DIR/browser.sh" <<'EOS'
#!/bin/sh
exec epiphany --new-window "$URL"
EOS
chmod +x "$XDG_RUNTIME_DIR/browser.sh"
# Vorgaben bei jedem Start in derselben Sitzung: keine Rückfragen, Tracking-Schutz und Werbefilter an,
# auf Handys die mobile Fassung der Webseiten.
exec dbus-run-session -- sh -c '
  W=org.gnome.Epiphany.web:/org/gnome/epiphany/web/
  gsettings set org.gnome.Epiphany ask-for-default false
  gsettings set org.gnome.Epiphany restore-session-policy crashed
  gsettings set org.gnome.Epiphany homepage-url about:blank
  gsettings set $W enable-adblock true
  gsettings set $W enable-itp true
  gsettings set $W ask-on-download true
  gsettings set $W default-zoom-level 1.0
  gsettings set org.gnome.Epiphany.state:/org/gnome/epiphany/state/ is-maximized true
  gsettings set $W user-agent "$UA"
  gsettings set org.gnome.desktop.interface color-scheme prefer-dark
  exec weston --config="$XDG_RUNTIME_DIR/weston.ini" --log=/run/ultar/log/weston.log
' 2>&1
