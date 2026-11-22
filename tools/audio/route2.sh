#!/system/bin/sh
A=/data/audctl
# pcm0 (SPUS0) -> SIFS1 -> UAIF SPK3 -> Codec UAIF3 -> Lautsprecher
$A setenum "ABOX SPUS OUT0" SIFS1
$A setenum "ABOX SIFS1" "SPUS OUT0"
$A setenum "ABOX UAIF SPK3" SIFS1
$A setenum "ABOX NSRC3" UAIF3
# Codec: Lautsprecher an, Mute aus, Gain
$A setname "HP HP On" 1
$A setname "Speaker Mute" 0
$A setname "DAC Soft Mute" 0
$A set 76 150
$A setname "Speaker EQ" 1
echo "== route gesetzt, spiele Ton =="
ULTAR_AUDIO_DEBUG=1 /usr/bin/ultar-shell --beep 2>&1
