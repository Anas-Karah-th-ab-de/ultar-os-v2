#!/system/bin/sh
# Ultar-Dienst "gnss": GPS über Samsungs GNSS-Dienste in der abgeschotteten Android-Umgebung.
#   gpsd (lädt die Kepler-Firmware, betreibt /dev/gnss_ipc)  ->  vendor.samsung.hardware.gnss@2.0-service
#   -> gnssclient daemon (HIDL über /dev/hwbinder) -> /run/ultar/gnss.nmea
# GNSS ist nur eingeschaltet, solange die Oberfläche /run/ultar/gnss.want frisch hält (spart Akku).
# Gestartet vom Ultar-Init (rootfs/etc/ultar/services/gnss.toml). Siehe docs/A30-GNSS.md.
A=/data/android; R=$A/root; L=$A/log; G=$A/gnss
HAL=vendor.samsung.hardware.gnss@2.0-service
mkdir -p $L $G /run/ultar
[ -c /dev/gnss_ipc ] || { echo "kein /dev/gnss_ipc"; sleep 3600; exit 0; }
[ -x $A/gnssclient ] || { echo "kein gnssclient"; sleep 3600; exit 0; }
[ -d $R/system/bin ] || [ -f $A/aroot.tar.gz ] || { echo "keine Android-Umgebung"; sleep 3600; exit 0; }
up() { pidof "$1" >/dev/null; }
# Gemeinsame Grundlage (wie fp-dienst.sh); nur einer richtet sie ein
if mkdir /run/ultar/android.lock 2>/dev/null; then
  mountpoint -q $R/proc || sh $A/3-umgebung.sh </dev/null >$L/umgebung-gnss.log 2>&1
  F=""; for f in system/build.prop system/etc/prop.default vendor/default.prop vendor/build.prop vendor/odm/etc/build.prop product/build.prop system/product/build.prop; do [ -f $R/$f ] && F="$F $R/$f"; done
  up propsvc || { setsid $A/propsvc /dev/socket/property_service $L/props.txt /dev/__properties__ $F </dev/null >$L/propsvc.log 2>&1 & sleep 1; }
  up logdw || setsid $A/logdw /dev/socket/logdw $L/logcat.txt </dev/null >/dev/null 2>&1 &
  up hwservicemanager || { setsid chroot $R /system/bin/hwservicemanager </dev/null >$L/hwsm.log 2>&1 & sleep 1; }
  rmdir /run/ultar/android.lock
else
  n=0; while ! up hwservicemanager && [ $n -lt 60 ]; do sleep 1; n=$((n+1)); done
fi
# gpsd wartet auf diese Eigenschaften (sonst Endlosschleife "Waiting for sys.boot_completed")
chroot $R /system/bin/setprop sys.boot_completed 1
chroot $R /system/bin/setprop service.bootanim.exit 1
# Samsungs Einstellungen, aber ohne MEMS (braucht Androids Sensordienst, sonst hängt setCallback)
sed 's/^GlueLayer_IsMemsEnable=1/GlueLayer_IsMemsEnable=0/' /vendor/etc/gnss/gps.cfg > $G/gps.cfg
mountpoint -q $R/vendor/etc/gnss/gps.cfg || mount --bind $G/gps.cfg $R/vendor/etc/gnss/gps.cfg
mkdir -p $R/data/vendor/gps/sgee
chmod 660 /dev/gnss_ipc
# Protokolle klein halten
for f in $L/gpsd.log $L/gnsshal.log $L/gnssd.log; do [ $(wc -c < $f 2>/dev/null || echo 0) -gt 1000000 ] && : > $f; done
rm -rf $R/data/vendor/gps/nmea
while true; do
  up gpsd || { setsid chroot $R /vendor/bin/hw/gpsd -c /vendor/etc/gnss/gps.cfg </dev/null >>$L/gpsd.log 2>&1 & sleep 3; }
  up $HAL || { setsid chroot $R /vendor/bin/hw/$HAL </dev/null >>$L/gnsshal.log 2>&1 & sleep 3; }
  $A/gnssclient daemon /run/ultar/gnss.nmea /run/ultar/gnss.want </dev/null >>$L/gnssd.log 2>&1
  # Client beendet (z. B. Dienst neu gestartet): HAL neu, damit der Rückruf frisch registriert wird
  kill $(pidof $HAL) 2>/dev/null
  sleep 3
done
