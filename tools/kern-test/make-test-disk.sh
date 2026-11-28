#!/usr/bin/env bash
# Testplatte für den Ultar-Kern: GPT mit ULTAR-EFI (FAT: Kern + /bin) und ultar-compat (ext4: kleine Ubuntu-Wurzel
# mit dynamisch gebundenen Programmen aus diesem Ubuntu, ld.so und Bibliotheken – merged-usr wie Ubuntu 24.04).
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=out; mkdir -p $OUT
ESP=$OUT/ultar-kern-esp.img
[ -f $ESP ] || IMG=1 scripts/build-kern.sh
R=$(mktemp -d); trap 'rm -rf $R' EXIT
mkdir -p $R/usr/bin $R/usr/lib/x86_64-linux-gnu $R/usr/lib64 $R/etc $R/tmp $R/root $R/home
ln -s usr/bin $R/bin; ln -s usr/lib $R/lib; ln -s usr/lib64 $R/lib64; ln -s usr/bin $R/sbin
PROGS="ls cat echo uname date head wc sort tr grep sed seq env id basename dirname true false sleep mkdir rm cp mv touch du df bash"
# a multi-call binary (e.g. the Rust coreutils of newer Ubuntu: one 11 MB file for every tool) is copied
# once; the other names link to it (it picks the tool from argv[0]); a hard-linked one counts as the same file
declare -A SEEN=()
for p in $PROGS; do
  f=$(PATH=/usr/bin:/bin:/usr/sbin:/sbin type -P $p) || continue
  real=$(stat -L -c %d:%i "$f")   # same file, also when the names are hard links
  if [ -n "${SEEN[$real]:-}" ]; then ln -sf "${SEEN[$real]}" $R/usr/bin/$p; continue; fi
  SEEN[$real]=$p
  cp -L "$f" $R/usr/bin/$p
  for l in $(ldd "$f" | grep -o '/[^ ]*'); do
    d=$R$(dirname "$l"); mkdir -p "$d"; cp -L "$l" "$d/" 2>/dev/null || true
    b=$(basename "$l"); [ -e $R/usr/lib/x86_64-linux-gnu/$b ] || cp -L "$l" $R/usr/lib/x86_64-linux-gnu/
  done
done
[ -x /bin/busybox ] && cp /bin/busybox $R/usr/bin/busybox
for t in /usr/share/terminfo/l/linux /lib/terminfo/l/linux; do [ -f $t ] && { mkdir -p $R/usr/share/terminfo/l; cp -L $t $R/usr/share/terminfo/l/; break; }; done
printf 'root:x:0:0:root:/root:/bin/bash\n' > $R/etc/passwd
printf 'root:x:0:\n' > $R/etc/group
printf 'Ultar OS (Ultar-Kern, Testwurzel)\n' > $R/etc/issue
{ cat rootfs/etc/os-release; printf 'ID_LIKE=ubuntu\nVERSION_ID="0.2"\n'; } > $R/etc/os-release   # brand part: one source (docs/BRAND.md)
# ultar-init (statisch) und ein Beispieldienst, falls gebaut
UI=target/x86_64-unknown-linux-gnu/release/ultar-init
if [ -x $UI ]; then
  cp $UI $R/usr/bin/ultar-init
  [ -x target/x86_64-unknown-linux-gnu/release/ultar-ctl ] && cp target/x86_64-unknown-linux-gnu/release/ultar-ctl $R/usr/bin/
  mkdir -p $R/etc/ultar/services
  printf 'exec = "/bin/bash"\nargs = ["-c", "sleep 3; echo --- ultar-ctl status ---; ultar-ctl status > /run/ctl.txt 2>&1; echo rc=$?; cat /run/ctl.txt"]\nconsole = true\noneshot = true\nrestart = "never"\n' > $R/etc/ultar/services/ctltest.toml
  printf 'exec = "/bin/bash"\nargs = ["-c", "while true; do date > /run/herzschlag; sleep 1; done"]\nrestart = "always"\n' > $R/etc/ultar/services/herzschlag.toml
  printf 'exec = "/bin/bash"\nargs = ["-c", "echo Dienst hallo laeuft unter ultar-init auf dem Ultar-Kern; sleep 2; echo Dienst hallo fertig"]\noneshot = true\nrestart = "never"\n' > $R/etc/ultar/services/hallo.toml
fi
# Ultar-Verbund (falls gebaut) samt Bibliotheken
UV=target/release/ultar-verbund
if [ -x $UV ]; then
  cp $UV $R/usr/bin/ultar-verbund
  for l in $(ldd "$UV" | grep -o '/[^ ]*'); do
    d=$R$(dirname "$l"); mkdir -p "$d"; cp -L "$l" "$d/" 2>/dev/null || true
    b=$(basename "$l"); [ -e $R/usr/lib/x86_64-linux-gnu/$b ] || cp -L "$l" $R/usr/lib/x86_64-linux-gnu/
  done
  printf 'ultar-kern\n' > $R/etc/hostname
  mkdir -p $R/etc/ultar/services; { echo 'manual = true'; sed '/^after/d;/^\[cgroup\]/,$d' rootfs/etc/ultar/services/verbund.toml; } > $R/etc/ultar/services/verbund.toml
