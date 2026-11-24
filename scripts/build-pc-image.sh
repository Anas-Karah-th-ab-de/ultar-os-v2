#!/usr/bin/env bash
# Baut ein startfähiges Ultar-OS-Abbild für PCs, Laptops und Server (UEFI, x86_64).
#   EDITION=desktop|server   OUT=out/   DATA_SIZE=512M   ESP_SIZE=256M   DEBUG=1
# Ergebnis: out/ultar-<edition>-x86_64.img – mit dd/Rufus/balenaEtcher auf einen USB-Stick schreiben und davon
# starten (UEFI). Beim ersten Start wächst die Datenpartition auf den freien Platz. Auf die interne Platte
# installiert scripts/…/install-to-disk.sh (siehe docs/AUSGABEN.md).
#
# Aufbau (GPT):
#   1  ULTAR-EFI     FAT32  systemd-boot, Kernel (aus dem Ubuntu-Bereich), Ultar-Startabbild, Bootmenü
#   2  ultar-compat  ext4   Ubuntu 24.04: Kernel-Module, Firmware, Wine, Waydroid, Flatpak, SSH … (PROFILE=EDITION)
#   3  ultar-data    ext4   Einstellungen, Apps, Nutzerdaten
set -euo pipefail
cd "$(dirname "$0")/.."
EDITION=${EDITION:-desktop}; OUT=${OUT:-out}; ARCH=x86_64; DARCH=amd64
DATA_SIZE=${DATA_SIZE:-512M}; ESP_SIZE=${ESP_SIZE:-256M}
case "$EDITION" in desktop|server) ;; *) echo "EDITION muss desktop oder server sein (Handy: Ultar Flash)"; exit 2;; esac
for t in sgdisk mkfs.vfat mcopy mmd mkfs.ext4 zstd cpio; do command -v $t >/dev/null || { echo "Werkzeug fehlt: $t (apt install gdisk dosfstools mtools e2fsprogs zstd cpio)"; exit 2; }; done
SDBOOT=${SDBOOT:-/usr/lib/systemd/boot/efi/systemd-bootx64.efi}
[ -f "$SDBOOT" ] || { echo "systemd-boot fehlt: apt install systemd-boot-efi"; exit 2; }

# 1) Ubuntu-Bereich mit Kernel (PROFILE = Ausgabe)
CROOT=${COMPAT_ROOT:-$OUT/compat-root-$DARCH-$EDITION}
CIMG=${COMPAT_IMG:-$OUT/compat-$DARCH-$EDITION.ext4}
if [ ! -f "$CIMG" ]; then PROFILE=$EDITION COMPAT_ROOT=$CROOT OUT=$OUT ARCH=$DARCH scripts/make-compat.sh; fi
KVER=${KVER:-$(ls "$CROOT/lib/modules" | sort -V | tail -1)}
VMLINUZ=${VMLINUZ:-$CROOT/boot/vmlinuz-$KVER}
[ -f "$VMLINUZ" ] || { echo "Kernel fehlt: $VMLINUZ (Ubuntu-Bereich mit PROFILE=$EDITION bauen)"; exit 2; }
echo "Kernel $KVER"

# 2) Ultar-Kernsystem (initramfs) mit den Treibern, die vor dem Einhängen der Partitionen nötig sind
BOOTMODS=${BOOTMODS:-"nvme ahci sd_mod usb_storage uas xhci_pci ehci_pci ohci_pci uhci_hcd sdhci_pci sdhci_acpi mmc_block vmd virtio_blk virtio_scsi hid_generic usbhid i2c_hid_acpi cfg80211 mac80211 iwlwifi iwlmvm intel_vpu nouveau nvidia nvidia_modeset nvidia_drm nvidia_uvm nls_cp437 nls_iso8859_1"}
# Prozessor-Grundlinie x86-64 (läuft überall, auch in VMs mit „qemu64“/„kvm64“-CPU). Für reine Hardware ab ~2009
# schneller: RUSTFLAGS_EXTRA="-C target-cpu=x86-64-v2" – aber PID 1 mit unbekanntem Befehl = Startabbruch (SIGILL).
# Eigenes Zielverzeichnis, damit der Handy-Bau unberührt bleibt.
export RUSTFLAGS_EXTRA="${RUSTFLAGS_EXTRA:-}" CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target-pc}
EDITION=$EDITION ARCH=$ARCH OUT=$OUT/initramfs-$EDITION MODULES="$BOOTMODS" MODULES_DIR="$CROOT" DEBUG=${DEBUG:-1} scripts/build-initramfs.sh
INITRD=$OUT/initramfs-$EDITION/ultar-initramfs-$ARCH.cpio.zst
if [ -d "$CROOT/lib/firmware/intel/iwlwifi" ]; then
  AX201=$(find "$CROOT/lib/firmware/intel/iwlwifi" -type f -name 'iwlwifi-QuZ-*.ucode.zst' | head -1)
  BE200_PNVM=$(find "$CROOT/lib/firmware/intel/iwlwifi" -type f -name 'iwlwifi-bz-*.pnvm.zst' | head -1)
  [ -n "$AX201" ] || echo "WARNUNG: AX201/QuZ-iwlwifi-Firmware fehlt"
  [ -n "$BE200_PNVM" ] || echo "WARNUNG: BE200/Bz-PNVM-Firmware fehlt"
fi

