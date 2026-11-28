#!/usr/bin/env bash
# USB-Paket für den Selbsttest des Ultar-Kerns auf echter Hardware.
#   scripts/make-kern-usb.sh [ZIEL]   → Ordner (Standard out/ultar-kern-usb), Inhalt einfach auf einen FAT32-Stick kopieren
#   IMG=1 scripts/make-kern-usb.sh    → zusätzlich out/ultar-kern-usb.img (128 MB, für Rufus/dd)
# Der Stick startet den Ultar-Kern (UEFI), der Selbsttest läuft von selbst und schreibt sein Protokoll nach
# ultar-kern/selbsttest.txt auf dem Stick. Auf die Datenträger des Rechners wird nichts geschrieben.
set -euo pipefail
cd "$(dirname "$0")/.."
Z=${1:-out/ultar-kern-usb}
bash scripts/build-kern.sh >/dev/null
EFI=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
( cd kernel/ultar-kern/user && make -s )
rm -rf "$Z"; mkdir -p "$Z/EFI/BOOT" "$Z/bin" "$Z/ultar-kern"
cp "$EFI" "$Z/EFI/BOOT/BOOTX64.EFI"
for p in hallo hallo-glibc kerntest ereignis; do cp kernel/ultar-kern/user/$p "$Z/bin/"; done
BB=$(bash scripts/static-busybox.sh)
if [ -n "$BB" ]; then
  cp "$BB" "$Z/bin/busybox"; "$BB" --list > "$Z/bin/.applets"
fi
# Protokolldatei vorab anlegen (256 KB): der Kern überschreibt sie an Ort und Stelle
head -c 262144 /dev/zero | tr '\0' ' ' > "$Z/ultar-kern/selbsttest.txt"
cat > "$Z/LIESMICH.txt" <<'TXT'
Ultar-Kern – Selbsttest auf echter Hardware (KM Code Craft)

1. USB-Stick mit FAT32 formatieren (Windows: Rechtsklick > Formatieren > FAT32) und den ganzen
   Inhalt dieses Ordners darauf kopieren (EFI, bin, ultar-kern, LIESMICH.txt).
2. Secure Boot im BIOS/UEFI ausschalten (der Ultar-Kern ist noch nicht signiert).
   Lenovo ThinkPad: beim Start F1 > Security > Secure Boot > Disabled.
3. Neu starten und vom Stick starten. ThinkPad: beim Start F12 > USB-Stick wählen.
4. Der Selbsttest läuft von selbst (ca. 10–30 s). Nichts wird auf der Festplatte geändert.
   Am Ende steht "Protokoll geschrieben". Mit dem Befehl  aus  ausschalten (oder Netzschalter).
5. Windows starten, den Stick einstecken und die Datei  ultar-kern\selbsttest.txt  schicken.

Optional: eine leere Datei  ultar-kern\auto-aus  anlegen – dann schaltet der Rechner nach dem Test
von selbst aus (für Server ohne Bildschirm).
Secure Boot danach wieder einschalten.
TXT
echo "USB-Paket: $Z"
if [ "${IMG:-0}" = 1 ]; then
  I=out/ultar-kern-usb.img; rm -f $I; truncate -s 128M $I
  mkfs.vfat -F 32 -n ULTARKERN $I >/dev/null
  mcopy -s -i $I "$Z"/* ::/
  mcopy -i $I "$Z/bin/.applets" ::/bin/.applets 2>/dev/null || true
  echo "Abbild: $I"
fi