fi
# Büro-Apps als Befehlszeile (udoc, statisch – reines Rust) mit Beispieldateien, falls gebaut
UD=target/x86_64-unknown-linux-gnu/release/udoc
if [ -x $UD ]; then
  cp $UD $R/usr/bin/udoc
  mkdir -p $R/root/buero
  for f in ${BUERO_DATEIEN:-}; do [ -f "$f" ] && cp "$f" $R/root/buero/; done
fi
# Sprachmodell-Dienst (statisch, llama.cpp + Bild-Projektor), falls gebaut – Test mit Test-Engine "mock:"
UL=target/x86_64-unknown-linux-gnu/release/ultar-llm
if [ -x $UL ] && [ "${MIT_LLM:-0}" = 1 ]; then
  cp $UL $R/usr/bin/ultar-llm
  [ -n "${TESTBILD:-}" ] && [ -f "$TESTBILD" ] && cp "$TESTBILD" $R/root/testbild.png
fi
# Oberfläche (statisch), falls gebaut – läuft auf dem Ultar-Kern über /dev/fb0 (tools/kern-test/qui.py)
US=target/x86_64-unknown-linux-gnu/release/ultar-shell
if [ -x $US ] && [ "${MIT_SHELL:-0}" = 1 ]; then
  cp $US $R/usr/bin/ultar-shell
  mkdir -p $R/data/ultar $R/run/ultar
  printf 'setup_done = true\nlang = "%s"\n' "${SPRACHE:-de}" > $R/data/ultar/settings.toml
  printf '1 -1 -1 1\n' > $R/data/ultar/buddy   # Helfer: vorgestellt, weggeschickt (Testbilder ohne Überlagerung)
  # wie ein installiertes System: Oberfläche startet von selbst (sonst Kern-Konsole)
  [ "${DESKTOP_AUTOSTART:-0}" = 1 ] && mkdir -p $R/etc/ultar && printf '%s\n' "${SPRACHE:-de}" > $R/etc/ultar/sprache
fi
# Entwickler-Zugang (SSH, statisch) – ENTWICKLER=1, Schlüssel aus ENTWICKLER_SCHLUESSEL (Standard ~/.ssh/*.pub);
# Wirtsschlüssel bleibt in out/ erhalten, damit der PC das Gerät wiedererkennt
USSH=target/x86_64-unknown-linux-gnu/release/ultar-sshd
if [ -x $USSH ] && [ "${ENTWICKLER:-0}" = 1 ]; then
  cp $USSH $R/usr/bin/ultar-sshd
  mkdir -p $R/etc/ultar $R/data/ultar/entwickler
  printf '# Entwickler-Zugang an (SSH, Port 2222, nur Schlüssel)\n' > $R/etc/ultar/entwickler
  : > $R/etc/ultar/entwickler.keys
  for k in ${ENTWICKLER_SCHLUESSEL:-$HOME/.ssh/*.pub}; do [ -f "$k" ] && cat "$k" >> $R/etc/ultar/entwickler.keys; done
  [ -f $OUT/entwickler-host_ed25519 ] || ssh-keygen -q -t ed25519 -N "" -C ultar-kern -f $OUT/entwickler-host_ed25519
  cp $OUT/entwickler-host_ed25519 $R/etc/ultar/entwickler.host
  chmod 600 $R/etc/ultar/entwickler.host
fi
ROOTIMG=$OUT/test-root.ext4; rm -f $ROOTIMG
mkfs.ext4 -q -L ultar-compat -d $R $ROOTIMG ${ROOT_MB:-128}M
DISK=${DISK:-$OUT/ultar-kern-disk.img}; rm -f $DISK; truncate -s $(( ${ROOT_MB:-128} + 72 ))M $DISK >/dev/null
ESPNAME=ULTAR-EFI; [ "${INSTALLER:-0}" = 1 ] && ESPNAME=ULTAR-INSTALL
sgdisk -n 1:2048:+64M -t 1:ef00 -c 1:$ESPNAME -n 2:0:+${ROOT_MB:-128}M -t 2:8300 -c 2:ultar-compat $DISK >/dev/null
S1=$(sgdisk -i 1 $DISK | awk '/First sector/{print $3}'); S2=$(sgdisk -i 2 $DISK | awk '/First sector/{print $3}')
dd if=$ESP of=$DISK bs=512 seek=$S1 conv=notrunc status=none
dd if=$ROOTIMG of=$DISK bs=512 seek=$S2 conv=notrunc status=none
ls -la $DISK >/dev/null
