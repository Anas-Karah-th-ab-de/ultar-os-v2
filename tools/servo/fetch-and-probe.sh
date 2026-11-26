#!/bin/bash
# Servo for Ultar on a slow, resetting link: the servo crate from crates.io (one small download
# per crate, retried), then SpiderMonkey and the rest; a build of a minimal embedder proves it.
set -x
export CARGO_NET_RETRY=20 CARGO_HTTP_TIMEOUT=120 CARGO_HTTP_MULTIPLEXING=false
mkdir -p /root/servo-build/probe/src && cd /root/servo-build/probe
cat > Cargo.toml <<'T'
[package]
name = "servo-probe"
version = "0.1.0"
edition = "2024"
[dependencies]
servo = "0.8"
[workspace]
T
echo 'fn main() { println!("{}", std::any::type_name::<servo::Servo>()); }' > src/main.rs
for i in $(seq 1 60); do cargo fetch && break; sleep 30; done
echo "FETCH-DONE"
apt-get install -y libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libfontconfig-dev libfreetype-dev libx11-dev libxkbcommon-dev libegl-dev libgl-dev clang llvm python3 2>&1 | tail -2
for i in $(seq 1 20); do cargo build --release 2>&1 | tail -40 && break; sleep 60; done
echo "BUILD-EXIT ${PIPESTATUS[0]}"
