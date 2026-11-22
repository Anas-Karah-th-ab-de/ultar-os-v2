#!/bin/bash
# Ultar-Kern Hardware-Inventur (nur lesen)
O=/root/ultar-hw; rm -rf $O; mkdir -p $O; cd $O
lspci -vvnn > lspci.txt 2>&1
lspci -nnk > lspci-k.txt
lspci -tv > lspci-tree.txt
lsusb -v > lsusb.txt 2>/dev/null; lsusb -t > lsusb-t.txt
lsmod > lsmod.txt
dmesg > dmesg.txt
cp /proc/cpuinfo cpuinfo.txt; cp /proc/iomem iomem.txt; cp /proc/interrupts irq.txt; cp /proc/cmdline cmdline.txt
mkdir acpi; for t in /sys/firmware/acpi/tables/*; do [ -f "$t" ] && cp "$t" acpi/ 2>/dev/null; done
command -v dmidecode >/dev/null && dmidecode > dmi.txt 2>&1
ip -d link > ip.txt
for m in $(lsmod | awk 'NR>1{print $1}'); do for f in $(modinfo -F firmware $m 2>/dev/null); do echo "$m $f"; done; done > fw-declared.txt
grep -iE 'firmware|loaded|guc|huc|dmc|ucode' dmesg.txt > fw-dmesg.txt
# tatsaechlich benoetigte Firmware-Dateien (vorhanden?)
awk '{print $2}' fw-declared.txt | sort -u | while read f; do for p in /lib/firmware/$f /lib/firmware/$f.zst /lib/firmware/$f.xz; do [ -e "$p" ] && echo "OK $p"; done; done > fw-present.txt
ls -la /dev/dri /dev/accel > dri.txt 2>&1
command -v nvme >/dev/null && nvme list > nvme.txt 2>&1
cat /proc/asound/cards > audio.txt 2>&1; ls /sys/class/sound >> audio.txt
for d in /sys/bus/i2c/devices/*; do echo "$(basename $d) $(cat $d/name 2>/dev/null) $(cat $d/modalias 2>/dev/null)"; done > i2c.txt
for d in /sys/bus/platform/devices/*; do echo "$(basename $d) $(readlink $d/driver | xargs -r basename)"; done > platform.txt
for d in /sys/bus/acpi/devices/*; do echo "$(basename $d) $(cat $d/hid 2>/dev/null) $(cat $d/status 2>/dev/null)"; done > acpi-devs.txt
for h in /sys/class/hwmon/*; do echo "$h $(cat $h/name)"; done > hwmon.txt
ls /sys/class/thermal > thermal.txt; cat /sys/class/thermal/thermal_zone*/type >> thermal.txt 2>/dev/null
cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_driver 2>/dev/null | sort | uniq -c > cpufreq.txt
cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences >> cpufreq.txt 2>/dev/null
for c in /sys/devices/system/cpu/cpu[0-9]*; do echo "$(basename $c) $(cat $c/cpufreq/cpuinfo_max_freq 2>/dev/null) $(cat $c/topology/core_type 2>/dev/null)"; done >> cpufreq.txt
ls /sys/class/net > net.txt; for n in /sys/class/net/*; do echo "$(basename $n) $(readlink $n/device/driver | xargs -r basename)"; done >> net.txt
cat /sys/class/drm/card*/device/uevent > gpu.txt 2>/dev/null
mokutil --sb-state > sb.txt 2>&1; ls /sys/firmware/efi >/dev/null 2>&1 && echo EFI >> sb.txt
cp /boot/config-$(uname -r) kconfig.txt 2>/dev/null
tar czf /root/ultar-hw.tgz -C /root ultar-hw
ls -la /root/ultar-hw.tgz; wc -l lspci-k.txt lsmod.txt fw-declared.txt fw-present.txt dmesg.txt
