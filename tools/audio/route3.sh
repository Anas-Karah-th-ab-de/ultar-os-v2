#!/system/bin/sh
A=/data/audctl
# pcm0 -> SIFS2 -> UAIF SPK2 (default vom Treiber getaktet)
$A setenum "ABOX SPUS OUT0" SIFS2
$A setenum "ABOX SIFS2" "SPUS OUT0"
$A setenum "ABOX UAIF SPK2" SIFS2
$A setname "HP HP On" 1
$A setname "Speaker Mute" 0
$A setname "DAC Soft Mute" 0
$A set 76 150
echo "== SIFS2/SPK2, Ton =="
/usr/bin/ultar-shell --beep 2>&1 | tail -2
