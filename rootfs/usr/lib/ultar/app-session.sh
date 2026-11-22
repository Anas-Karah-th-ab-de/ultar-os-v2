#!/bin/sh
# Grafisches Ubuntu-Programm im Vollbild: kleines Fenstersystem (Weston) im Ubuntu-Bereich, darin das Programm.
# Aufruf über ultar-compat: sh /run/ultar/web-session.sh "<Befehl>" [Maßstab]
# Endet das Programm, endet das Fenstersystem – die Ultar-Oberfläche übernimmt den Bildschirm wieder.
# Rückweg unabhängig vom Programm: Ein/Aus-Taste (siehe ultar-shell).
CMD="${1:-}"
SCALE="${2:-1.0}"
[ -n "$CMD" ] || { echo "kein Befehl"; exit 2; }
export HOME=/root
export XDG_RUNTIME_DIR=/run/ultar-web
mkdir -p "$XDG_RUNTIME_DIR" /run/ultar/log && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_CONFIG_HOME=/root/.config XDG_DATA_HOME=/root/.local/share XDG_CACHE_HOME=/root/.cache

# Eingabegeräte und Bildschirm findet das Fenstersystem nur über udev (im Ubuntu-Bereich = chroot)
export SYSTEMD_IGNORE_CHROOT=1
if ! pidof systemd-udevd >/dev/null 2>&1; then
  mkdir -p /run/udev
  /lib/systemd/systemd-udevd --daemon 2>/dev/null
fi
udevadm trigger --action=add --subsystem-match=input --subsystem-match=drm 2>/dev/null
udevadm settle -t 8 2>/dev/null

export LIBSEAT_BACKEND=builtin WLR_LIBINPUT_NO_DEVICES=1
# Programme sollen Wayland nehmen (GTK, Qt, SDL, Firefox); X11-Programme laufen über Xwayland, falls installiert
export GDK_BACKEND=wayland QT_QPA_PLATFORM=wayland SDL_VIDEODRIVER=wayland MOZ_ENABLE_WAYLAND=1
if [ "${ULTAR_WEB_GPU:-0}" != 1 ]; then
  export GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1
fi
OSCALE=$(awk "BEGIN{z=int($SCALE*0.85+0.5); if(z<1)z=1; if(z>4)z=4; print z}")
INI="$XDG_RUNTIME_DIR/weston-app.ini"
{
  printf '[core]\nshell=desktop-shell.so\nbackend=drm-backend.so\nidle-time=0\nrequire-input=false\n'
  [ "${ULTAR_WEB_GPU:-0}" != 1 ] && printf 'renderer=pixman\n'
  [ -x /usr/bin/Xwayland ] && printf 'xwayland=true\n'
  for c in /sys/class/drm/card*-*; do
    [ "$(cat "$c/status" 2>/dev/null)" = connected ] || continue
    n=${c##*/}; n=${n#card*-}
    printf '\n[output]\nname=%s\nscale=%s\n' "$n" "$OSCALE"
  done
  printf '\n[shell]\npanel-position=none\nlocking=false\nbackground-color=0xff0b0d11\nanimation=none\nstartup-animation=none\nclose-animation=none\n'
  printf '\n[autolaunch]\npath=%s\nwatch=true\n' "$XDG_RUNTIME_DIR/ultar-app"
} > "$INI"
# Der Startname „ultar-app" erlaubt der Oberfläche, die Sitzung gezielt zu beenden.
{
  printf '#!/bin/sh\n'
  printf 'exec %s\n' "$CMD"
} > "$XDG_RUNTIME_DIR/ultar-app"
chmod +x "$XDG_RUNTIME_DIR/ultar-app"
exec dbus-run-session -- sh -c '
  gsettings set org.gnome.desktop.interface color-scheme prefer-dark 2>/dev/null
  exec weston --config="$XDG_RUNTIME_DIR/weston-app.ini" --log=/run/ultar/log/weston-app.log
' 2>&1
