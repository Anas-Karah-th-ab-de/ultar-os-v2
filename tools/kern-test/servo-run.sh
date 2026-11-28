#!/usr/bin/env bash
# The Servo engine host on the Ultar kernel: boots the kernel in QEMU/KVM (no GPU, no VFIO: llvmpipe
# renders on the processor) with the runtime bundle as the root disk and lets `shot` load a page
# through the Tor port and the engine; the picture comes back over the serial console as PNG.
#
# Usage: tools/kern-test/servo-run.sh --bundle DIR [--kernel EFI] [--page URL]
#   --bundle DIR  result of scripts/bundle-servo-engine.sh (usr/lib/ultar/web and tests/{shot,ultar-tor-port})
#   --kernel EFI  the kernel image (default: built here from kernel/ultar-kern, like igpu-run.sh)
#   --page URL    the page (default https://example.org/)
# Environment: LIMIT=<s> hard limit of the VM (900), MEM=<MiB> (4096), SMP=<n> (4), OUT=<dir> for the
# results (default out/servo-run): serial.log, shot.png, verdict.txt.
# Final line: `SERVO: ALLES OK` only when the PNG that came back is not blank (pixels that differ from the
# most frequent colour are counted by this script, not taken from the guest); otherwise `SERVO: FEHLER ...`.
# Needs: qemu-system-x86_64 (KVM), OVMF 4M, mtools, dosfstools, e2fsprogs, python3.
set -uo pipefail
cd "$(dirname "$0")/../.."
bundle=""; kernel=""; page="https://example.org/"
while [ $# -gt 0 ]; do
  case "$1" in
    --bundle) bundle=$2; shift 2 ;;
    --kernel) kernel=$2; shift 2 ;;
    --page) page=$2; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
bundle=$(cd "$bundle" 2>/dev/null && pwd) || { echo "usage: $0 --bundle DIR"; exit 2; }
OUT=${OUT:-$PWD/out/servo-run}; mkdir -p "$OUT"; rm -f "$OUT"/serial.log "$OUT"/shot.png "$OUT"/verdict.txt
B=/usr/lib/ultar/web
for f in "$bundle$B/libexec/ultar-web-engine" "$bundle/tests/shot" "$bundle/tests/ultar-tor-port"; do
  [ -f "$f" ] || { echo "SERVO: FEHLER missing $f"; exit 1; }
done
for c in qemu-system-x86_64 mcopy mkfs.vfat mkfs.ext4 python3; do command -v $c >/dev/null || { echo "SERVO: FEHLER $c missing"; exit 1; }; done
[ -e /dev/kvm ] || { echo "SERVO: FEHLER /dev/kvm missing"; exit 1; }
W=$(mktemp -d /tmp/servo-run.XXXXXX); trap 'rm -rf "$W"' EXIT

# the kernel
if [ -z "$kernel" ]; then
  export PATH=/usr/lib/rust-1.97/bin:$PATH RUSTC_BOOTSTRAP=1
  if ! find /usr/lib/rust-1.97 -name rust-lld 2>/dev/null | grep -q .; then export CARGO_TARGET_X86_64_UNKNOWN_UEFI_LINKER=lld-link; fi
  ( cd kernel/ultar-kern && nice -n 10 cargo build --release 2>&1 | grep -E "^error|Finished" | tail -5 )
  kernel=kernel/ultar-kern/target/x86_64-unknown-uefi/release/ultar-kern.efi
fi
[ -s "$kernel" ] || { echo "SERVO: FEHLER kernel image missing"; exit 1; }

# the root disk: the bundle, and the test programs with the bundle's loader and run path
R=$W/root; mkdir -p "$R"; cp -a "$bundle/usr" "$R/"
mkdir -p "$R$B/tests"
for t in shot ultar-tor-port; do install -m 0755 "$bundle/tests/$t" "$R$B/tests/$t"; done   # already patched by the bundle script
# what a glibc program expects in /etc (the Tor client checks its state folder against the user database)
mkdir -p "$R/etc" "$R/tmp"
printf 'root:x:0:0:root:/tmp:/bin/false\n' > "$R/etc/passwd"
printf 'root:x:0:\n' > "$R/etc/group"
printf 'passwd: files\ngroup: files\nhosts: files dns\n' > "$R/etc/nsswitch.conf"
printf 'nameserver 10.0.2.3\n' > "$R/etc/resolv.conf"
mb=$(( $(du -sm "$R" | cut -f1) * 13 / 10 + 64 ))
truncate -s "${mb}M" "$W/root.img"
mkfs.ext4 -q -L pakete -O ^metadata_csum,^64bit,^has_journal -d "$R" "$W/root.img" || { echo "SERVO: FEHLER mkfs.ext4"; exit 1; }
echo "root disk: ${mb} MB (the bundle $(du -sh "$bundle$B" | cut -f1), read only)"

