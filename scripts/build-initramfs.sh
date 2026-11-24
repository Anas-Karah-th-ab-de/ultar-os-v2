#!/usr/bin/env bash
# Baut das Ultar-Kernsystem als initramfs.
#   ARCH=x86_64|aarch64   DEBUG=1 (busybox für Shell/Selbsttest)   OUT=out/
#   EDITION=phone|desktop|server   (Ausgabe; desktop/server bekommen die PC-Konfiguration aus editions/)
#   MODULES="virtio-gpu virtio_input" MODULES_DIR=<entpacktes linux-modules-Paket>
#     -> Kernel-Module samt Abhängigkeiten ins Image (z. B. für QEMU mit Ubuntu-Kernel)
set -euo pipefail
cd "$(dirname "$0")/.."
ARCH=${ARCH:-x86_64}; DEBUG=${DEBUG:-1}; OUT=${OUT:-out}; EDITION=${EDITION:-phone}
case "$EDITION" in phone|desktop|server) ;; *) echo "EDITION muss phone, desktop oder server sein"; exit 2;; esac
TARGET="$ARCH-unknown-linux-gnu"
# C/C++-Querübersetzer für llama.cpp (ultar-llm), wenn für ARM auf einem x86-Rechner gebaut wird
if [ "$ARCH" = aarch64 ] && [ "$(uname -m)" != aarch64 ] && command -v aarch64-linux-gnu-g++ >/dev/null; then
  export CC_aarch64_unknown_linux_gnu=${CC_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-gcc}
  export CXX_aarch64_unknown_linux_gnu=${CXX_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-g++}
  export AR_aarch64_unknown_linux_gnu=${AR_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-ar}
fi
if [ -n "${BIN_DIR:-}" ]; then
  # schon gebaute Programme nehmen (z. B. aus dem CI oder einem Teil-Bau)
  BIN=$BIN_DIR
elif [ "${BUILD_STD:-0}" = 1 ]; then
  # Ohne rustup-Target: Standardbibliothek aus den Quellen mitbauen (braucht rust-src, z. B. Ubuntu-Pakete
  # rustc-1.97 cargo-1.97 rust-1.97-src und PATH=/usr/lib/rust-1.97/bin:$PATH)
  export RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target-$ARCH}
  RUSTFLAGS="-C target-feature=+crt-static ${RUSTFLAGS_EXTRA:-}" cargo build --release -Z build-std=std,panic_abort --target "$TARGET"
  BIN="$CARGO_TARGET_DIR/$TARGET/release"
else
  RUSTFLAGS="-C target-feature=+crt-static ${RUSTFLAGS_EXTRA:-}" cargo build --release --target "$TARGET"
  BIN="${CARGO_TARGET_DIR:-target}/$TARGET/release"
