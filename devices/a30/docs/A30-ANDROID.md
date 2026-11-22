# Ultar – Android-App-Laufzeit (isolierter Kaefig) auf dem A30

Ziel: echte APKs (WhatsApp, Spotify, YouTube …) als vollwertige Apps, damit Dienste Ultar als
Hauptgeraet akzeptieren – ABER als strikt isolierter, abschaltbarer Gast. Ultar erbt Androids
Angriffsflaeche NICHT: der Host bleibt minimal/sauber, Android sitzt im Kaefig.

## Machbarkeit am A30 (Kernel 4.4.177, Exynos7885, verifiziert in /home/claude/a30k)
VORHANDEN (Kern der Laufzeit funktioniert):
- **Binder**: CONFIG_ANDROID_BINDER_IPC=y, DEVICES="binder,hwbinder,vndbinder" (Treble-tauglich) ✓
- **ashmem** (CONFIG_ASHMEM=y), **ion** (CONFIG_ION/ION_EXYNOS=y), dma-buf ✓
- **Namespaces** pid/net/mount/ipc/uts=y, **cgroups**/freezer=y ✓
- **Mali-GPU** (b_r19p0/r25/r26, MALI_REAL_HW) + **FUSE**, **TUN**, **NF_NAT/MASQUERADE** ✓
FEHLT / EINSCHRAENKUNG (ehrlich):
- **USER_NS=n** → keine unprivilegierten User-Namespaces → Container laeuft privilegiert;
  Ausgleich: seccomp + Capability-Drop + alle anderen Namespaces + strenge Mounts.
- **veth=n**, **BRIDGE=n** → kein Standard-Waydroid-Netz; Loesung: TUN+NAT (eigenes IF) oder
  Host-Netz nur mit Firewall. (TUN+NAT ist der isolierte Default.)
- **OVERLAY_FS=n, SQUASHFS=n** → Rootfs als ext4-Images (system.img ro + data.img rw) statt overlay/squashfs.
- **DRM/KMS=n** → kein gbm/drm-hwcomposer; Display zunaechst per **SwiftShader** (Software-GL) →
  Weston (fbdev). 2D-Apps (WhatsApp/Telegram) laufen; Video (YouTube/Spotify) ist ohne GPU/Codec-
  Beschleunigung langsam. Volle GPU-Beschleunigung braucht erheblichen Mehraufwand (Mali-gralloc/
  hwcomposer-Bruecke ohne DRM) und ist ggf. erst mit Kernel-Aenderung sauber.

FAZIT: **Ja, machbar** fuer die Laufzeit + 2D-Apps (Waydroid-Stil, privilegierter LXC, ext4-Images,
TUN-NAT, SwiftShader). Video-Apps zunaechst eingeschraenkt. Ohne eigene Kernel-Aenderung bleibt der
gemeinsame Kernel das Restrisiko (siehe Sicherheitsmodell).

## Sicherheitsmodell (erste Design-Anforderung)
- **Isolation**: eigener pid/net/mount/ipc/uts-Namespace; Capabilities gedroppt (android.conf);
  seccomp-Filter (seccomp.txt) sperrt host-nahe Syscalls (mount, module, kexec, reboot, setns,
  unshare, bpf, perf …). KEIN Bind von Ultars echtem /data/ /efs/ Secrets – nur EIN Austauschordner
  /data/ultar/android-share. Rootfs sind eigene Images.
- **Netz kontrolliert**: eigenes TUN-IF mit NAT; Firewall-Default: LAN (10/8,172.16/12,192.168/16,
  169.254/16) blockiert, nur Internet via NAT; Option, allen Verkehr ueber VPN zu zwingen; kein Eingang.
- **Datenschutz-Kopplung**: ultar-android.sh liest /run/ultar/privacy (vom Kontrollzentrum). Bei
  Netz/Kamera/Mikro/Ort = AUS bekommt der Container das jeweilige Geraet nicht.
- **Keine Google-Dienste**: System-Image ohne GApps; microG nur als optionale, abschaltbare Wahl.
- **Standard AUS**: Kaefig startet nie von selbst; Nutzer startet bewusst. **Jederzeit wipebar**
  (`ultar-android.sh wipe` loescht Container-Daten) – Werksreset des Kaefigs.
- **Restrisiko (ehrlich)**: gemeinsamer Kernel (kein USER_NS) + GPU-Passthrough = theoretische
  Kernel-Angriffsflaeche. Darum: privilegiert, aber seccomp+capdrop; Android bleibt abschaltbar und
  loeschbar. Wer maximale Sicherheit will, laesst den Kaefig aus – Ultar funktioniert ohne ihn.

## Android-System-Image
- Generisches **arm64 GSI** (z.B. LineageOS GSI / Waydroid-Image), OHNE GApps. Da Ultar den
  Stock-Kernel + Stock-/vendor nutzt, koennen Android-HALs (gralloc, codec, audio) aus /vendor
  verwendet werden; Display aber SwiftShader (kein DRM). Image als system.img nach /data/ultar/android/.
- VERIFY am Geraet: welche /vendor-HAL-Blobs der Container nutzen kann vs. SwiftShader-only.

## Geruest (neue Dateien, nichts Geteiltes angefasst)
- tools/androidcheck.c → /mnt/user-data/outputs/androidcheck: prueft am Geraet binder/ashmem/ion/mali,
  SELinux, welche Namespaces/seccomp moeglich sind, overlay/squashfs, /run/ultar/privacy, Images.
- rootfs/usr/lib/ultar/android/android.conf (gehaertete LXC-Vorlage), seccomp.txt, ultar-android.sh
  (status/start/stop/restart/wipe/install-apk, Firewall, Datenschutz-Kopplung).

## Geraete-Testreihenfolge (fuer den Koordinator)
1. `upush androidcheck /data/androidcheck; ucmd "/data/androidcheck"` → zeigt binder/hwbinder/vndbinder,
   ashmem, ion, mali0 als [DA] (erwartet, Android-Kernel), USER_NS NEIN, seccomp-Status, FS-Lage.
2. /vendor mounten; pruefen welche HAL-Blobs/Codec-Nodes da sind (`ls /vendor/lib*/hw`).
3. LXC aus dem Ubuntu-Bereich bereitstellen (/compat: `apt install lxc`) ODER minimalen Namespace-
   Launcher nutzen; android.conf mit den echten Geraete-Majoren fuellen (aus `ls -l /dev/binder …`).
4. arm64-GSI (ohne GApps) als /data/ultar/android/system.img bereitstellen; data.img (ext4, leer) anlegen.
5. `ultar-android.sh start` → Container headless hochfahren; `logcat`/`dmesg` pruefen; dann `adb` im
   Container, erste APK: `pm install` (z.B. ein kleines 2D-App zuerst, dann WhatsApp). Display via
   SwiftShader → Weston-Fenster; Isolation + Firewall + Datenschutz-Schalter verifizieren.
