#!/usr/bin/env bash
# The checks of the app-bus branch, run by rb.sh in the worktree (output cut down to what decides):
#   test      crate tests of the broker, launcher, init, Settings, account service
#   settings  Settings and account tests with the layout renders in /root/shots
#   clippy    clippy -D warnings of the same crates
#   fmt       cargo fmt --check
#   i18n      ultar-i18n-check on Settings' catalogs and those of the services it shows
#   content   the VMware gate's app content steps only, one run (GATE_ONLY=content: no verdict)
#   gate      the VMware input gate, all runs (INPUT_RUNS=... to choose)
#   all       fmt, i18n, test, settings, clippy, then the kernel gate tools/kern-test/ci.sh (which holds the
#             VMware input gate); the last line is "APPBUS-ALL: ci exit <code>"
#   image     the VMware machine (tools/vm/make-vmware.sh, random password) copied with its password file
#             to /root/ultar-build/images/appbus-<commit>/
set -o pipefail
P="-p ultar-busd -p ultar-launcher -p ultar-init -p ultar-settings -p ultar-accountd"
case "${1:-test}" in
  test) cargo test --no-fail-fast $P 2>&1 | grep -E "^error|panicked|FAILED|^---- |error\[|^  *--> |test result" | sort | uniq -c | head -80 ;;
  clippy) cargo clippy $P --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A6 | head -120; echo "clippy exit ${PIPESTATUS[0]}" ;;
  settings) rm -rf /root/shots; ULTAR_CONFORMANCE_OUT=/root/shots cargo test --no-fail-fast -p ultar-settings -p ultar-accountd 2>&1 | grep -E "^test .*FAILED|panicked|keys without|differ|layout problems|^[0-9]+x|test result|error\[|^error|^ +-->" | cut -c1-260 | head -150 ;;
  fmt) cargo fmt --check $P && echo fmt-ok ;;
  i18n) for d in apps/ultar-settings system/core/ultar-settingsd system/core/ultar-devd system/core/ultar-update system/services/ultar-accountd; do
      cargo run -q -p ultar-i18n --bin ultar-i18n-check -- "$d/locales" >/dev/null 2>&1 && echo "i18n ok   $d" || echo "i18n FAIL $d"; done ;;
  content) GATE_ONLY=content INPUT_RUNS="${INPUT_RUNS:-usb}" bash tools/vm/vmware-input-e2e.sh; grep -a "settings\|theme\|store\|files\|GATE_ONLY\|FEHLER" out/vmware/input-*-1.out | tail -60 ;;
  gate) bash tools/vm/vmware-input-e2e.sh; grep -a "settings\|theme\|store\|files\|VMWARE-INPUT" out/vmware/input-*-1.out | tail -80 ;;
  all) for step in fmt i18n test settings clippy; do bash "$0" "$step"; done
    bash tools/kern-test/ci.sh; echo "APPBUS-ALL: ci exit $?" ;;
  image) dest=/root/ultar-build/images/appbus-$(git rev-parse --short HEAD)
    # a new random password for this image (make-vmware.sh writes it beside the image, mode 0600)
    VM_PASSWORD_FILE= bash tools/vm/make-vmware.sh | tail -15 || exit 1
    mkdir -p "$dest" && cp -a out/vmware/UltarOS-v2 out/vmware/UltarOS-v2.zip out/vmware/UltarOS-v2.password "$dest/" \
      && chmod 600 "$dest/UltarOS-v2.password" && (cd "$dest" && sha256sum UltarOS-v2.zip >UltarOS-v2.zip.sha256) \
      && echo "APPBUS-IMAGE: $dest" && ls -la "$dest" "$dest/UltarOS-v2" ;;
esac
