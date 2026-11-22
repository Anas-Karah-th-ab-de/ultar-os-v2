#!/system/bin/sh
# Ultar Android-Kaefig – Steuerung. Standard: AUS. Nutzer startet/stoppt/loescht bewusst.
# Sicherheitsprinzip: Android bleibt ein isolierter Gast; der Host (Ultar) bleibt minimal/sauber.
# VERIFY: mit echtem LXC (aus dem Ubuntu-Bereich /compat) oder eigenem Namespace-Launcher verdrahten.
set -e
A=/usr/lib/ultar/android
IMG=/data/ultar/android
RUN=/run/ultar/android
SHARE=/data/ultar/android-share
PRIV=/run/ultar/privacy

priv_on() { # priv_on <key>  -> 0 wenn erlaubt
  [ -f "$PRIV" ] || return 0
  grep -q "^all=0" "$PRIV" && return 1
  grep -q "^$1=0" "$PRIV" && return 1
  return 0
}

status() {
  echo "Android-Kaefig:"
  [ -d "$RUN/rootfs/system" ] && echo "  Rootfs: gemountet" || echo "  Rootfs: nicht gemountet (aus)"
  pgrep -f "ultar-android-container" >/dev/null 2>&1 && echo "  Zustand: LAEUFT" || echo "  Zustand: GESTOPPT"
  echo "  Images: $(ls -la $IMG/system.img 2>/dev/null | awk '{print $5}')B system, $(ls -la $IMG/data.img 2>/dev/null | awk '{print $5}')B data"
  [ -f "$PRIV" ] && echo "  Datenschutz: $(tr '\n' ' ' < $PRIV)"
}

firewall() { # strenge Default-Regeln fuer das Container-Netz (TUN ultar-and0)
  # Default: nur ausgehend ins Internet ueber NAT, KEIN Zugriff aufs LAN des Hosts, kein Eingang.
  iptables -N ULTAR_AND 2>/dev/null || iptables -F ULTAR_AND
  iptables -A ULTAR_AND -d 10.0.0.0/8 -j DROP       # kein LAN
  iptables -A ULTAR_AND -d 172.16.0.0/12 -j DROP
  iptables -A ULTAR_AND -d 192.168.0.0/16 -j DROP
  iptables -A ULTAR_AND -d 169.254.0.0/16 -j DROP
  iptables -A ULTAR_AND -j ACCEPT
  # Optional (VPN-Zwang): hier Policy-Routing/Mark auf das VPN-IF setzen.
  echo "Firewall: LAN blockiert, Internet via NAT erlaubt (Default gefiltert)."
}

start() {
  priv_on net || { echo "Netzwerk ist im Datenschutz-Kontrollzentrum AUS -> Container bekommt kein Netz."; }
  [ -f "$IMG/system.img" ] || { echo "Kein Android-System-Image unter $IMG. Erst bereitstellen (siehe docs/A30-ANDROID.md)."; exit 2; }
  mkdir -p "$RUN/rootfs" "$SHARE"
  mount -o ro,loop "$IMG/system.img" "$RUN/rootfs" 2>/dev/null || true
  [ -f "$IMG/data.img" ] && mount -o loop "$IMG/data.img" "$RUN/rootfs/data" 2>/dev/null || true
  # Geraete-Freigaben nur wenn Datenschutz es erlaubt:
  priv_on cam || echo "  Kamera gesperrt -> nicht durchgereicht."
  priv_on mic || echo "  Mikrofon gesperrt -> nicht durchgereicht."
  priv_on loc || echo "  Ort gesperrt -> nicht durchgereicht."
  firewall
  sync_apks
  echo "VERIFY: Container hier mit LXC/eigenem Launcher starten (Name ultar-android-container),"
  echo "        binder/ashmem/ion/mali durchreichen, SwiftShader fuer Display, Weston fuer Ausgabe."
}

# Ausstehende APKs (von uapp nach $SHARE/pending gelegt) in den laufenden Container installieren.
# VERIFY: Ausführung im Container (lxc-attach bzw. eigener Launcher) – hier über android_exec gekapselt.
android_exec() {
  if command -v lxc-attach >/dev/null 2>&1; then lxc-attach -n ultar-android-container -- "$@"; else return 127; fi
}
sync_apks() {
  for apk in "$SHARE"/pending/*.apk; do
    [ -f "$apk" ] || continue
    mkdir -p "$RUN/rootfs/data/local/tmp" 2>/dev/null
    if cp "$apk" "$RUN/rootfs/data/local/tmp/ultar-install.apk" 2>/dev/null && android_exec /system/bin/pm install -r /data/local/tmp/ultar-install.apk; then
      echo "  installiert: $(basename "$apk")"; rm -f "$apk"
    else
      echo "  noch ausstehend: $(basename "$apk")"
    fi
  done
}

stop() {
  pkill -f "ultar-android-container" 2>/dev/null || true
  umount "$RUN/rootfs/data" 2>/dev/null || true
  umount "$RUN/rootfs" 2>/dev/null || true
  iptables -F ULTAR_AND 2>/dev/null || true
  echo "Gestoppt."
}

wipe() { # Container + alle Android-Daten loeschen (Werkseinstellung des Kaefigs)
  stop
  rm -f "$IMG/data.img"
  rm -rf "$SHARE"
  echo "Android-Daten geloescht. System-Image bleibt (zum Neuaufsetzen)."
}

case "${1:-status}" in
  status) status ;;
  start)  start ;;
  stop)   stop ;;
  restart) stop; start ;;
  wipe)   wipe ;;
  install-apk) # install-apk <datei.apk>: vormerken; Installation sobald der Container laeuft
    [ -f "${2:-}" ] || { echo "Benutzung: install-apk <datei.apk>"; exit 2; }
    mkdir -p "$SHARE/pending" && cp "$2" "$SHARE/pending/" && echo "vorgemerkt: $(basename "$2")"
    pgrep -f "ultar-android-container" >/dev/null 2>&1 && sync_apks ;;
  sync-apks) sync_apks ;;
  launch) # launch <paket>: App im Container starten
    android_exec /system/bin/monkey -p "${2:?Paket}" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1 || { echo "Android-Umgebung laeuft nicht"; exit 1; } ;;
  *) echo "ultar-android.sh status|start|stop|restart|wipe|install-apk <apk>|sync-apks|launch <paket>" ;;
esac
