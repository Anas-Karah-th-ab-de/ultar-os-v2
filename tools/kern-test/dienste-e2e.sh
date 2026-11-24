#!/usr/bin/env bash
# The system services behind the desktop shell on the Ultar kernel: ultar-init (not PID 1) starts the
# shipped logd, bus, authd, sessiond, powerd, notifyd, calendard and audiod definitions, each as its own
# user; dienstetest plays the shell, the lock screen and two apps: a sign-in through the credential store,
# notification buttons back to their sender only, calendar events of the person in front, "no device"
# for sound and sleep where QEMU has none, a killed service restarted by init, signing out behind the
# lock screen and power off through init.power. Verdict line: "DIENSTE: ALLES OK".
# IMG=<esp image> to use a built kernel image (else scripts/build-kern.sh builds one);
# DIENSTE_BIN=<dir> with static builds of the programs and examples/dienstetest (else they are built here).
set -euo pipefail
cd "$(dirname "$0")/../.."
PROGRAMS="ultar-init ultar-busd ultar-logd ultar-authd ultar-sessiond ultar-powerd ultar-notifyd ultar-calendard ultar-audiod ultar-mediad ultar-netd ultar-btd ultar-volumed"
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${DIENSTE_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-init -p ultar-busd -p ultar-logd -p ultar-auth -p ultar-sessiond -p ultar-powerd \
    -p ultar-notifyd -p ultar-calendard -p ultar-audiod -p ultar-mediad -p ultar-netd -p ultar-btd -p ultar-volumed --bins
  RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu \
    -p ultar-powerd --example dienstetest
  DIENSTE_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# A larger copy of the image: the static programs do not fit the 64 MiB test image.
truncate -s 256M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
for b in $PROGRAMS examples/dienstetest; do
  cp "$DIENSTE_BIN/$b" "$WORK/$(basename "$b")"; strip "$WORK/$(basename "$b")" 2>/dev/null || true
  mcopy -o -i "$WORK/esp.img" "$WORK/$(basename "$b")" ::/bin/
done
LOG="${LOG:-$WORK/dienste-e2e.log}"
SMP="${SMP:-2}" MEM="${MEM:-1024}" WAIT="${WAIT:-300}" timeout 900 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
  "/bin/dienstetest --bin /bin --work /tmp/dienste --group-root /sys/fs/cgroup" > "$LOG" 2>&1 || true
# every service ends with code 0 (power off through init.power ends them in order)
clean=1
for s in logd bus authd sessiond powerd notifyd calendard audiod mediad netd btd volumed; do
  grep -aqE "exit $s pid=[0-9]+ code=0 " "$LOG" || clean=0
done
if grep -aq "DIENSTE: ALLES OK" "$LOG" && [ "$clean" = 1 ] && ! grep -aqiE "kernel panic|PANIK" "$LOG"; then
  echo "ok    DIENSTE: ALLES OK (system services behind the shell, every service ended with code 0)"
  if [ "${VERBOSE:-0}" = 1 ]; then grep -aE "^DIENSTE" "$LOG"; fi
else
  echo "FAIL  DIENSTE"; grep -aE "DIENSTE|ultar-init|refused|exit " "$LOG" | tail -40; exit 1
fi
