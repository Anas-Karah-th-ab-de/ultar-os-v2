#!/bin/sh
# Ultar-Desktop: eigenes Fenstersystem ultar-wm (Rust, Wayland) im Ubuntu-Bereich – Fenster nebeneinander, Leiste,
# Startmenü mit allen installierten Programmen (VS Code, Firefox, LibreOffice …), X11-Programme über Xwayland.
# Aufruf aus ultar-shell über ultar-compat: sh /run/ultar/web-session.sh
# Endet ultar-wm („Zurück zu Ultar“ im Startmenü, Strg+Alt+Rücktaste oder Ein/Aus-Taste), übernimmt die
# Ultar-Oberfläche den Bildschirm wieder.
export HOME=/root
export XDG_RUNTIME_DIR=/run/ultar-desktop
mkdir -p "$XDG_RUNTIME_DIR" /run/ultar/log && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_CONFIG_HOME=/root/.config XDG_DATA_HOME=/root/.local/share XDG_CACHE_HOME=/root/.cache
export LANG="${LANG:-C.UTF-8}"
# Xwayland braucht das X11-Socket-Verzeichnis (fehlt in einem frisch eingerichteten Ubuntu-Bereich)
mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

# Eingabegeräte und Grafikchip findet libinput/libseat nur über udev (im Ubuntu-Bereich = chroot)
export SYSTEMD_IGNORE_CHROOT=1
if ! pidof systemd-udevd >/dev/null 2>&1; then
  mkdir -p /run/udev
  /lib/systemd/systemd-udevd --daemon 2>/dev/null
fi
udevadm trigger --action=add --subsystem-match=input --subsystem-match=drm 2>/dev/null
udevadm settle -t 8 2>/dev/null

# Sitzung ohne logind: libseat übernimmt den Bildschirm selbst (läuft als root)
export LIBSEAT_BACKEND=builtin
# Anzeigeweg: mit DRM-Grafikchip GPU (--drm), sonst Bildspeicher mit Pixman (--fb, z. B. Galaxy A30 über DECON).
# Programme rechnen ohne GPU-Treiber in Software (Mesa llvmpipe).
MODE=--drm
ls /dev/dri/card* >/dev/null 2>&1 || MODE=--fb
[ "$MODE" = --fb ] && export LIBGL_ALWAYS_SOFTWARE=1 GSK_RENDERER=cairo
[ "${ULTAR_DESKTOP_SOFTWARE:-0}" = 1 ] && export LIBGL_ALWAYS_SOFTWARE=1

WM=$(command -v ultar-wm || echo /usr/lib/ultar/ultar-wm)
[ -x "$WM" ] || { echo "ultar-wm fehlt (Paket ultar-wm im Ubuntu-Bereich installieren)"; exit 2; }

# dbus für Programme, die ihn brauchen (Einstellungen von GTK, Dateidialoge)
if command -v dbus-run-session >/dev/null 2>&1; then
  exec dbus-run-session -- "$WM" $MODE
fi
exec "$WM" $MODE
