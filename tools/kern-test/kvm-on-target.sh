#!/usr/bin/env bash
# Step before a bare-metal start: boot the Ultar kernel on the target computer itself under
# KVM, with the real processor (-cpu host), all cores and the real SMBIOS tables of the machine.
# Changes nothing on the machine. Usage (on the target, as root):
#   kvm-on-target.sh <kernel.efi> <bin-dir>        prints the self-test log
set -euo pipefail
EFI="${1:?kernel.efi}"; BINS="${2:?bin-dir}"; W="$(mktemp -d)"
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86 missing"; exit 1; }
command -v mcopy >/dev/null || { echo "mtools missing"; exit 1; }
truncate -s 64M "$W/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$W/esp.img" >/dev/null
mmd -i "$W/esp.img" ::/EFI ::/EFI/BOOT ::/bin ::/ultar-kern
mcopy -i "$W/esp.img" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
for f in "$BINS"/*; do mcopy -i "$W/esp.img" "$f" ::/bin/; done
head -c 262144 /dev/zero | tr '\0' ' ' > "$W/log.txt"; mcopy -i "$W/esp.img" "$W/log.txt" ::/ultar-kern/selbsttest.txt
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd"
SMB=(); for t in /sys/firmware/dmi/tables/smbios_entry_point; do [ -r "$t" ] && cat /sys/firmware/dmi/tables/DMI > "$W/dmi.bin" && SMB=(-smbios "file=$W/dmi.bin"); done
timeout 240 qemu-system-x86_64 -enable-kvm -machine q35 -cpu host -smp "$(nproc)" -m 4096 -display none -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive "if=pflash,format=raw,file=$W/vars.fd" \
  -drive "file=$W/esp.img,if=none,id=b,format=raw" -device nvme,drive=b,serial=ultartest \
  -netdev user,id=n -device e1000e,netdev=n "${SMB[@]}" -serial "file:$W/serial.log" -monitor none || true
mcopy -o -i "$W/esp.img" ::/ultar-kern/selbsttest.txt "$W/out.txt"
if grep -aq "Selbsttest" "$W/out.txt"; then tr -d '\0' < "$W/out.txt" | sed 's/ *$//' | grep -av '^$'; else echo "NO LOG – serial output:"; tail -40 "$W/serial.log"; exit 1; fi
rm -rf "$W"
