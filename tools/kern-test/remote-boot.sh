#!/usr/bin/env bash
# Starts the Ultar kernel ONCE on a remote computer (bare metal) and gives the machine back.
# Runs on the target (Ubuntu, UEFI) as root. Nothing on the data partitions is changed; only
# files below EFI/ultar-test and ultar-kern/ on the EFI system partition(s) and one boot entry.
#
#   remote-boot.sh check                 read-only: is this machine suitable?
#   remote-boot.sh prepare <kernel.efi> [bin-dir]   copy files, create the one-time boot entry
#   remote-boot.sh go                    restart into the Ultar kernel (BootNext, one time only)
#   remote-boot.sh collect               after the machine is back: print the log, remove everything
#   remote-boot.sh cleanup               remove boot entry and files without a test
#
# Safety net: the kernel must be built with `--features remote-test` (self test, then restart;
# restart deadline 180 s if it hangs). BootNext is used, so every later start – also after a
# power cycle – goes to the normal system again. A hang with interrupts off before the timer
# runs is NOT covered: then someone has to press the power button once.
set -euo pipefail
LABEL="Ultar remote test"; DIR="EFI/ultar-test"; LOGDIR="ultar-kern"
die() { echo "ERROR: $*" >&2; exit 1; }
esp_mounts() { findmnt -rn -t vfat -o TARGET | while read -r m; do [ -d "$m/EFI" ] && echo "$m"; done; }
main_esp() { findmnt -rn -o TARGET /boot/efi 2>/dev/null || esp_mounts | head -1; }
entry() { efibootmgr | sed -n "s/^Boot\([0-9A-F]\{4\}\)\*\{0,1\} $LABEL.*/\1/p" | head -1; }

check() {
  [ "$(id -u)" = 0 ] || die "run as root"
  [ -d /sys/firmware/efi ] || die "not started with UEFI"
  command -v efibootmgr >/dev/null || die "efibootmgr missing (apt install efibootmgr)"
  local sb; sb=$(od -An -t u1 -j4 -N1 /sys/firmware/efi/efivars/SecureBoot-* 2>/dev/null | tr -d ' ' || echo "?")
  [ "$sb" = 0 ] || die "Secure Boot is on (state $sb) – the kernel is not signed"
  local esp; esp=$(main_esp); [ -n "$esp" ] || die "no EFI system partition mounted"
  local free; free=$(df -Pk "$esp" | awk 'NR==2{print $4}'); [ "$free" -ge 16384 ] || die "less than 16 MB free on $esp"
  echo "machine : $(cat /sys/class/dmi/id/product_name 2>/dev/null)"
  echo "esp     : $esp (free $((free/1024)) MB) on $(findmnt -rn -o SOURCE "$esp")"
  echo "current : Boot$(efibootmgr | sed -n 's/^BootCurrent: //p'), order $(efibootmgr | sed -n 's/^BootOrder: //p')"
  echo "sessions: $(who | wc -l) logged in; load $(cut -d' ' -f1-3 /proc/loadavg)"
  echo "OK – suitable"
}

prepare() {
  local efi="${1:?kernel.efi}" bins="${2:-}"; check >/dev/null
  [ -f "$efi" ] || die "$efi not found"
  local esp; esp=$(main_esp)
  mkdir -p "$esp/$DIR"; cp "$efi" "$esp/$DIR/ultar-kern.efi"
  if [ -n "$bins" ]; then mkdir -p "$esp/bin"; cp "$bins"/* "$esp/bin/" && echo "$bins" > "$esp/$DIR/bins-copied"; ls "$bins" > "$esp/$DIR/bins-list"; fi
  if [ ! -e "$esp/liesmich.txt" ]; then printf 'Willkommen auf dem Ultar-Kern.\n' > "$esp/liesmich.txt"; : > "$esp/$DIR/liesmich-created"; fi
  # preallocated log file on every EFI partition: the kernel overwrites it in place
  for m in $(esp_mounts); do mkdir -p "$m/$LOGDIR"; head -c 262144 /dev/zero | tr '\0' ' ' > "$m/$LOGDIR/selbsttest.txt"; done
  sync
  local src disk part order
  src=$(findmnt -rn -o SOURCE "$esp"); disk="/dev/$(lsblk -no PKNAME "$src")"; part=$(cat "/sys/class/block/$(basename "$src")/partition")
  order=$(efibootmgr | sed -n 's/^BootOrder: //p')
  [ -n "$(entry)" ] && efibootmgr -q -b "$(entry)" -B
  efibootmgr -q -c -d "$disk" -p "$part" -L "$LABEL" -l "\\EFI\\ultar-test\\ultar-kern.efi"
  efibootmgr -q -o "$order"                      # creating an entry puts it first: restore the old order
  [ "$(efibootmgr | sed -n 's/^BootOrder: //p')" = "$order" ] || die "boot order could not be restored"
  echo "prepared: entry Boot$(entry), boot order unchanged ($order). Next: remote-boot.sh go"
}

go() {
  local e; e=$(entry); [ -n "$e" ] || die "not prepared"
  efibootmgr -q -n "$e"
  efibootmgr | grep -q "^BootNext: $e" || die "BootNext not set"
  echo "restarting into the Ultar kernel (one time). Back in about 1–4 minutes."
  sync; systemctl reboot
}

cleanup() {
  local e; e=$(entry); [ -n "$e" ] && efibootmgr -q -b "$e" -B
  efibootmgr -q -N 2>/dev/null || true
  for m in $(esp_mounts); do
    if [ -f "$m/$DIR/bins-list" ]; then while read -r f; do rm -f "$m/bin/$f"; done < "$m/$DIR/bins-list"; rmdir "$m/bin" 2>/dev/null || true; fi
    if [ -f "$m/$DIR/liesmich-created" ]; then rm -f "$m/liesmich.txt"; fi
    rm -rf "${m:?}/$DIR" "${m:?}/$LOGDIR"
  done
  echo "cleaned: boot entry and files removed"
}

collect() {
  local found=0
  for m in $(esp_mounts); do
    local f="$m/$LOGDIR/selbsttest.txt"
    if [ -f "$f" ] && grep -aq "Selbsttest" "$f"; then found=1; echo "=== log from $m ==="; tr -d '\0' < "$f" | sed 's/ *$//' | grep -av '^$'; fi
  done
  [ "$found" = 1 ] || echo "NO LOG: the kernel did not reach the end of the self test (see uptime / last boot)."
  cleanup
}

case "${1:-}" in
  check) check ;; prepare) shift; prepare "$@" ;; go) go ;; collect) collect ;; cleanup) cleanup ;;
  *) sed -n 2,16p "$0"; exit 2 ;;
esac
