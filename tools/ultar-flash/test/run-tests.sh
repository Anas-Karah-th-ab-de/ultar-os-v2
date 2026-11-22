#!/usr/bin/env bash
# Prüft die Logik von Ultar Flash mit Geräte-Attrappen (kein echtes Handy, kein Windows): mono + test/fake/*
set -u
cd "$(dirname "$0")/.."
W=$(mktemp -d); export FAKE_DEV=$W/dev ULTARFLASH_TOOLS=$PWD/test/fake ULTARFLASH_USE_HEIMDALL=1
BN=$FAKE_DEV/dev/block/by-name
mkdir -p $BN $FAKE_DEV/vendor/firmware $FAKE_DEV/vendor/lib/modules $FAKE_DEV/vendor/etc/wifi $FAKE_DEV/proc $FAKE_DEV/sys
for p in boot recovery efs sec_efs param system bootloader; do head -c $((RANDOM % 9000 + 20000)) /dev/urandom > $BN/$p; done
echo fw > $FAKE_DEV/vendor/firmware/bcmdhd.bin; echo ko > $FAKE_DEV/vendor/lib/modules/dhd.ko; echo cal > $FAKE_DEV/vendor/etc/wifi/nvram.txt
echo "console=ram" > $FAKE_DEV/proc/cmdline
ORIG=$(sha256sum < $BN/boot)
X="mono UltarFlash.exe --cli"; pass=0; fail=0
t() { local name=$1 want=$2; shift 2; out=$("$@" 2>&1); rc=$?; if [ $rc -eq $want ]; then pass=$((pass+1)); echo "ok   $name"; else fail=$((fail+1)); echo "FAIL $name (rc=$rc, erwartet $want)"; echo "$out" | tail -5; fi; }
has() { if echo "$out" | grep -q "$1"; then pass=$((pass+1)); echo "ok     enthält: $1"; else fail=$((fail+1)); echo "FAIL   fehlt: $1"; echo "$out" | tail -5; fi; }
B=$W/backup; P=$W/pkg; mkdir -p $P
t "Gerät erkennen" 0 $X detect; has "model=SM-A305F device=a30 battery=80 root=True unlocked=True"
t "Treiber sichern" 0 $X drivers $B; has "files=3"
[ -f $B/drivers/vendor_firmware/bcmdhd.bin ] && echo "ok     Treiberdatei vorhanden" || { echo "FAIL   Treiberdatei fehlt"; fail=$((fail+1)); }
head -c 30000 /dev/urandom > $P/boot.img
printf 'device=a30,a30dd\nimage.boot=boot.img\nsha256.boot=%s\n' $(sha256sum < $P/boot.img | cut -d" " -f1) > $P/ultar-package.txt
t "Flashen ohne Sicherung gesperrt" 2 $X check $P $B; has "Keine Vollsicherung"
t "Vollsicherung" 0 $X full $B; has "Prüfsumme stimmt"
grep -q "^efs|" $B/manifest.txt && cmp -s $B/partitions/efs.img $BN/efs && echo "ok     EFS gesichert und identisch" || { echo "FAIL   EFS"; fail=$((fail+1)); }
grep -q "^bootloader|" $B/manifest.txt && { echo "FAIL   Bootloader unnötig gesichert?"; } ; true
t "Prüfung bestanden" 0 $X check $P $B
t "Flashen ohne Bestätigung gesperrt" 2 $X flash $P $B
echo 30 > $FAKE_DEV/battery; t "Akku zu schwach" 2 $X check $P $B; has "Akku bei 30"; echo 80 > $FAKE_DEV/battery
echo 1 > $FAKE_DEV/locked; t "Bootloader gesperrt" 2 $X check $P $B; has "Bootloader ist gesperrt"; echo 0 > $FAKE_DEV/locked
sed -i 's/device=a30,a30dd/device=a50/' $P/ultar-package.txt; t "Falsches Gerät" 2 $X check $P $B; has "Paket ist für"; sed -i 's/device=a50/device=a30,a30dd/' $P/ultar-package.txt
echo "image.bootloader=boot.img" >> $P/ultar-package.txt; t "Bootloader schreiben verboten" 2 $X check $P $B; has "darf nicht geschrieben werden"; sed -i '/image.bootloader/d' $P/ultar-package.txt
echo x >> $P/boot.img; t "Beschädigtes Abbild" 2 $X check $P $B; has "beschädigt"; truncate -s -2 $P/boot.img
BL=$(sha256sum < $BN/bootloader); EF=$(sha256sum < $BN/efs)
t "Flashen" 0 $X flash $P $B --yes SM-A305F
cmp -s $P/boot.img $BN/boot && echo "ok     neues Boot-Abbild auf dem Gerät" || { echo "FAIL   Boot-Abbild nicht geschrieben"; fail=$((fail+1)); }
[ "$BL" = "$(sha256sum < $BN/bootloader)" ] && [ "$EF" = "$(sha256sum < $BN/efs)" ] && echo "ok     Bootloader und EFS unberührt" || { echo "FAIL   Bootloader/EFS verändert"; fail=$((fail+1)); }
t "Wiederherstellen ohne Bestätigung gesperrt" 2 $X restore $B
t "Wiederherstellen" 0 $X restore $B --yes SM-A305F
[ "$ORIG" = "$(sha256sum < $BN/boot)" ] && echo "ok     Original-Boot wieder auf dem Gerät" || { echo "FAIL   Original nicht wiederhergestellt"; fail=$((fail+1)); }
echo kaputt >> $B/partitions/boot.img; t "Beschädigte Sicherung wird nicht zurückgeschrieben" 1 $X restore $B --yes SM-A305F; has "beschädigt"
# ---- Paket aus Firmware bauen (nachgebaute AP-Datei), flashen ohne Root-Sicherung als Testgerät, wiederherstellen
FW=$W/fw; AP=$(python3 test/make-firmware.py $FW); NB=$W/nb; PK=$NB/paket
cp $FW/boot.img $BN/boot; cp $FW/vbmeta.img $BN/vbmeta       # Gerät trägt den Stand der Firmware
t "Paket aus Firmware bauen" 0 $X build "$AP" $FW/initramfs.cpio.gz $PK $NB; has "an 2 Stelle(n) umbenannt"; has "vbmeta: durch leeres vbmeta ersetzt"
out=$(python3 test/check-package.py $FW $PK); echo "$out"; pass=$((pass + $(echo "$out" | grep -c "^ok"))); fail=$((fail + $(echo "$out" | grep -c "^FAIL")))
cmp -s $NB/partitions/boot.img $FW/boot.img && grep -q "^boot|.*|1$" $NB/manifest.txt && echo "ok     Original als Wiederherstellung abgelegt" || { echo "FAIL   Original nicht abgelegt"; fail=$((fail+1)); }
t "Ohne EFS-Sicherung gesperrt" 2 $X check $PK $NB; has "EFS-Partition"
t "Als Testgerät erlaubt" 0 $X check $PK $NB --testdevice; has "WARNUNG: keine Sicherung der EFS"
EF2=$(sha256sum < $BN/efs)
t "Paket flashen (Testgerät)" 0 $X flash $PK $NB --testdevice --yes SM-A305F
cmp -s $PK/boot.img $BN/boot && cmp -s $PK/vbmeta.img $BN/vbmeta && [ "$EF2" = "$(sha256sum < $BN/efs)" ] && echo "ok     boot und vbmeta geschrieben, EFS unberührt" || { echo "FAIL   Schreiben"; fail=$((fail+1)); }
t "Original wiederherstellen" 0 $X restore $NB --yes SM-A305F
cmp -s $FW/boot.img $BN/boot && cmp -s $FW/vbmeta.img $BN/vbmeta && echo "ok     Original-boot und -vbmeta zurück" || { echo "FAIL   Wiederherstellung"; fail=$((fail+1)); }
# ---- Eigener Schreibweg (Download-Modus über COM-Port; hier Geräte-Attrappe über TCP)
python3 test/fake/loke.py 8094 > $W/loke.log 2>&1 & LP=$!; sleep 1
export ULTARFLASH_USE_HEIMDALL=0 ULTARFLASH_LOKE_TCP=127.0.0.1:8094
EF3=$(sha256sum < $BN/efs); BL3=$(sha256sum < $BN/bootloader)
t "Eigener Schreibweg: flashen" 0 $X flash $PK $NB --testdevice --yes SM-A305F; has "Partitionstabelle gelesen: 8 Einträge"; has "Schreibe BOOT"
cmp -s $PK/boot.img $BN/boot && cmp -s $PK/vbmeta.img $BN/vbmeta && echo "ok     boot und vbmeta bytegenau angekommen" || { echo "FAIL   Daten nicht bytegenau"; cat $W/loke.log; fail=$((fail+1)); }
[ "$EF3" = "$(sha256sum < $BN/efs)" ] && [ "$BL3" = "$(sha256sum < $BN/bootloader)" ] && [ "$(cat $FAKE_DEV/state)" = adb ] && echo "ok     EFS/Bootloader unberührt, Handy neu gestartet" || { echo "FAIL   Schutz/Neustart"; fail=$((fail+1)); }
t "Eigener Schreibweg: wiederherstellen" 0 $X restore $NB --yes SM-A305F
cmp -s $FW/boot.img $BN/boot && cmp -s $FW/vbmeta.img $BN/vbmeta && echo "ok     Original wieder da (34 MiB in 2 Abschnitten)" || { echo "FAIL   Wiederherstellung"; fail=$((fail+1)); }
[ -s $W/loke.log ] && { echo "FAIL   Attrappe meldet Protokollfehler"; cat $W/loke.log; fail=$((fail+1)); } || echo "ok     keine Protokollfehler"
kill $LP; python3 test/fake/loke.py 8094 nak-end > $W/loke2.log 2>&1 & LP=$!; sleep 1
t "Eigener Schreibweg: Schreibfehler des Handys wird gemeldet" 1 $X flash $PK $NB --testdevice --yes SM-A305F; has "Fehler beim Schreiben von BOOT"
kill $LP; echo adb > $FAKE_DEV/state
t "Eigener Schreibweg: kein Gerät im Download-Modus" 1 env ULTARFLASH_LOKE_WAIT=3 $X flash $PK $NB --testdevice --yes SM-A305F; has "Es wurde nichts geschrieben\|Kein Samsung-Gerät"
export ULTARFLASH_USE_HEIMDALL=1; unset ULTARFLASH_LOKE_TCP
echo adb > $FAKE_DEV/state; (echo neu | gzip > $W/neu.cpio.gz); t "Neues Kernsystem: Paket wird selbst neu gebaut" 0 $X check $PK $NB --testdevice --ramdisk $W/neu.cpio.gz; has "baue das Paket neu"; has "OK"
t "Gleiches Kernsystem: kein Neubau" 0 $X check $PK $NB --testdevice --ramdisk $W/neu.cpio.gz; out=$(echo "$out" | grep -c "baue das Paket neu"); has "^0$"
head -c 5000 /dev/urandom > $W/kaputt.img; t "Kein Boot-Abbild wird abgelehnt" 1 $X build $W/kaputt.img $FW/initramfs.cpio.gz $W/p2 $W/b2; has "Kein Android-Boot-Abbild"

