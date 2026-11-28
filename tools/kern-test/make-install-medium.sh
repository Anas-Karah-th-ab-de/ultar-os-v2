#!/usr/bin/env bash
# Installationsmedium: der Ultar-Kern startet die grafische Installation und kopiert die Nutzlast auf die Zielplatte.
# Aufbau (GPT):
#   1  ULTAR-INSTALL  FAT32  Ultar-Kern (\EFI\BOOT\BOOTX64.EFI) + \ultar-installer – startet die Installation
#   2  ULTAR-EFI      FAT32  Startpartition des installierten Systems (Ultar-Kern-Ausgabe: nur der A/B-Lader)
#   3  ultar-kernel   roh    (Ultar-Kern-Ausgabe) Kern für Bereich A
#   4  ultar-compat   ext4   Systembereich (wird ultar-system-a)
#   5  ultar-data     ext4   Einstellungen, Apps, Nutzerdaten
# Die Ultar-Kern-Ausgabe wird im A/B-Aufbau installiert (docs/integration/boot-ab.md): ULTAR-EFI, ULTAR-BOOTCTL,
# ultar-kernel-a/b, ultar-system-a/b, ultar-data.
# Nutzlast:
#   DESKTOP_IMG=out/ultar-desktop-x86_64.img  → Ultar OS Desktop (Linux-Kernel + Ultar-Oberfläche, scripts/build-pc-image.sh)
#   sonst                                    → Ultar-Kern + Ultar-Bereich mit Desktop-Oberfläche (ultar-shell), ohne Linux
#                                              und ohne Ubuntu (unabhängig). MIT_UBUNTU=1 (oder UBUNTU_ROOT=…) legt die
#                                              Ubuntu-Programme dazu (Standard out/compat-root-amd64-u26, Ubuntu 26.04)
# Ist das System installiert, startet das Medium künftig die Platte (Taste I = trotzdem Installation).
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH=$PATH:/usr/sbin:/sbin
OUT=out
IMG=${IMG_OUT:-$OUT/ultar-install-u26.img}
ESP=$OUT/ultar-kern-esp.img
[ -f "$ESP" ] || IMG=1 scripts/build-kern.sh
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
# 1) Installationspartition
cp "$ESP" "$W/install.fat"
printf 'Ultar OS Installationsmedium\n' > "$W/marke"
mcopy -o -i "$W/install.fat" "$W/marke" ::/ultar-installer
PARTS=("$W/install.fat:ef00:ULTAR-INSTALL")
if [ -n "${DESKTOP_IMG:-}" ]; then
  # 2–4) Partitionen der Desktop-Ausgabe unverändert übernehmen
  for n in 1 2 3; do
    S=$(sgdisk -i $n "$DESKTOP_IMG" | awk '/First sector/{print $3}'); E=$(sgdisk -i $n "$DESKTOP_IMG" | awk '/Last sector/{print $3}')
    NAME=$(sgdisk -i $n "$DESKTOP_IMG" | sed -n "s/^Partition name: '\(.*\)'/\1/p")
    TYPE=$(sgdisk -i $n "$DESKTOP_IMG" | awk '/Partition GUID code/{print $4}')
    dd if="$DESKTOP_IMG" of="$W/p$n" bs=512 skip="$S" count=$((E - S + 1)) status=none
    case "$NAME" in ULTAR-EFI) T=ef00;; *) T=8300;; esac
    if [ "$NAME" = ULTAR-EFI ]; then
      # Systemsprache: die Installation überschreibt sie mit der gewählten (ar/de/en); ultar-init übernimmt sie
      printf 'ar\n' > "$W/spr"; mmd -i "$W/p$n" ::/ultar 2>/dev/null || true; mcopy -o -i "$W/p$n" "$W/spr" ::/ultar/sprache
    fi
    PARTS+=("$W/p$n:$T:$NAME")
  done
