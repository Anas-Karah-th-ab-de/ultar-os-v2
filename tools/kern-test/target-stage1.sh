#!/usr/bin/env bash
# Stage 1 and 2 of the remote hardware test, run ON the target computer (Ubuntu, root):
#   builds the Ultar kernel in remote-test mode and the test programs from this checkout,
#   then boots it under KVM with the real processor, all cores and the real SMBIOS tables.
# Changes nothing on the machine except installing missing packages and files below out/.
# Usage: tools/kern-test/target-stage1.sh            (after: gh repo clone … && cd ultar-os-v2)
# Stage 3 (one-time bare-metal start) is separate: tools/kern-test/remote-boot.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p out
need=(); for p in mtools dosfstools ovmf qemu-system-x86 gcc make; do dpkg -s "$p" >/dev/null 2>&1 || need+=("$p"); done
for p in rustc-1.97 cargo-1.97 rust-1.97-src; do dpkg -s "$p" >/dev/null 2>&1 || need+=("$p"); done
if [ "${#need[@]}" -gt 0 ]; then echo "installing: ${need[*]}"; DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${need[@]}"; fi
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
IMG=1 bash scripts/build-kern.sh >/dev/null 2>out/build-kern.err || { tail -20 out/build-kern.err; exit 1; }                      # normal kernel + ring-3 test programs
# The memory limit test brings the machine to its limits on purpose and needs more than the default 180 s.
( cd kernel/ultar-kern && ULTAR_DEADLINE_MS=900000 cargo build --release --features remote-test 2>&1 | tail -1 )
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p ultar-bus --example selfcheck --target x86_64-unknown-linux-gnu 2>&1 | tail -1
B=out/remote-test; rm -rf "$B"; mkdir -p "$B/bin"
cp kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi "$B/ultar-kern-remote-test.efi"
# every test program of the emulator image (incl. hallo-glibc and the attack program ausbruch), long names kept
mcopy -n -s -i out/ultar-kern-esp.img ::/bin/ "$B/" >/dev/null
cp target/x86_64-unknown-linux-gnu/release/examples/selfcheck "$B/bin/busck"
( cd kernel/ultar-kern && cargo build --release >/dev/null 2>&1 )  # leave the normal kernel as the last build
echo "=== machine: $(cat /sys/class/dmi/id/product_name) · $(nproc) cores · kernel under KVM with real SMBIOS ==="
LIMIT="${LIMIT:-900}" bash tools/kern-test/kvm-on-target.sh "$B/ultar-kern-remote-test.efi" "$B/bin" | tee out/stage1-kvm.log
echo "=== verdict ==="
ok=1; for want in "ALLES OK" "BUS SELFCHECK OK" "ZELLTEST: ALLES OK" "RECHTETEST: ALLES OK" "GERAETETEST: ALLES OK" "WECKERTEST: ALLES OK" "GRENZTEST: ALLES OK" "alle Geräte haben einen Treiber"; do
  if grep -aq "$want" out/stage1-kvm.log; then echo "ok    $want"; else echo "FAIL  $want"; ok=0; fi; done
[ "$ok" = 1 ] && echo "STAGE1 PASSED" || { echo "STAGE1 FAILED"; exit 1; }
