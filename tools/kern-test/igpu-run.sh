#!/usr/bin/env bash
# One-shot Intel display test on the target (Beelink GTi15 Ultra, root, Ubuntu), from a checkout:
# builds the Ultar kernel (normal features, the self test is triggered by the boot medium), prints
# the facts the test depends on, runs igpu-passthrough.sh (i915 is always rebound) and reports the
# driver binding afterwards.   Usage: bash tools/kern-test/igpu-run.sh > igpu.log 2>&1
#   STOP_UNITS="a.service b.service"  systemd units that use the GPU (e.g. a Vulkan LLM) and may be
#   stopped for the test; they are started again at the end in every case. Nothing else is stopped.
#   OPTIN=0  boot the VM without the driver's opt-in (expects READ-ONLY OK); default: with opt-in.
set -uo pipefail
cd "$(dirname "$0")/../.."
mkdir -p out out/igpu-bin
rm -f out/igpu-bin/*
DEV=/sys/bus/pci/devices/0000:00:02.0
export PATH=/usr/lib/rust-1.97/bin:$PATH RUSTC_BOOTSTRAP=1
# The UEFI target links with rust-lld. Newer Ubuntu releases ship it in a separate package; fall back to LLVM's lld-link.
if ! find /usr/lib/rust-1.97 -name rust-lld 2>/dev/null | grep -q .; then
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends rust-1.97-llvm >/dev/null 2>&1 || true
fi
if ! find /usr/lib/rust-1.97 -name rust-lld 2>/dev/null | grep -q .; then
  command -v lld-link >/dev/null || DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends lld
  export CARGO_TARGET_X86_64_UNKNOWN_UEFI_LINKER=lld-link
  echo "linker: lld-link (rust-lld not packaged here)"
fi
echo "=== build"
( cd kernel/ultar-kern && cargo build --release 2>&1 | grep -E "^error|warning: unused|Finished" | tail -15 )
EFI=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
[ -s "$EFI" ] || { echo "BUILD FAILED"; exit 1; }
cp "$EFI" out/ultar-kern-intel.efi
echo "=== facts"
for c in /sys/class/drm/card*-DP-3/edid; do [ -s "$c" ] && { echo "edid $c:"; base64 -w0 "$c"; echo; }; done
echo "reset_method: $(cat $DEV/reset_method 2>/dev/null)"
echo "disable_idle_d3: $(cat /sys/module/vfio_pci/parameters/disable_idle_d3 2>/dev/null || echo n/a)"
for t in intel_reg fuser mcopy mkfs.vfat qemu-system-x86_64; do printf '%s=%s ' "$t" "$(command -v $t || echo MISSING)"; done; echo
echo "driver before: $(basename "$(readlink -f $DEV/driver)")"
STOPPED=()
restart_units() {
  for u in "${STOPPED[@]}"; do systemctl start "$u" && echo "unit $u started again" || echo "WARNING: unit $u did not start"; done
  for u in ${STOP_UNITS:-}; do echo "unit $u: $(systemctl is-active "$u" 2>&1)"; done
  STOPPED=()
}
trap restart_units EXIT INT TERM
for u in ${STOP_UNITS:-}; do
  case "$u" in *.service) ;; *) echo "STOP_UNITS: '$u' is not a .service name - ignored"; continue ;; esac
  if systemctl is-active --quiet "$u"; then
    systemctl stop "$u" && STOPPED+=("$u") && echo "unit $u stopped for the test"
  else
    echo "unit $u was not running"
  fi
done
sleep 2
echo "=== passthrough"
# the program that uses the render node (static, built here; skipped without a C compiler)
BINDIR=""
if command -v gcc >/dev/null && gcc -O2 -Wall -Wextra -static -o out/igpu-bin/dritest kernel/ultar-kern/user/dritest.c 2>/dev/null; then
  BINDIR=out/igpu-bin
else
  echo "dritest not built (gcc or static libc missing) - the render node is only checked from inside the kernel"
fi
LIMIT="${LIMIT:-240}" bash tools/kern-test/igpu-passthrough.sh out/ultar-kern-intel.efi $BINDIR 2>&1 | tail -200
echo "=== after"
echo "driver after: $(basename "$(readlink -f $DEV/driver)")"
dmesg | tail -40 | grep -iE "i915|vfio|drm" | tail -15