fi
STAGE=$(mktemp -d); trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE"/{bin,sbin,usr/bin,usr/lib,etc,dev,proc,sys,run,tmp,compat,data,media,root}
install -m755 "$BIN/ultar-init" "$STAGE/sbin/init"
install -m755 "$BIN/ultar-ctl" "$BIN/ultar-compat" "$BIN/ultar-net" "$STAGE/usr/bin/"
# Oberfläche (fehlt nur in Teil-Bauten ohne Oberfläche, z. B. Server-Test)
if [ -f "$BIN/ultar-shell" ]; then install -m755 "$BIN/ultar-shell" "$STAGE/usr/bin/"; else echo "Hinweis: ohne ultar-shell"; fi
# Sprachmodell-Dienst (llama.cpp statisch); Modelle selbst liegen auf /data
[ -f "$BIN/ultar-llm" ] && install -m755 "$BIN/ultar-llm" "$STAGE/usr/bin/"
# App-Installer für alle Formate (.uap/.deb/.apk/.exe/.msi/AppImage/Flatpak)
[ -f "$BIN/uapp" ] && install -m755 "$BIN/uapp" "$STAGE/usr/bin/"
# Geräteverbund (Handy, Laptop, Server, Platine): Dienst + Befehlszeile, Standard aus
[ -f "$BIN/ultar-verbund" ] && install -m755 "$BIN/ultar-verbund" "$STAGE/usr/bin/"
[ -f "$BIN/zelle" ] && install -m755 "$BIN/zelle" "$STAGE/usr/bin/"
# Büro-Dateien für Agent und Skripte (PDF, .docx, .xlsx/.csv, Text) – reines Rust
[ -f "$BIN/udoc" ] && install -m755 "$BIN/udoc" "$STAGE/usr/bin/"
# Entwickler-Zugang (SSH, nur Schlüssel; aus, bis eingeschaltet)
[ -f "$BIN/ultar-sshd" ] && install -m755 "$BIN/ultar-sshd" "$STAGE/usr/bin/"
# Gerätedateien des Arbeitsplatzes (Gruppe seat) und Compositor
[ -f "$BIN/ultar-devd-seat" ] && install -m755 "$BIN/ultar-devd-seat" "$STAGE/usr/bin/"
[ -f "$BIN/ultar-compositor" ] && install -m755 "$BIN/ultar-compositor" "$STAGE/usr/bin/"
cp -a rootfs/. "$STAGE/"
# Entwicklungs-Abbild: ENTWICKLER=1 schaltet den SSH-Zugang ein, ENTWICKLER_SCHLUESSEL=<datei.pub …> erlaubt diese
# Schlüssel (z. B. ~/.ssh/ultar_dev_ed25519.pub vom PC, tools/entwickler/ultar-dev.sh einrichten)
if [ "${ENTWICKLER:-0}" = 1 ]; then
  printf '# Entwickler-Zugang an (SSH, Port 2222, nur Schlüssel)\n' > "$STAGE/etc/ultar/entwickler"
  : > "$STAGE/etc/ultar/entwickler.keys"
  for k in ${ENTWICKLER_SCHLUESSEL:-}; do [ -f "$k" ] && cat "$k" >> "$STAGE/etc/ultar/entwickler.keys"; done
  chmod 600 "$STAGE/etc/ultar/entwickler.keys"
fi
# Ausgabe: PC-/Server-Konfiguration darüberlegen und festschreiben (ultar-init liest /etc/ultar/edition)
if [ "$EDITION" != phone ]; then
  cp -a editions/pc/. "$STAGE/"
  cp -a "editions/$EDITION/." "$STAGE/"
  [ -f "$BIN/ultar-admin" ] && install -m755 "$BIN/ultar-admin" "$STAGE/usr/bin/"
fi
echo "$EDITION" > "$STAGE/etc/ultar/edition"
VER=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
# os-release: brand part from the one source (rootfs/etc/os-release = ultar_brand::release::OS_RELEASE,
# compared by `ultar-check brand`), then the fields of this release
OSNAME=$(. rootfs/etc/os-release; echo "$NAME")
{ cat rootfs/etc/os-release; printf 'PRETTY_NAME="%s %s"\nVERSION_ID=%s\nVARIANT_ID=%s\n' "$OSNAME" "$VER" "$VER" "$EDITION"; } > "$STAGE/etc/os-release.new"
mv "$STAGE/etc/os-release.new" "$STAGE/etc/os-release"
ln -sf sbin/init "$STAGE/init"   # Kernel sucht im Start-Ramdisk /init (Handy-Boot-Abbilder)
ln -sf /usr/bin/ultar-compat "$STAGE/sbin/modprobe"   # Kernel-Modul-Nachladen über den Ubuntu-Bereich
if [ "$DEBUG" = 1 ]; then
  BB=${BUSYBOX:-/bin/busybox}
  install -m755 "$BB" "$STAGE/bin/busybox"
  for a in $("$BB" --list); do [ -e "$STAGE/bin/$a" ] || ln -s busybox "$STAGE/bin/$a"; done
else
  rm -f "$STAGE/etc/ultar/services/"{selftest,shell,heartbeat,wifitest,tontest,webtest}.toml "$STAGE/usr/lib/ultar/"{selftest,wifitest}.sh
