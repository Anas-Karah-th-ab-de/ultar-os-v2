#!/system/bin/sh
# Abgeschottete Android-Umgebung (chroot) für Samsungs Fingerabdruck-/TEE-Dienste auf dem A30.
# Voraussetzung: /data/android/aroot.tar.gz (aus 2-packen-wsl.sh, Übertragung per WLAN, siehe Doku).
A=/data/android; R=$A/root; L=$A/log; mkdir -p $L
[ -d $R/system/bin ] || { mkdir -p $R; gzip -dc $A/aroot.tar.gz | toybox tar -x -C $R 2>/dev/null; }
cd $R
mkdir -p apex vendor dev proc sys data/log data/vendor/tee data/vendor/biometrics linkerconfig mnt/vendor/efs
mountpoint -q vendor || mount --bind /vendor vendor
mountpoint -q dev || mount --rbind /dev dev
mountpoint -q proc || mount -t proc proc proc
mountpoint -q sys || mount --rbind /sys sys
if ! mountpoint -q apex; then
  mount -t tmpfs tmpfs apex
  for d in system/apex/*; do n=$(basename $d); mkdir -p apex/$n; mount --bind $d apex/$n; done
  V=$(grep ro.vndk.version= vendor/build.prop | cut -d= -f2)
  mkdir -p apex/com.android.vndk.v$V; mount --bind system/apex/com.android.vndk.current apex/com.android.vndk.v$V
fi
mountpoint -q linkerconfig || { mount -t tmpfs tmpfs linkerconfig; chroot $R /system/bin/linkerconfig --target /linkerconfig >/dev/null 2>&1; }
# EFS nur lesend (enthält IMEI/Kalibrierung)
E=""; for p in /sys/class/block/mmcblk0p*; do n=$(grep PARTNAME $p/uevent | cut -d= -f2 | tr a-z A-Z); [ "$n" = "EFS" ] && E=$(basename $p); done
[ -n "$E" ] && { [ -e /dev/$E ] || mknod /dev/$E b $(cut -d: -f1 /sys/class/block/$E/dev) $(cut -d: -f2 /sys/class/block/$E/dev); mountpoint -q mnt/vendor/efs || mount -t ext4 -o ro,nosuid,nodev /dev/$E mnt/vendor/efs; }
# TEE-Speicher (Gatekeeper/Fingerabdruck-Objekte) muss beschreibbar sein: Kopie von efs/tee nach /data,
# darübergelegt – EFS selbst bleibt unangetastet.
if [ -d mnt/vendor/efs/tee ] && ! mountpoint -q mnt/vendor/efs/tee; then
  [ -d $A/efs-tee ] || cp -a mnt/vendor/efs/tee $A/efs-tee
  mount --bind $A/efs-tee mnt/vendor/efs/tee
fi
mkdir -p /dev/socket/iwt/ca
# Kernel-Fehler (A30, 4.4.177): der srpmb-Worker öffnet fest "/dev/block/mmcblk0rpmb" und prüft
# das Ergebnis nicht -> fehlt der Knoten, NULL-Zeiger-Panic, sobald TEEgris RPMB liest (tzts_daemon).
mkdir -p /dev/block
for b in /sys/class/block/mmcblk0*; do n=$(basename $b); [ -e /dev/block/$n ] || mknod /dev/block/$n b $(cut -d: -f1 $b/dev) $(cut -d: -f2 $b/dev); done
chroot $R /system/bin/toybox echo android-ok 2>&1 | grep -v "libc: Found"
pidof hwservicemanager >/dev/null || setsid chroot $R /system/bin/hwservicemanager </dev/null > $L/hwservicemanager.log 2>&1 &
