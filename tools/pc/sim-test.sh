#!/system/bin/sh
# SIM-Test: Modem mit Dual-SIM-Einstellung neu starten, beide Steuerkanaele mitlesen
D=/sys/devices/platform/10000.mif_pdata
ultar-ctl stop ril; sleep 1
/usr/bin/ultar-modem --reset 2>&1 | tail -2
sleep 2; cat $D/modem_state; echo
echo 2 > $D/sim/ds_detect; echo "ds_detect=$(cat $D/sim/ds_detect)"
dmesg -c >/dev/null 2>&1
/usr/bin/ultar-modem --auto 2>&1 | tail -2
(timeout 25 cat /dev/umts_ipc1 | od -An -tx1 -v > /run/ultar/radio/ipc1.hex) &
rm -f /run/ultar/radio/ipc.log
ultar-ctl start ril; sleep 24
cat /run/ultar/radio/state | grep -E "sim=|note|operator|rat"
echo "== ipc0"; cut -c12- /run/ultar/radio/ipc.log | grep -E "RX 05|RX 08|RX 0107|RX 13" | head -30
echo "== ipc1"; head -30 /run/ultar/radio/ipc1.hex
echo "== dmesg"; dmesg | grep -iE "ds_det|dual|sim" | head -10