fi
if [ -n "${MODULES:-}" ]; then
  KVER=$(ls "$MODULES_DIR/lib/modules" | head -1)
  [ -f "$MODULES_DIR/lib/modules/$KVER/modules.dep" ] || depmod -b "$MODULES_DIR" "$KVER"
  mkdir -p "$STAGE/lib/modules"
  for m in $MODULES; do
    # fehlende/fest eingebaute Module nur melden, nicht abbrechen (je nach Kernel-Konfiguration verschieden)
    { modprobe -d "$MODULES_DIR" -S "$KVER" --show-depends "$m" 2>/dev/null || echo "Hinweis: Modul $m nicht gefunden" >&2; } | awk '$1=="insmod"{print $2}' | while read -r f; do
      n=$(basename "$f" .zst); n=${n%.xz}
      if [ ! -e "$STAGE/lib/modules/$n" ]; then
        case "$f" in *.zst) zstd -dqc "$f" > "$STAGE/lib/modules/$n";; *.xz) xz -dc "$f" > "$STAGE/lib/modules/$n";; *) cp "$f" "$STAGE/lib/modules/$n";; esac
        echo "/lib/modules/$n" >> "$STAGE/etc/ultar/modules.list"
      fi
    done
  done
fi
# Benutzer und Gruppen aus rootfs/etc/ultar/ids.toml (feste Nummern der Dienste, Gruppe seat …)
awk -v P="$STAGE/etc/passwd" -v G="$STAGE/etc/group" '
  /^\[/ { sec = $0; next }
  /^[a-z][a-z0-9_-]* *= */ {
    name = $1; v = $0; sub(/^[^=]*= */, "", v); sub(/ *#.*$/, "", v)
    if (sec == "[users]") { uid[name] = v; order[++n] = name; gid[name] = v }
    else if (sec == "[groups]") { gid[name] = v; order[++n] = name }
    else if (sec == "[members]") { gsub(/[][" ]/, "", v); mem[name] = v }
  }
  END {
    printf "" > P; printf "" > G
    for (i = 1; i <= n; i++) {
      k = order[i]
      if (k in uid) print k ":x:" uid[k] ":" uid[k] ":" k ":" (k == "root" ? "/root:/bin/sh" : "/nonexistent:/bin/false") >> P
      print k ":x:" gid[k] ":" mem[k] >> G
    }
  }' rootfs/etc/ultar/ids.toml
mkdir -p "$OUT"
# Startbilder für den Bootloader (Logo + Text je Sprache), nur wenn das Programm hier lauffähig ist
if [ "$ARCH" = "$(uname -m)" ] && [ -x "$BIN/ultar-shell" ]; then
  mkdir -p "$OUT/boot"
  for l in ar en de; do "$BIN/ultar-shell" --splash "$l" "${SPLASH_W:-720}" "${SPLASH_H:-1280}" "$OUT/boot/splash-$l.bmp"; done
fi
# zstd: kleiner als gzip und beim Start deutlich schneller entpackt (Kernel braucht CONFIG_RD_ZSTD)
# COMPRESS=gzip für ältere Kernel ohne zstd (z. B. Samsung 4.4)
# Gerätedateien, die schon VOR dem Einhängen von devtmpfs da sein müssen: Hat der Kernel keine Konsole (Handy),
# startet init ohne Standard-Ein/Ausgabe; die Rust-Laufzeit öffnet dann /dev/null und bricht ab, wenn es fehlt.
mkdir -p "$STAGE/dev"
for n in "null c 1 3" "zero c 1 5" "console c 5 1" "kmsg c 1 11" "tty c 5 0" "urandom c 1 9"; do set -- $n; [ -e "$STAGE/dev/$1" ] || mknod -m 666 "$STAGE/dev/$1" $2 $3 $4 || { echo "FEHLER: mknod $1 (als root bauen)"; exit 1; }; done
chmod 600 "$STAGE/dev/console" "$STAGE/dev/kmsg"
rm -f "$OUT/ultar-initramfs-$ARCH.cpio.gz"
if [ "${COMPRESS:-zstd}" = gzip ]; then
  (cd "$STAGE" && find . | cpio -o -H newc --quiet | gzip -9) > "$OUT/ultar-initramfs-$ARCH.cpio.gz"
  ls -la "$OUT/ultar-initramfs-$ARCH.cpio.gz"
else
  (cd "$STAGE" && find . | cpio -o -H newc --quiet | zstd -19 -T0 -q) > "$OUT/ultar-initramfs-$ARCH.cpio.zst"
  ls -la "$OUT/ultar-initramfs-$ARCH.cpio.zst"
fi
