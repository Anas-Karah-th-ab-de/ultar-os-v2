#!/bin/bash
# Prepare an Ubuntu machine as an Ultar OS build server (same toolchain as the Beelink):
# Rust 1.97 from Ubuntu, kernel/image tools, QEMU + OVMF for the kernel gate, rb.sh/rb-log.sh.
#   bash setup-builder.sh
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q rustc-1.97 cargo-1.97 rustfmt-1.97 rust-1.97-clippy rust-1.97-src \
  lld clang musl-tools qemu-system-x86 qemu-utils ovmf mtools dosfstools e2fsprogs \
  python3 python3-pil git build-essential pkg-config 2>&1 | tail -3
for b in cargo rustc rustfmt cargo-clippy clippy-driver rustdoc; do
  [ -e /usr/lib/rust-1.97/bin/$b ] && ln -sf /usr/lib/rust-1.97/bin/$b /usr/local/bin/$b
done
mkdir -p /root/ultar-build
cd /root/ultar-build
[ -d ultar-os-v2/.git ] || git clone -q https://github.com/Anas-Karah-th-ab-de/ultar-os-v2
git -C ultar-os-v2 pull -q
cp ultar-os-v2/tools/remote/rb.sh ultar-os-v2/tools/remote/rb-log.sh .
echo "BUILDER READY: $(hostname) $(nproc) cpus, $(cargo --version)"