# the boot medium: kernel, self-test trigger, and the one program the self test runs after its tests
truncate -s 64M "$W/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$W/esp.img" >/dev/null
mmd -i "$W/esp.img" ::/EFI ::/EFI/BOOT ::/bin ::/ultar-kern ::/ultar
mcopy -i "$W/esp.img" "$kernel" ::/EFI/BOOT/BOOTX64.EFI
head -c 262144 /dev/zero | tr '\0' ' ' > "$W/log.txt"; mcopy -i "$W/esp.img" "$W/log.txt" ::/ultar-kern/selbsttest.txt
cat > "$W/testprogramm" <<EOF
$B/tests/shot $B/libexec/ultar-web-engine tor:$B/tests/ultar-tor-port $page /tmp/shot.png 1280 800
HOME=/tmp
SHOT_SERIAL_PNG=1
MESA_SHADER_CACHE_DISABLE=true
SSL_CERT_FILE=$B/share/ssl/cacert.pem
EOF
mcopy -i "$W/esp.img" "$W/testprogramm" ::/ultar/testprogramm
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd"

# the VM: KVM, user network (DHCP from QEMU, the Tor port reaches the internet through it), serial into a file
: > "$OUT/serial.log"
# CPU: Nehalem (SSE4.2, no AVX): the kernel does not save the wide vector registers of a program yet
# (see pakete/llama), so the guest must not see them; CPU=host to try the real processor
timeout "${LIMIT:-900}" qemu-system-x86_64 -enable-kvm -machine q35 -cpu "${CPU:-Nehalem}" -smp "${SMP:-4}" -m "${MEM:-4096}" \
  -display none -vga none -nodefaults -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd -drive "if=pflash,format=raw,file=$W/vars.fd" \
  -drive "file=$W/esp.img,if=none,id=b,format=raw" -device virtio-blk-pci,drive=b,addr=05.0 \
  -drive "file=$W/root.img,if=none,id=r,format=raw,readonly=on" -device virtio-blk-pci,drive=r,addr=06.0 \
  -netdev user,id=n -device virtio-net-pci,netdev=n \
  -serial "file:$OUT/serial.log" -monitor none 2> "$W/qemu.err" &
qp=$!
while kill -0 "$qp" 2>/dev/null; do
  if grep -aqE "extra program done|AUSNAHME|PANIK" "$OUT/serial.log" 2>/dev/null; then sleep 2; kill "$qp" 2>/dev/null; break; fi
  sleep 2
done
wait "$qp" 2>/dev/null || true
echo "--- QEMU: $(head -3 "$W/qemu.err" | tr '\n' ' ')"

# what the guest said
tr -d '\r' < "$OUT/serial.log" > "$W/serial.txt"
sed -n '/\[test\] extra program .*:$/,/extra program done/p' "$W/serial.txt" | grep -v -E '^[A-Za-z0-9+/=]{60,}$' | head -${EXTRA_LINES:-150}
sed -n '/^PNG-BEGIN/,/^PNG-END/p' "$W/serial.txt" | sed '1d;$d' | base64 -d > "$OUT/shot.png" 2>/dev/null || true

# the verdict is made here, from the PNG that came back
python3 -I - "$OUT/shot.png" "$page" > "$OUT/verdict.txt" <<'PY'
import collections, struct, sys, zlib
path, page = sys.argv[1], sys.argv[2]
try:
    d = open(path, "rb").read()
    assert d[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, hdr = 8, b"", None
    while pos < len(d):
        n, t = struct.unpack(">I4s", d[pos:pos + 8]); body = d[pos + 8:pos + 8 + n]; pos += 12 + n
        if t == b"IHDR": hdr = struct.unpack(">IIBBBBB", body)
        elif t == b"IDAT": idat += body
    w, h, depth, ctype, _, _, interlace = hdr
    assert (depth, ctype, interlace) == (8, 2, 0), f"unexpected PNG kind {hdr}"
    raw = zlib.decompress(idat); stride = w * 3; prev = bytearray(stride); hist = collections.Counter()
    for y in range(h):
        base = y * (stride + 1); f = raw[base]; line = bytearray(raw[base + 1:base + 1 + stride])
        for i in range(stride):
            a = line[i - 3] if i >= 3 else 0; b = prev[i]; c = prev[i - 3] if i >= 3 else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + ((a + b) >> 1)) & 255
            elif f == 4:
                p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        hist.update(bytes(line[i:i + 3]) for i in range(0, stride, 3)); prev = line
    total = w * h; bg, bgn = hist.most_common(1)[0]
    drawn = total - bgn
    if drawn >= 300:
        print(f"SERVO: ALLES OK ({w}x{h} PNG of {page}: {drawn} pixels differ from the background colour {bg.hex()}, {len(hist)} colours)")
    else:
        print(f"SERVO: FEHLER the picture is blank ({drawn} non-background pixels of {total})")
except Exception as e:
    print(f"SERVO: FEHLER no usable picture came back ({e})")
PY
cat "$OUT/verdict.txt"
grep -q "SERVO: ALLES OK" "$OUT/verdict.txt"
