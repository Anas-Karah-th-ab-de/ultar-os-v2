#!/bin/sh
echo
echo "================ Ultar OS Selbsttest ================"
echo "Kernel: $(uname -r)   Hostname: $(hostname)   Uptime: $(ultar-ctl uptime)s"
ultar-ctl status
echo "--- Ubuntu-Kompatibilität ---"
ultar-compat check
ultar-compat run -- /bin/bash -c '. /etc/os-release; echo "bash im Compat: $PRETTY_NAME"; echo "installierte .deb-Pakete: $(dpkg -l | grep -c ^ii)"; apt-get --version | head -1; ldd --version | head -1'
echo "--- Offline-Installation eines .deb (wie per USB) ---"
ultar-compat dpkg -i /opt/hello.deb >/dev/null && ultar-compat run -- hello
echo "--- cgroup heartbeat ---"
echo "memory.max=$(cat /sys/fs/cgroup/ultar/heartbeat/memory.max) pids.max=$(cat /sys/fs/cgroup/ultar/heartbeat/pids.max)"
sleep 5
echo "--- nach 5 s (heartbeat sollte neu gestartet sein) ---"
ultar-ctl status
ultar-ctl stop heartbeat
sleep 1
ultar-ctl status
echo "--- Speicher ---"
free -m
echo "Prozesse gesamt: $(ls -d /proc/[0-9]* | wc -l)"
echo "================ Selbsttest OK -> poweroff ================"
ultar-ctl poweroff
