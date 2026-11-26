#!/usr/bin/env bash
# Sound for calls on the Ultar kernel: pcmtest opens /dev/ultar/pcm (gen-hda, QEMU intel-hda +
# hda-duplex), plays 2 s of 440 Hz at the card's pace and records at the same time. QEMU writes
# what the card played into a WAV file; the script checks that the tone is in it.
# Verdict lines: "PCM: ALLES OK" (in the machine) and "PCM-E2E: ALLES OK" (tone in the recording).
# IMG=<esp image> to use a built kernel image; PCM_BIN=<dir> with a static examples/pcmtest.
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${IMG:-}"
if [ -z "$IMG" ]; then IMG=1 bash scripts/build-kern.sh >/dev/null; IMG=out/ultar-kern-esp.img; fi
if [ -z "${PCM_BIN:-}" ]; then
  RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release --target x86_64-unknown-linux-gnu \
    -p ultar-call --example pcmtest
  PCM_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release"
fi
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
truncate -s 128M "$WORK/esp.img"; mkfs.vfat -F 32 -n ULTAR-KERN "$WORK/esp.img" >/dev/null
mkdir -p "$WORK/content"; mcopy -s -n -i "$IMG" "::/*" "$WORK/content/"
mcopy -s -i "$WORK/esp.img" "$WORK/content"/* ::/
cp "$PCM_BIN/examples/pcmtest" "$WORK/pcmtest"; strip "$WORK/pcmtest" 2>/dev/null || true
mcopy -o -i "$WORK/esp.img" "$WORK/pcmtest" ::/bin/
LOG="${LOG:-$WORK/pcm-e2e.log}"
run() { # $1 audiodev, $2 pcmtest arguments
  QARGS="-audiodev $1 -device intel-hda -device hda-duplex,audiodev=snd" \
    SMP="${SMP:-2}" MEM="${MEM:-512}" WAIT="${WAIT:-120}" timeout 300 python3 tools/kern-test/qtest.py "$WORK/esp.img" \
    "/bin/pcmtest $2" >> "$LOG" 2>&1 || true
  if [ "$(grep -c "PCM: ALLES OK" "$LOG")" -lt "$3" ] || grep -qiE "kernel panic|PANIK" "$LOG"; then
    echo "FAIL  PCM ($1)"; grep -aE "PCM|hda|FEHLER" "$LOG" | tail -20; exit 1
  fi
}
: > "$LOG"
# 1. output and input run at the card's pace (QEMU's silent backend records silence)
run "none,id=snd" "" 1
# 2. what the card plays reaches the speaker: QEMU writes it into a WAV file (no input there)
run "wav,id=snd,path=$WORK/out.wav" "--output-only" 2
check_wav() { python3 - "$1" "$2" <<'PY'
import math, struct, sys
# QEMU finishes the WAV header only on a clean exit; read the fixed 44-byte header directly.
raw = open(sys.argv[1], "rb").read()
if raw[:4] != b"RIFF" or len(raw) < 44:
    sys.exit("PCM-E2E: FEHLER no WAV file (%d bytes)" % len(raw))
ch, rate = struct.unpack("<HI", raw[22:28])
body = raw[44:]
n = len(body) // (2 * ch)
data = struct.unpack("<%dh" % (n * ch), body[: n * ch * 2])
mono = data[int(sys.argv[2])::ch]
loud = [i for i, s in enumerate(mono) if abs(s) > 2000]
if len(loud) < rate:
    sys.exit("PCM-E2E: FEHLER only %d loud samples of %d" % (len(loud), len(mono)))
part = mono[loud[0]:loud[0] + rate // 2]
def power(hz):
    k = 2 * math.cos(2 * math.pi * hz / rate); a = b = 0.0
    for x in part:
        a, b = x + k * a - b, a
    return a * a + b * b - k * a * b
p440, p300 = power(440), power(300)
if p440 < 50 * p300:
    sys.exit("PCM-E2E: FEHLER no 440 Hz in the recording (%.0f vs %.0f)" % (p440, p300))
print("PCM-E2E: ALLES OK (%d frames, 440 Hz played)" % len(loud))
PY
}
check_wav "$WORK/out.wav" 0
# 3. mono (accessibility): the tone only on the left channel, the switch /dev/ultar/pcm-mono on –
#    the card plays it on the right one too
run "wav,id=snd,path=$WORK/mono.wav" "--output-only --mono" 3
check_wav "$WORK/mono.wav" 1
echo "ok    PCM: ALLES OK"
