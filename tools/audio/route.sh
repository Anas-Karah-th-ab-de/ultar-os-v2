#!/system/bin/sh
A=/data/audctl
echo "== route-spkamp-playback =="
$A setname "ERAP info Bypass On" 1
$A setname "ERAP info DSM On" 0
$A setenum "ABOX NSRC3" UAIF3
$A setenum "ABOX SPUS OUT7" SIFS1
$A setenum "ABOX SIFS1" "SPUS OUT7"
$A setenum "ABOX UAIF SPK3" SIFS1
echo "== dev-speaker + unmute + gain =="
$A setname "HP HP On" 1
$A setname "Speaker Mute" 0
$A set 76 150
$A setname "DAC Soft Mute" 0
echo "== Testton pcm7 dann pcm0 =="
$A tone 7 440 700
$A tone 0 440 700