# 3) EFI-Systempartition
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
ESP=$W/esp.img
truncate -s "$ESP_SIZE" "$ESP"; mkfs.vfat -F 32 -n ULTAR-EFI "$ESP" >/dev/null
mmd -i "$ESP" ::/EFI ::/EFI/BOOT ::/loader ::/loader/entries ::/ultar
mcopy -i "$ESP" "$SDBOOT" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$ESP" "$VMLINUZ" ::/ultar/vmlinuz
mcopy -i "$ESP" "$INITRD" ::/ultar/initramfs.cpio.zst
TITLE=$([ "$EDITION" = server ] && echo "Ultar OS Server" || echo "Ultar OS")
printf 'timeout 2\ndefault ultar.conf\nconsole-mode max\neditor no\n' > "$W/loader.conf"
COMMON="ultar.edition=$EDITION rw quiet loglevel=3"
printf 'title   %s\nsort-key 1\nlinux   /ultar/vmlinuz\ninitrd  /ultar/initramfs.cpio.zst\noptions %s\n' "$TITLE" "$COMMON" > "$W/ultar.conf"
printf 'title   %s (sichere Grafik / safe graphics)\nsort-key 2\nlinux   /ultar/vmlinuz\ninitrd  /ultar/initramfs.cpio.zst\noptions %s nomodeset\n' "$TITLE" "$COMMON" > "$W/ultar-safe.conf"
printf 'title   %s (Rettung / rescue)\nsort-key 3\nlinux   /ultar/vmlinuz\ninitrd  /ultar/initramfs.cpio.zst\noptions ultar.edition=%s rw console=tty0 ultar.start=shell\n' "$TITLE" "$EDITION" > "$W/ultar-rescue.conf"
# Ultar-Kern (eigener Rust-Kernel, ohne Linux) als zusätzlicher Eintrag, wenn gebaut (scripts/build-kern.sh)
KERN_EFI=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
if [ -f "$KERN_EFI" ]; then
  mcopy -i "$ESP" "$KERN_EFI" ::/ultar/ultar-kern.efi
  printf 'title   Ultar-Kern (eigener Kernel, Vorschau)\nsort-key 4\nefi     /ultar/ultar-kern.efi\n' > "$W/ultar-kern.conf"
  mcopy -i "$ESP" "$W/ultar-kern.conf" ::/loader/entries/
fi
mcopy -i "$ESP" "$W/loader.conf" ::/loader/loader.conf
mcopy -i "$ESP" "$W/ultar.conf" "$W/ultar-safe.conf" "$W/ultar-rescue.conf" ::/loader/entries/
# Startbild mit Logo und Arabisch (systemd-stub zeigt .splash): ein UKI je Ausgabe, wenn ukify da ist
if command -v ukify >/dev/null && [ -f "$OUT/initramfs-$EDITION/boot/splash-ar.bmp" ]; then
  ukify build --linux "$VMLINUZ" --initrd "$INITRD" --cmdline "$COMMON" --splash "$OUT/initramfs-$EDITION/boot/splash-ar.bmp" \
    --os-release @rootfs/etc/os-release --output "$W/ultar.efi" >/dev/null 2>&1 && {
      mmd -i "$ESP" ::/EFI/Linux; mcopy -i "$ESP" "$W/ultar.efi" ::/EFI/Linux/ultar-$EDITION.efi
      printf 'timeout 2\ndefault ultar-%s.efi\nconsole-mode max\neditor no\n' "$EDITION" > "$W/loader.conf"; mcopy -o -i "$ESP" "$W/loader.conf" ::/loader/loader.conf
      echo "UKI mit Startbild (Arabisch) angelegt"; } || echo "Hinweis: ukify fehlgeschlagen – Bootmenü ohne Startbild"
fi

# 4) Datenpartition
DATA=$W/data.img
truncate -s "$DATA_SIZE" "$DATA"
mkdir -p "$W/data/ultar"
mkfs.ext4 -q -L ultar-data -d "$W/data" "$DATA"

# 5) Datenträger zusammensetzen
mib() { echo $(( ($(stat -c %s "$1") + 1048575) / 1048576 )); }
E=$(mib "$ESP"); C=$(mib "$CIMG"); D=$(mib "$DATA")
IMG=$OUT/ultar-$EDITION-$ARCH.img
rm -f "$IMG"; truncate -s $(( (1 + E + C + D + 1) * 1048576 )) "$IMG"
sgdisk -o \
  -n 1:2048:+${E}M -t 1:EF00 -c 1:ULTAR-EFI \
  -n 2:0:+${C}M -t 2:8300 -c 2:ultar-compat \
  -n 3:0:+${D}M -t 3:8300 -c 3:ultar-data "$IMG" >/dev/null
[ "$(sgdisk -p "$IMG" | grep -c '^ *[123] ')" = 3 ] || { echo "Partitionierung fehlgeschlagen"; sgdisk -p "$IMG"; exit 1; }
# Partitionen müssen genau dort beginnen, wohin die Dateisysteme geschrieben werden
for i in 1 2 3; do sgdisk -i $i "$IMG" | grep 'First sector'; done
put() { dd if="$2" of="$IMG" bs=1M seek="$1" conv=notrunc,sparse status=none; }
put 1 "$ESP"; put $((1 + E)) "$CIMG"; put $((1 + E + C)) "$DATA"
sgdisk -v "$IMG" | tail -1
ls -la "$IMG"
[ "${COMPRESS:-0}" = 1 ] && { zstd -T0 -19 -q -f "$IMG" -o "$IMG.zst"; ls -la "$IMG.zst"; }
echo "Fertig: $IMG  (auf USB-Stick schreiben: dd if=$IMG of=/dev/sdX bs=4M conv=fsync)"
