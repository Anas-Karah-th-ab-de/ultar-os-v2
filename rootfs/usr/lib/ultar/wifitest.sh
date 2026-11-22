#!/bin/sh
# Braucht hostapd im Ubuntu-Bereich und in /data/ultar/net.toml:  ignore = ["wlan1"]
printf 'interface=wlan1\ndriver=nl80211\nssid=UltarTest\nhw_mode=g\nchannel=1\nwpa=2\nwpa_passphrase=geheim1234\nwpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\n' > /tmp/hostapd.conf
printf 'start 192.168.77.10\nend 192.168.77.50\ninterface wlan1\nlease_file /tmp/udhcpd.leases\nopt subnet 255.255.255.0\nopt lease 3600\n' > /tmp/udhcpd.conf
: > /tmp/udhcpd.leases
# Zugangspunkt und Client liegen hier im selben Kernel: Pakete mit eigener Absenderadresse zulassen
for f in all default wlan0 wlan1; do echo 1 > /proc/sys/net/ipv4/conf/$f/accept_local; echo 0 > /proc/sys/net/ipv4/conf/$f/rp_filter; done 2>/dev/null
ultar-compat run -- /usr/sbin/hostapd -B /tmp/hostapd.conf
ifconfig wlan1 192.168.77.1 netmask 255.255.255.0 up
exec udhcpd -f /tmp/udhcpd.conf
