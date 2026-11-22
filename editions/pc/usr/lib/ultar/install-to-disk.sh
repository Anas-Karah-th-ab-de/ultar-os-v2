#!/bin/sh
# Ultar OS vom USB-Stick fest auf eine interne Platte installieren (PC/Laptop/Server).
#   install-to-disk.sh                     verfügbare Platten anzeigen
#   install-to-disk.sh /dev/nvme0n1 --yes-erase-nvme0n1
# Kopiert Startpartition (ULTAR-EFI) und Ubuntu-Bereich (ultar-compat) 1:1, legt eine neue, leere
# Datenpartition (ultar-data) über den ganzen Rest an. ALLE DATEN AUF DER ZIELPLATTE GEHEN VERLOREN.
# Danach den USB-Stick abziehen (sonst gibt es die Partitionsnamen doppelt) und neu starten.
set -e
C="ultar-compat run --"
src_part() { awk -v m="$1" '$2==m{print $1}' /proc/mounts | head -1; }
SRC_COMPAT=$(src_part /compat); SRC_EFI=$(src_part /boot/efi)
[ -b "$SRC_COMPAT" ] || { echo "Ubuntu-Bereich (/compat) ist nicht von einer Partition eingebunden"; exit 2; }
SRC_DISK=/dev/$(basename "$(readlink -f /sys/class/block/${SRC_COMPAT##*/}/..)")
if [ -z "$1" ]; then
  echo "Start-Datenträger: $SRC_DISK"; echo "Mögliche Ziele:"
  for b in /sys/block/*; do n=${b##*/}; case $n in loop*|ram*|zram*|sr*|dm-*) continue;; esac
    [ "/dev/$n" = "$SRC_DISK" ] && continue
    echo "  /dev/$n  $(( $(cat $b/size) / 2097152 )) GB  $(cat $b/device/model 2>/dev/null)"; done
  echo "Installieren: $0 /dev/<name> --yes-erase-<name>"; exit 0
fi
DST=$1; N=${DST##*/}
[ "$2" = "--yes-erase-$N" ] || { echo "Zur Bestätigung: $0 $DST --yes-erase-$N"; exit 2; }
[ -b "$DST" ] || { echo "$DST ist kein Blockgerät"; exit 2; }
[ "$DST" != "$SRC_DISK" ] || { echo "Ziel ist der Start-Datenträger"; exit 2; }
grep -q "^$DST" /proc/mounts && { echo "$DST ist eingebunden – abbrechen"; exit 2; }
sz() { echo $(( $(cat /sys/class/block/${1##*/}/size) / 2048 )); }   # MiB
E=$(sz "$SRC_EFI"); K=$(sz "$SRC_COMPAT")
echo "Lösche und partitioniere $DST …"
$C sgdisk -Z "$DST" >/dev/null 2>&1 || true
$C sgdisk -o -n 1:2048:+${E}M -t 1:EF00 -c 1:ULTAR-EFI -n 2:0:+${K}M -t 2:8300 -c 2:ultar-compat -n 3:0:0 -t 3:8300 -c 3:ultar-data "$DST"
$C partx -u "$DST" 2>/dev/null || true; sleep 2
case $N in nvme*|mmcblk*|loop*) P=${DST}p;; *) P=$DST;; esac
echo "Kopiere Startpartition …"; dd if="$SRC_EFI" of=${P}1 bs=4M conv=fsync 2>/dev/null
echo "Kopiere Ubuntu-Bereich …"
sync; mount -o remount,ro /compat 2>/dev/null && RO=1 || RO=0
dd if="$SRC_COMPAT" of=${P}2 bs=4M conv=fsync 2>/dev/null || { [ $RO = 1 ] && mount -o remount,rw /compat; exit 1; }
[ $RO = 1 ] && mount -o remount,rw /compat
# Kopie eines eingebundenen Dateisystems: sicherheitshalber prüfen (Ziel ist nicht eingebunden)
$C e2fsck -fy ${P}2 >/dev/null 2>&1 || true
echo "Lege Datenpartition an …"; $C mkfs.ext4 -q -F -L ultar-data ${P}3
echo "Fertig. USB-Stick abziehen und neu starten."
