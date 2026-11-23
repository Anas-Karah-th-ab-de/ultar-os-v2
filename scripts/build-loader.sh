#!/usr/bin/env bash
# Builds the Ultar A/B loader (kernel/ultar-loader, UEFI) → kernel/ultar-loader/target/x86_64-unknown-uefi/release/ultar-loader.efi
# It is \EFI\BOOT\BOOTX64.EFI on ULTAR-EFI of an installed system: it chooses the slot and starts that slot's kernel.
set -euo pipefail
cd "$(dirname "$0")/../kernel/ultar-loader"
[ -d /usr/lib/rust-1.97/bin ] && export PATH=/usr/lib/rust-1.97/bin:$PATH
export RUSTC_BOOTSTRAP=1
env -u CARGO_TARGET_DIR cargo build --release
ls -la target/x86_64-unknown-uefi/release/ultar-loader.efi