# ---- Übersetzung: Meldungen des Kerns dürfen auf Arabisch/Englisch nicht deutsch bleiben
out=$(printf 'Sichere /vendor/etc/wifi …\n  • Akku bei 30 %% – mindestens 50 %% nötig.\nLese Partition efs …\nFehler: Boot-Abbild abgeschnitten.\n' | $X translate ar | grep -v "^mode=")
pass=$((pass+1)); if echo "$out" | grep -qE "Sichere|Akku|Lese|Fehler|abgeschnitten"; then fail=$((fail+1)); pass=$((pass-1)); echo "FAIL Arabisch enthält deutschen Text"; echo "$out"; else echo "ok   Meldungen vollständig auf Arabisch"; fi
out=$(printf 'Paket fertig: C:/x\n' | $X translate en | grep -v "^mode="); has "Package ready: C:/x"

# ---- Zweites, abgeschaltetes Gerät in der Liste (Emulator offline): gezielt das echte Gerät ansprechen
touch $FAKE_DEV/multi; echo adb > $FAKE_DEV/state
t "Mehrere Geräte: echtes Gerät gewählt" 0 $X detect; has "model=SM-A305F device=a30 battery=80 root=True"
t "Mehrere Geräte: Treiber sichern" 0 $X drivers $W/multi; has "files=3"
# ---- Geführter Ablauf: Handy erst nicht da, dann ohne Freigabe, dann freigegeben
echo none > $FAKE_DEV/state; G=$W/gefuehrt
( sleep 4; echo unauthorized > $FAKE_DEV/state; sleep 4; echo adb > $FAKE_DEV/state ) &
t "Geführter Ablauf" 0 $X guide $G; has "SCHRITT 2 g_prepare"; has "SCHRITT 3 g_authorize"; has "SCHRITT 4 g_drivers"; has "SCHRITT 5 g_full"; has "SCHRITT 6 g_done"
[ -f $G/a30-R58M123ABC/manifest.txt ] && [ -f $G/a30-R58M123ABC/drivers/vendor_firmware/bcmdhd.bin ] && echo "ok     Sicherung im Geräteordner" || { echo "FAIL   Sicherung fehlt"; fail=$((fail+1)); }
# Werkzeuge fehlen -> werden geladen und entpackt (Zip von einem lokalen Server statt von Google)
Z=$W/zip; mkdir -p $Z/platform-tools; cp test/fake/adb $Z/platform-tools/adb; (cd $Z && zip -qr pt.zip platform-tools); (cd $Z && python3 -m http.server 8095 >/dev/null 2>&1 &) ; sleep 1
mkdir -p $W/home $W/leer; HOME=$W/home ULTARFLASH_TOOLS=$W/leer ULTARFLASH_PLATFORM_TOOLS_URL=http://127.0.0.1:8095/pt.zip PATH=/usr/bin:/bin mono UltarFlash.exe --cli guide $W/g2 > $W/g2.log 2>&1 &
GP=$!; sleep 6; kill $GP 2>/dev/null; pkill -f "http.server 8095"; out=$(cat $W/g2.log); has "g_toolsdl"
ls $W/home/UltarFlash/tools/adb >/dev/null 2>&1 || ls $W/home/Documents/UltarFlash/tools/adb >/dev/null 2>&1 && echo "ok     adb geladen und entpackt" || { echo "FAIL   adb nicht entpackt"; find $W/home | head; fail=$((fail+1)); }

