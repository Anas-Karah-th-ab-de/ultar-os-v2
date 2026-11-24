#!/usr/bin/env bash
# Erstellt die Ubuntu-Kompatibilitätspartition (Ubuntu 24.04 minbase) als ext4-Image.
#   ARCH=amd64|arm64  SIZE=1G  OUT=out/  PROFILE=phone|desktop|server
#
# Profile (welche Ausgabe von Ultar OS den Bereich bekommt):
#   phone    WLAN-Anmeldung, Downloads (wie bisher, klein)
#   desktop  + Linux-Kernel/Module/Firmware für PCs und Laptops, Fenstersystem-Unterbau (Xwayland, Weston,
#            Mesa, Schriften), Windows-Programme (Wine, 32+64 Bit), Flatpak, Android-Apps (Waydroid)
#   server   + Linux-Kernel/Module/Firmware, OpenSSH, nftables, Container (Podman), Werkzeuge
# Kernel und Module liegen im Ubuntu-Bereich: der Kernel (/boot/vmlinuz-*) startet Ultar, die Module lädt
# ultar-compat (modprobe) bei Bedarf – so passen Kernel und Treiber immer zusammen.
set -euo pipefail
cd "$(dirname "$0")/.."
ARCH=${ARCH:-amd64}; SIZE=${SIZE:-}; OUT=${OUT:-out}; PROFILE=${PROFILE:-phone}
# Ubuntu-Version: noble (24.04) oder resolute (26.04 LTS)
SUITE=${SUITE:-noble}
ROOT=${COMPAT_ROOT:-$OUT/compat-root-$ARCH-$PROFILE}
MIRROR=${MIRROR:-http://archive.ubuntu.com/ubuntu/}
[ "$ARCH" = arm64 ] && MIRROR=${MIRROR_ARM:-http://ports.ubuntu.com/ubuntu-ports/}
mkdir -p "$OUT"
if [ ! -x "$ROOT/usr/bin/dpkg" ]; then
  FOREIGN=""; [ "$ARCH" != "$(dpkg --print-architecture)" ] && FOREIGN="--foreign"
  DBS=""; [ -e "/usr/share/debootstrap/scripts/$SUITE" ] || DBS=/usr/share/debootstrap/scripts/gutsy
  debootstrap --variant=minbase --arch="$ARCH" $FOREIGN "$SUITE" "$ROOT" "$MIRROR" $DBS
fi
# -updates/-security: dort liegen u. a. die Kernel-Modulpakete für die Treibersuche
printf 'deb %s %s main universe restricted multiverse\ndeb %s %s-updates main universe restricted multiverse\ndeb %s %s-security main universe restricted multiverse\n' "$MIRROR" "$SUITE" "$MIRROR" "$SUITE" "$MIRROR" "$SUITE" > "$ROOT/etc/apt/sources.list"

PKGS="wpasupplicant iw curl ca-certificates kmod"
# linux-image-generic zieht in noble auch linux-modules-extra (Binder für Waydroid, WLAN, Grafik) mit
KERNEL_PKGS="linux-image-generic linux-firmware linux-firmware-intel-wireless linux-firmware-intel-misc linux-firmware-intel-graphics linux-firmware-nvidia-graphics gdisk e2fsprogs dosfstools parted cloud-guest-utils"
case "$PROFILE" in
  phone) ;;
  desktop)
    PKGS="$PKGS $KERNEL_PKGS udev dbus dbus-user-session libinput10 libseat1 xwayland weston mesa-vulkan-drivers libgl1-mesa-dri libegl1 \
      fonts-noto-core fonts-noto-color-emoji fonts-dejavu-core flatpak bubblewrap xdg-utils desktop-file-utils shared-mime-info \
      lxc python3 python3-gbinder iptables dnsmasq-base openssh-client sudo nano less htop unzip p7zip-full \
      foot epiphany-browser git ripgrep"
    [ "$ARCH" = amd64 ] && PKGS="$PKGS wine wine64 wine32:i386 libvulkan1:i386 mesa-vulkan-drivers:i386 libgl1-mesa-dri:i386 cabextract winbind"
    ;;
  server)
    PKGS="$PKGS $KERNEL_PKGS openssh-server nftables iproute2 iputils-ping podman crun conmon uidmap slirp4netns \
      rsync tzdata nano less htop tmux unzip sudo"
    ;;
  *) echo "PROFILE muss phone, desktop oder server sein"; exit 2 ;;
esac
PKGS="$PKGS ${EXTRA_PKGS:-}"

