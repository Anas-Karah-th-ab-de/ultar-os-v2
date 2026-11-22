#!/system/bin/sh
# Ultar-Dienst „fingerabdruck": abgeschottete Android-Umgebung + Samsungs TEE-/Fingerabdruckdienste
# und der Vermittler `fpclient daemon` (Socket /run/ultar/fp.sock für die Oberfläche).
# Wird vom Ultar-Init gestartet (rootfs/etc/ultar/services/fingerabdruck.toml) und bei Ende neu gestartet.
A=/data/android; R=$A/root; L=$A/log
HAL=vendor.samsung.hardware.biometrics.fingerprint@3.0-service
mkdir -p $L /run/ultar
[ -d $R/system/bin ] || [ -f $A/aroot.tar.gz ] || { echo "keine Android-Umgebung"; sleep 3600; exit 0; }
sh $A/3-umgebung.sh </dev/null >$L/umgebung.log 2>&1
cd $R
F=""; for f in system/build.prop system/etc/prop.default vendor/default.prop vendor/build.prop vendor/odm/etc/build.prop product/build.prop system/product/build.prop; do [ -f $R/$f ] && F="$F $R/$f"; done
up() { pidof "$1" >/dev/null; }
up propsvc || { setsid $A/propsvc /dev/socket/property_service $L/props.txt /dev/__properties__ $F </dev/null >$L/propsvc.log 2>&1 & sleep 1; }
up logdw || setsid $A/logdw /dev/socket/logdw $L/logcat.txt </dev/null >/dev/null 2>&1 &
up hwservicemanager || { setsid chroot $R /system/bin/hwservicemanager </dev/null >$L/hwsm.log 2>&1 & sleep 1; }
up tzdaemon || { setsid chroot $R /vendor/bin/tzdaemon </dev/null >$L/tzdaemon.log 2>&1 & sleep 5; }
up tzts_daemon || { setsid chroot $R /vendor/bin/tzts_daemon </dev/null >$L/tzts.log 2>&1 & sleep 4; }
up android.hardware.keymaster@4.0-service || setsid chroot $R /vendor/bin/hw/android.hardware.keymaster@4.0-service </dev/null >$L/km.log 2>&1 &
up android.hardware.gatekeeper@1.0-service || setsid chroot $R /vendor/bin/hw/android.hardware.gatekeeper@1.0-service </dev/null >$L/gk.log 2>&1 &
mkdir -p $R/data/vendor_de/0/fpdata $R/data/vendor/biometrics $R/data/vendor/gatekeeper
# Logdatei klein halten
[ $(wc -c < $L/logcat.txt 2>/dev/null || echo 0) -gt 2000000 ] && : > $L/logcat.txt
while true; do
  up $HAL || { setsid chroot $R /vendor/bin/hw/$HAL </dev/null >>$L/hal.log 2>&1 & sleep 3; }
  # Der Vermittler hält die Verbindung; endet er, beendet sich auch der HAL (Samsung) → neu starten
  $A/fpclient daemon /run/ultar/fp.sock $A/gk-ultar.handle </dev/null >>$L/fpd.log 2>&1
  rm -f /run/ultar/fp.sock
  sleep 2
done
