#!/usr/bin/env bash
# Baut den Ultar-Kern (eigener Rust-Kernel, UEFI) und optional ein startfähiges Testabbild.
#   scripts/build-kern.sh            → kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
#   IMG=1 scripts/build-kern.sh      → zusätzlich out/ultar-kern-esp.img (FAT, \EFI\BOOT\BOOTX64.EFI)
#   RUN=1 scripts/build-kern.sh      → in QEMU starten (Konsole auf dem Terminal)
# Werkzeugkette: Rust mit Ziel x86_64-unknown-uefi (rustup target add x86_64-unknown-uefi) ODER Ubuntu-Pakete
# rustc-1.97 cargo-1.97 rust-1.97-src (dann baut cargo core/alloc aus den Quellen mit).
set -euo pipefail
cd "$(dirname "$0")/../kernel/ultar-kern"
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1
cargo build --release
EFI=target/x86_64-unknown-uefi/release/ultar-kern.efi
ls -la "$EFI"
OUT=../../out; mkdir -p "$OUT"
if [ "${IMG:-0}" = 1 ] || [ "${RUN:-0}" = 1 ]; then
  rm -f "$OUT/ultar-kern-esp.img"; truncate -s 64M "$OUT/ultar-kern-esp.img"
  mkfs.vfat -F 32 -n ULTAR-KERN "$OUT/ultar-kern-esp.img" >/dev/null
  mmd -i "$OUT/ultar-kern-esp.img" ::/EFI ::/EFI/BOOT
  mcopy -i "$OUT/ultar-kern-esp.img" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
  # Testprogramme für Ring 3 (statisch) und – falls vorhanden – Ubuntus busybox-static unverändert
  mmd -i "$OUT/ultar-kern-esp.img" ::/bin
  ( cd user && make -s ) && for p in user/hallo user/hallo-glibc user/kerntest user/ereignis user/fadentest user/blocktest user/lauscher user/zelltest; do mcopy -i "$OUT/ultar-kern-esp.img" "$p" ::/bin/; done
  if [ -x /bin/busybox ] && file -L /bin/busybox | grep -q "statically linked"; then
    mcopy -i "$OUT/ultar-kern-esp.img" /bin/busybox ::/bin/busybox
    /bin/busybox --list > /tmp/ultar-applets && mcopy -i "$OUT/ultar-kern-esp.img" /tmp/ultar-applets ::/bin/.applets
  fi
  printf 'Willkommen auf dem Ultar-Kern.\nDiese Datei liegt auf der FAT-Startpartition.\n' > /tmp/ultar-liesmich.txt
  mcopy -i "$OUT/ultar-kern-esp.img" /tmp/ultar-liesmich.txt ::/liesmich.txt
  ls -la "$OUT/ultar-kern-esp.img"
fi
if [ "${RUN:-0}" = 1 ]; then
  CODE=/usr/share/OVMF/OVMF_CODE_4M.fd; cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/ultar-kern-vars.fd
  exec qemu-system-x86_64 -machine q35 -m 1024 -smp 4 -cpu max -drive if=pflash,format=raw,readonly=on,file=$CODE \
    -drive if=pflash,format=raw,file=/tmp/ultar-kern-vars.fd -drive file="$OUT/ultar-kern-esp.img",if=none,id=b,format=raw \
    -device virtio-blk-pci,drive=b -netdev user,id=n -device virtio-net-pci,netdev=n -serial stdio
fi