if [ "$ARCH" = "$(dpkg --print-architecture)" ]; then
  cp /etc/resolv.conf "$ROOT/etc/resolv.conf"
  printf '#!/bin/sh\nexit 101\n' > "$ROOT/usr/sbin/policy-rc.d"; chmod +x "$ROOT/usr/sbin/policy-rc.d"   # keine Dienste im chroot starten
  # Ultar baut sein eigenes Startabbild – Ubuntus initramfs beim Kernel-Paket nicht erzeugen (spart Minuten und Platz)
  mkdir -p "$ROOT/etc/initramfs-tools"
  printf 'update_initramfs=no\n' > "$ROOT/etc/initramfs-tools/update-initramfs.conf"
  for d in proc sys dev; do mountpoint -q "$ROOT/$d" || mount --bind "/$d" "$ROOT/$d"; done
  trap 'for d in dev sys proc; do umount -l "$ROOT/$d" 2>/dev/null || true; done' EXIT
  [ "$PROFILE" = desktop ] && [ "$ARCH" = amd64 ] && chroot "$ROOT" dpkg --add-architecture i386
  chroot "$ROOT" apt-get update -qq
  # Pakete, die es in dieser Ubuntu-Version nicht gibt, auslassen (z. B. python3-gbinder in 26.04)
  VERFUEGBAR=""
  for pk in $PKGS; do
    if chroot "$ROOT" apt-cache show "${pk%%:*}" >/dev/null 2>&1; then VERFUEGBAR="$VERFUEGBAR $pk"; else echo "Hinweis: Paket fehlt in $SUITE: $pk"; fi
  done
  DEBIAN_FRONTEND=noninteractive chroot "$ROOT" apt-get install -y -qq --no-install-recommends $VERFUEGBAR
  # Android-Apps (Desktop): Waydroid aus dem Paketarchiv des Projekts (nicht in Ubuntu enthalten).
  # Ohne Netz/Archiv geht es ohne weiter – uapp meldet dann „waydroid fehlt“.
  if [ "$PROFILE" = desktop ] && [ "${WAYDROID:-1}" = 1 ]; then
    if curl -fsSL --max-time 60 https://repo.waydro.id/waydroid.gpg -o "$ROOT/usr/share/keyrings/waydroid.gpg"; then
      echo "deb [signed-by=/usr/share/keyrings/waydroid.gpg] https://repo.waydro.id/ noble main" > "$ROOT/etc/apt/sources.list.d/waydroid.list"
      chroot "$ROOT" apt-get update -qq && DEBIAN_FRONTEND=noninteractive chroot "$ROOT" apt-get install -y -qq --no-install-recommends waydroid \
        || echo "WARNUNG: Waydroid nicht installiert (Android-Apps erst nach: ultar-compat apt install waydroid)"
    else
      echo "WARNUNG: Waydroid-Archiv nicht erreichbar – Android-Apps später nachrüsten"
    fi
  fi
  # Ultar-Desktop (Fenstersystem ultar-wm), wenn als Paket gebaut (scripts/package-ultar-wm.sh)
  if [ "$PROFILE" = desktop ]; then
    WMDEB=${ULTAR_WM_DEB:-$(ls -1 "$OUT"/ultar-wm_*_"$ARCH".deb 2>/dev/null | tail -1)}
    if [ -n "$WMDEB" ] && [ -f "$WMDEB" ]; then
      cp "$WMDEB" "$ROOT/tmp/ultar-wm.deb"
      DEBIAN_FRONTEND=noninteractive chroot "$ROOT" apt-get install -y -qq --no-install-recommends /tmp/ultar-wm.deb
      rm -f "$ROOT/tmp/ultar-wm.deb"
    else
      echo "Hinweis: kein ultar-wm-Paket gefunden (scripts/package-ultar-wm.sh) – Desktop-Ausgabe startet dann in der Touch-Oberfläche"
    fi
  fi
  chroot "$ROOT" apt-get clean
  rm -f "$ROOT/usr/sbin/policy-rc.d"
  for d in dev sys proc; do umount -l "$ROOT/$d" 2>/dev/null || true; done
  trap - EXIT
elif [ "$PROFILE" != phone ]; then
  echo "PROFILE=$PROFILE braucht einen Bau-Rechner mit derselben Architektur ($ARCH) oder qemu-user-static"; exit 2
fi

# Doppelklick auf App-Dateien im Desktop: uapp (aus dem Kernsystem eingebunden) installiert sie
mkdir -p "$ROOT/usr/local/share/applications"
cat > "$ROOT/usr/local/share/applications/ultar-install.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Ultar App-Installer
Name[de]=Ultar App-Installer
Name[ar]=مثبّت تطبيقات Ultar
Exec=uapp install %f
NoDisplay=true
MimeType=application/x-ms-dos-executable;application/x-msdownload;application/x-msi;application/x-ole-storage;application/vnd.android.package-archive;application/vnd.debian.binary-package;application/x-appimage;application/vnd.flatpak;application/vnd.flatpak.ref;application/x-rpm;
EOF
command -v update-desktop-database >/dev/null && update-desktop-database "$ROOT/usr/local/share/applications" 2>/dev/null || true

# Größe: so groß wie nötig + 25 % Reserve (mind. 1 GB), wenn nicht vorgegeben
if [ -z "$SIZE" ]; then
  USED=$(du -sm "$ROOT" | cut -f1); SIZE="$(( USED * 125 / 100 + 512 ))M"
  [ "${USED}" -lt 600 ] && SIZE=1G
fi
IMG="$OUT/compat-$ARCH.ext4"; [ "$PROFILE" != phone ] && IMG="$OUT/compat-$ARCH-$PROFILE.ext4"
rm -f "$IMG"
truncate -s "$SIZE" "$IMG"
mkfs.ext4 -q -L ultar-compat -d "$ROOT" "$IMG"
ls -la "$IMG"