# ---- Diagnose vom Handy (Attrappe über TCP)
python3 -c "
import socket
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1',8097)); s.listen(1)
c,_=s.accept(); c.sendall('muell\r\n===== ULTAR-DIAGNOSE ANFANG =====\r\nversion: 0.6.0\r\nuptime: 12.3\r\nusb: configfs\r\nfb0: name=decon\r\n===== ULTAR-DIAGNOSE ENDE =====\r\n'.encode()); import time; time.sleep(1)
" & sleep 1
export ULTARFLASH_DIAG_TCP=127.0.0.1:8097
t "Diagnose gelesen" 0 $X diag $W/diag; has "Ultar OS läuft auf dem Handy"; has "usb: configfs"
grep -q "fb0: name=decon" $W/diag/diagnose-*.txt && echo "ok     Diagnose gespeichert" || { echo "FAIL   Diagnose nicht gespeichert"; fail=$((fail+1)); }
t "Diagnose ohne Gerät" 1 $X diag $W/diag2; has "meldet sich nicht"
unset ULTARFLASH_DIAG_TCP

# ---- Aktualisierung aus Releases (gegen scripts/release-stub.py)
R=$W/rel; mkdir -p $R; cp UltarFlash-*.msi $R/UltarFlash-9.9.9.msi; (cd $R && sha256sum UltarFlash-9.9.9.msi > SHA256SUMS)
python3 ../../scripts/release-stub.py $R 8096 v9.9.9 & SP=$!; sleep 1; export ULTARFLASH_API=http://127.0.0.1:8096 HOME=$W
t "Update gefunden und geprüft geladen" 0 $X update --download; has "newer=True"; has "Prüfsumme stimmt"
echo "0000  UltarFlash-9.9.9.msi" > $R/SHA256SUMS; t "Update mit falscher Prüfsumme abgelehnt" 1 $X update --download; has "stimmt nicht"
rm $R/SHA256SUMS; t "Update ohne Prüfsummen abgelehnt" 1 $X update --download; has "keine SHA256SUMS"
kill $SP; python3 ../../scripts/release-stub.py $R 8096 v0.1.0 & SP=$!; sleep 1
t "Ältere Version wird nicht angeboten" 0 $X update; has "newer=False"; kill $SP
rm -rf "$W"; echo "bestanden: $pass, fehlgeschlagen: $fail"; [ $fail -eq 0 ]
