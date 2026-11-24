#!/usr/bin/env bash
# ultar-devd end to end on the Ultar kernel in QEMU: the daemon reads the kernel's device tree
# (/sys/bus, driver ABI from /sys/kernel/ultar/driver_abi), finds a signed stage-A package for the
# e1000 whose built-in driver was unbound before, binds it through /sys/bus/pci/drivers/<d>/bind and
# confirms the binding from the kernel's new state. Then ultar-devd-seat gives the seat's device nodes
# (fb0, input/event0, input/event1) to the seat group: the kernel keeps owner, group and mode of device
# nodes, so every node is "set" and the verdict is "kept" (not "kernel-gated", not "failed").
# Verdict line: "ok    DEVD-QEMU: ALLES OK".
# DEVD=<file>: prebuilt static ultar-devd; SEAT=<file>: prebuilt static ultar-devd-seat; UDRV=<file>:
# prebuilt udrv (host). Image: IMG=1 scripts/build-kern.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
T="${CARGO_TARGET_DIR:-target}"
if [ -z "${DEVD:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release -p ultar-devd --bin ultar-devd --target x86_64-unknown-linux-gnu
  DEVD="$T/x86_64-unknown-linux-gnu/release/ultar-devd"
fi
if [ -z "${SEAT:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release -p ultar-devd --bin ultar-devd-seat --target x86_64-unknown-linux-gnu
  SEAT="$T/x86_64-unknown-linux-gnu/release/ultar-devd-seat"
fi
if [ -z "${UDRV:-}" ]; then
  cargo build -q --release -p ultar-driverpkg --bin udrv
  UDRV="$T/release/udrv"
fi
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
export ULTAR_LANG=en
mkdir -p "$W/payload/builtin" "$W/depot" "$W/keys"
"$UDRV" keygen > "$W/keygen.txt"
sed -n 's/^Private key[^:]*: *//p' "$W/keygen.txt" > "$W/priv"
sed -n 's/^Public key: *//p' "$W/keygen.txt" > "$W/keys/test.keys"
cat > "$W/qemu-e1000.toml" <<'TOML'
# Test package: activates the built-in gen-intel-eth for QEMU's e1000 (8086:100e).
id = "qemu-e1000"
version = "1.0.0"
class = "net"
kernel_abi = ["ultar-kern-abi-1"]
origin = "workshop"
rank = 50
name_key = "driver.gen-intel-eth.name"
description_key = "driver.gen-intel-eth.description"

[match]
pci = ["8086:100e"]

[content]
hal_config = "builtin/qemu-e1000.toml"
TOML
printf 'kernel_driver = "gen-intel-eth"\n' > "$W/payload/builtin/qemu-e1000.toml"
"$UDRV" pack "$W/qemu-e1000.toml" "$W/payload" "$W/depot/qemu-e1000.utr" "$W/priv" >/dev/null
"$UDRV" index "$W/depot" "$W/priv" >/dev/null
IMG="$W/esp.img"; cp out/ultar-kern-esp.img "$IMG"
mmd -i "$IMG" ::/devd ::/devd/depot ::/devd/keys
mcopy -i "$IMG" "$W"/depot/* ::/devd/depot/
mcopy -i "$IMG" "$W/keys/test.keys" ::/devd/keys/
mcopy -o -i "$IMG" "$DEVD" ::/bin/ultar-devd
mcopy -o -i "$IMG" "$SEAT" ::/bin/ultar-devd-seat
printf 'group = 812\n' > "$W/seat.toml"; mcopy -i "$IMG" "$W/seat.toml" ::/devd/seat.toml
DLOG="${DLOG:-$W/log}"
QARGS="${QARGS:-} -netdev user,id=n2 -device e1000,netdev=n2" SMP="${SMP:-2}" WAIT=60 timeout 300 python3 tools/kern-test/qtest.py "$IMG" \
  "/bin/geraetetest --unbind-e1000" "/bin/ultar-devd --once --lang en --depot /devd/depot --keys /devd/keys --state /tmp/devd" \
  "/bin/geraetetest --state-e1000" "/bin/ultar-devd-seat --once --lang en --config /devd/seat.toml" > "$DLOG" 2>&1 || true
fail=0
for want in "e1000 0000:00:" "gelöst" "pci:8086:100e  [working]" "driver: qemu-e1000 1.0.0" "bound"; do
  grep -aqF "$want" "$DLOG" || { echo "FAIL  DEVD-QEMU: missing \"$want\""; fail=1; }
done
grep -aF "GERAETETEST: e1000" "$DLOG" | tail -1 | grep -q " bound" || { echo "FAIL  DEVD-QEMU: e1000 not bound after ultar-devd"; fail=1; }
grep -aqF "device modes: kept" "$DLOG" || { echo "FAIL  DEVD-QEMU: ultar-devd-seat did not keep the modes of the seat's nodes"; fail=1; }
for node in fb0 input/event0 input/event1; do
  grep -aqE "^$node seat-[a-z]+ 0:812 0660 set" "$DLOG" || { echo "FAIL  DEVD-QEMU: $node not given to the seat"; fail=1; }
done
if grep -aqiE "kernel panic|PANIK" "$DLOG"; then echo "FAIL  kernel panic"; fail=1; fi
[ "$fail" = 0 ] || { tail -40 "$DLOG"; exit 1; }
echo "ok    DEVD-QEMU: ALLES OK"