else
  SRC=${UBUNTU_ROOT:-$OUT/compat-root-amd64-u26}
  # Desktop-Oberfläche: ultar-shell statisch (läuft unverändert auf dem Ultar-Kern)
  SH=target/x86_64-unknown-linux-gnu/release/ultar-shell
  if [ ! -x "$SH" ] || [ -n "${NEU_BAUEN:-}" ]; then
    ( [ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
      RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu -p ultar-shell )
  fi
  if { [ -z "${MIT_UBUNTU:-}" ] && [ -z "${UBUNTU_ROOT:-}" ]; } || [ ! -x "$SRC/usr/bin/dpkg" ]; then
    # nur Ultar: kein Ubuntu – Oberfläche, Busybox für das Terminal, Grundgerüst
    echo "Ultar-Bereich ohne Ubuntu"
    SRC=$W/eigen; mkdir -p "$SRC"/{bin,usr/bin,etc/ultar,data/ultar,root,tmp,home}
    # brand part: one source (docs/BRAND.md)
    { cat rootfs/etc/os-release; printf 'PRETTY_NAME="%s (Ultar-Kern)"\n' "$(. rootfs/etc/os-release; echo "$NAME")"; } > "$SRC/etc/os-release"
    BB=$(bash scripts/static-busybox.sh)
    if [ -n "$BB" ]; then
      cp "$BB" "$SRC/bin/busybox"; "$BB" --list > "$SRC/bin/.applets"
    fi
  fi
  # 2) Startpartition des installierten Systems: A/B-Lader + Marke \ultar\installiert; der Kern liegt roh daneben
  bash scripts/build-loader.sh >/dev/null
  cp "$ESP" "$W/efi.fat"
  mcopy -o -i "$W/efi.fat" kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi ::/EFI/BOOT/BOOTX64.EFI
  mmd -i "$W/efi.fat" ::/ultar 2>/dev/null || true
  printf 'Ultar OS\n' > "$W/inst"; mcopy -o -i "$W/efi.fat" "$W/inst" ::/ultar/installiert
  # Systemsprache (wird bei der Installation mit der gewählten überschrieben) – ruhiger Start + Startseite
  printf 'ar\n' > "$W/spr"; mcopy -o -i "$W/efi.fat" "$W/spr" ::/ultar/sprache
  PARTS+=("$W/efi.fat:ef00:ULTAR-EFI")
  cp kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi "$W/kernel.raw"
  PARTS+=("$W/kernel.raw:8300:ultar-kernel")
  # 3) Ubuntu-Bereich
  R=$W/root; mkdir -p "$R"; cp -a "$SRC/." "$R/"
  mkdir -p "$R/etc/ultar"; printf 'desktop\n' > "$R/etc/ultar/edition"; printf 'ultar\n' > "$R/etc/hostname"
  printf 'Ultar OS (Ultar-Kern) – %s\n' "$(. "$SRC/etc/os-release"; echo "$PRETTY_NAME")" > "$R/etc/issue"
  rm -f "$R/etc/ultar/sprache"; rm -rf "$R/var/cache/apt/archives/"*.deb "$R/var/lib/apt/lists/"* 2>/dev/null || true
  UI=target/x86_64-unknown-linux-gnu/release/ultar-init; [ -x "$UI" ] && cp "$UI" "$R/usr/bin/ultar-init"
  mkdir -p "$R/usr/bin" "$R/data/ultar" && strip -o "$R/usr/bin/ultar-shell" "$SH"
  MB=$(( $(du -sm "$R" | cut -f1) * 13 / 10 + 64 ))
  mkfs.ext4 -q -L ultar-compat -d "$R" "$W/root.ext4" "${MB}M"
  rm -rf "$R"
  PARTS+=("$W/root.ext4:8300:ultar-compat")
  mkdir -p "$W/data/ultar"; mkfs.ext4 -q -L ultar-data -d "$W/data" "$W/data.ext4" 256M
  # owned by root whoever builds the medium (the updater trusts only a state directory of its own user)
  printf 'sif / uid 0\nsif / gid 0\nsif /ultar uid 0\nsif /ultar gid 0\n' > "$W/owners"
  debugfs -w -f "$W/owners" "$W/data.ext4" >/dev/null 2>&1
  PARTS+=("$W/data.ext4:8300:ultar-data")
fi
# Größe und Partitionstabelle
TOTAL=2048
for p in "${PARTS[@]}"; do f=${p%%:*}; TOTAL=$(( TOTAL + ( $(stat -c %s "$f") / 512 + 2047 ) / 2048 * 2048 )); done
rm -f "$IMG"; truncate -s $(( (TOTAL + 2048) * 512 )) "$IMG"
ARGS=(); i=1
for p in "${PARTS[@]}"; do
  f=${p%%:*}; rest=${p#*:}; t=${rest%%:*}; name=${rest#*:}
  SECT=$(( ( $(stat -c %s "$f") / 512 + 2047 ) / 2048 * 2048 ))
  ARGS+=(-n "$i:0:+$SECT" -t "$i:$t" -c "$i:$name"); i=$((i + 1))
done
sgdisk "${ARGS[@]}" "$IMG" >/dev/null
i=1
for p in "${PARTS[@]}"; do
  f=${p%%:*}
  S=$(sgdisk -i $i "$IMG" | awk '/First sector/{print $3}')
  dd if="$f" of="$IMG" bs=512 seek="$S" conv=notrunc status=none
  i=$((i + 1))
done
ls -la "$IMG"
sgdisk -p "$IMG" | tail -$(( ${#PARTS[@]} + 1 ))
