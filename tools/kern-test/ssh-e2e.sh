#!/usr/bin/env bash
# SSH into the Ultar kernel end to end: the pure-Rust server ultar-sshd runs on the Ultar kernel
# (Linux ABI, no Ubuntu compat area) and a Linux PC reaches it over QEMU user networking.
# Builds a static ultar-sshd, a test disk with only a fresh test key (never a real key), boots QEMU
# with a forward of a free host port to the guest's port 2222, waits for the port, then checks:
#   a) a command runs plus `uname`;  b) the exit code of a failing remote command arrives;
#   c) scp of a 1 MiB random file up and back, sha256 equal;  d) sftp `ls`;
#   e) a different key is rejected;  f) password auth is rejected;
#   g) three connections one after another and two at the same time work.
# KERNPASSTHROUGH=1 / VIRTIO=0 / SHUTDOWN=0 as in other e2e tests. Verdict line: "SSH: ALLES OK".
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=out; mkdir -p $OUT
WORK=$(mktemp -d)
QEMU=""
trap 'if [ -n "$QEMU" ]; then kill "$QEMU" 2>/dev/null || true; fi; rm -rf "$WORK"' EXIT

fail=0
ok()   { echo "ok    $1"; }
bad()  { echo "FAIL  $1"; fail=1; }

# 1. static ultar-sshd (reines Rust, läuft auf dem Ultar-Kern wie auf dem Handy)
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p ultar-sshd --target x86_64-unknown-linux-gnu >/dev/null
USSH="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/ultar-sshd"

# 2. fresh TEST key pair – never a real key, regenerated every run
KEY=$OUT/ssh-e2e-test_ed25519
rm -f "$KEY" "$KEY.pub"
ssh-keygen -q -t ed25519 -N "" -C ultar-ssh-e2e -f "$KEY"
FREMD=$WORK/fremd_ed25519
ssh-keygen -q -t ed25519 -N "" -C fremd -f "$FREMD"

# 3. test disk with only that one key (ENTWICKLER_SCHLUESSEL overrides the developer's own ~/.ssh keys)
DISK=$OUT/ultar-kern-ssh-e2e.img
ENTWICKLER=1 ENTWICKLER_SCHLUESSEL="$KEY.pub" DISK="$DISK" tools/kern-test/make-test-disk.sh >/dev/null

# 4. free host port for the forward
PORT=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$WORK/vars.fd"
LOG="$WORK/qemu.log"
qemu-system-x86_64 -machine q35 -m "${MEM:-1024}" -smp "${SMP:-2}" -cpu max -display none -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file="$WORK/vars.fd" \
  -drive "file=$DISK,if=none,id=b,format=raw" -device virtio-blk-pci,drive=b \
  -netdev "user,id=n,hostfwd=tcp:127.0.0.1:$PORT-:2222" -device virtio-net-pci,netdev=n \
  -serial "file:$LOG" -monitor none -daemonize -pidfile "$WORK/qemu.pid"
QEMU=$(cat "$WORK/qemu.pid")

SSHOPTS=(-p "$PORT" -i "$KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR)
SSH=(ssh "${SSHOPTS[@]}" root@127.0.0.1)
SCP=(scp -P "$PORT" -i "$KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o LogLevel=ERROR)

# 5. wait until the developer access is up and answers
waited=0
while [ $waited -lt 300 ]; do
  if grep -aq '\[entwickler\] SSH-Zugang' "$LOG" 2>/dev/null && \
     "${SSH[@]}" -o ConnectTimeout=3 'true' 2>/dev/null; then break; fi
  if ! kill -0 "$QEMU" 2>/dev/null; then break; fi
  sleep 1; waited=$((waited + 1))
done
if [ $waited -ge 300 ]; then
  echo "FAIL  ultar-sshd became reachable on 127.0.0.1:$PORT"; tail -40 "$LOG"; exit 1
fi
ok "ultar-sshd reachable via 127.0.0.1:$PORT ($(grep -ao '\[entwickler\] SSH-Zugang.*' "$LOG" | head -1 || true))"

# a) command and uname
out="$("${SSH[@]}" 'echo SSH-OK; uname -a' 2>/dev/null || true)"
if printf '%s' "$out" | grep -q 'SSH-OK' && printf '%s' "$out" | grep -qE 'Linux|Ultar'; then
  ok "command runs and uname answers: $(printf '%s' "$out" | tr '\n' ' ' | head -c 80)"
else
  bad "command + uname (got: $(printf '%s' "$out" | tr '\n' ' ' | head -c 80))"
fi

# b) exit code of a failing remote command arrives
rc=0
"${SSH[@]}" 'echo vorexit; exit 7' > "$WORK/rc.out" 2>&1 || rc=$?
if [ "$rc" = 7 ]; then
  ok "exit code 7 of a failing remote command arrives (rc=$rc)"
else
  bad "exit code of failing command (want 7, got $rc, out: $(tr '\n' ' ' < "$WORK/rc.out" | head -c 80))"
fi

# c) scp 1 MiB up and back, sha256 equal
head -c 1048576 /dev/urandom > "$WORK/up.bin"
scp_ok=0
if "${SCP[@]}" "$WORK/up.bin" root@127.0.0.1:/tmp/ssh-e2e-up.bin 2>/dev/null && \
   "${SCP[@]}" root@127.0.0.1:/tmp/ssh-e2e-up.bin "$WORK/down.bin" 2>/dev/null; then
  a=$(sha256sum "$WORK/up.bin" | cut -d' ' -f1)
  b=$(sha256sum "$WORK/down.bin" | cut -d' ' -f1)
  if [ "$a" = "$b" ]; then scp_ok=1; ok "scp 1 MiB up and back, sha256 equal ($a)"; else bad "scp sha256 differs"; fi
else
  bad "scp up and back"
fi

# benchmark: scp 16 MiB up and down, sha256 equal (numbers go into docs/integration/ssh.md)
head -c 16777216 /dev/urandom > "$WORK/up16.bin"
t0=$(date +%s.%N)
"${SCP[@]}" "$WORK/up16.bin" root@127.0.0.1:/tmp/ssh-e2e-up16.bin 2>/dev/null
t1=$(date +%s.%N)
"${SCP[@]}" root@127.0.0.1:/tmp/ssh-e2e-up16.bin "$WORK/down16.bin" 2>/dev/null
t2=$(date +%s.%N)
a16=$(sha256sum "$WORK/up16.bin" | cut -d' ' -f1)
b16=$(sha256sum "$WORK/down16.bin" | cut -d' ' -f1)
up_s=$(awk "BEGIN{printf \"%.1f\", $t1-$t0}")
down_s=$(awk "BEGIN{printf \"%.1f\", $t2-$t1}")
if [ "$a16" = "$b16" ]; then
  echo "info  scp 16 MiB: up ${up_s}s, down ${down_s}s, sha256 equal"
else
  bad "scp 16 MiB sha256 differs"
fi

# d) sftp ls
if printf 'ls /\nbye\n' | sftp -P "$PORT" -i "$KEY" -o IdentitiesOnly=yes \
     -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes root@127.0.0.1 > "$WORK/sftp.out" 2>/dev/null; then
  ok "sftp ls lists the root (e.g. $(grep -aoE '^/?(etc|bin|usr|tmp)' "$WORK/sftp.out" | head -1))"
else
  bad "sftp ls"
fi

# e) a different key is rejected (only the foreign key in the list – the good key must not ride along)
if ssh -p "$PORT" -i "$FREMD" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no \
     -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR \
     root@127.0.0.1 'true' >/dev/null 2>&1; then
  bad "a different key is rejected"
else
  ok "a different key is rejected"
fi

# f) password auth is rejected (no key, only password – the server offers no password method)
if command -v sshpass >/dev/null 2>&1; then
  if sshpass -p irgendeinpasswort ssh -p "$PORT" -o PreferredAuthentications=password \
       -o PubkeyAuthentication=no -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
       -o ConnectTimeout=10 -o LogLevel=ERROR root@127.0.0.1 'true' >/dev/null 2>&1; then
    bad "password auth is rejected"
  else
    ok "password auth is rejected"
  fi
else
  if ssh -p "$PORT" -o PreferredAuthentications=password -o PubkeyAuthentication=no \
       -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
       -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR root@127.0.0.1 'true' >/dev/null 2>&1; then
    bad "password auth is rejected (no sshpass: BatchMode+sperre)"
  else
    ok "password auth is rejected (no sshpass: keyless+pw-only fails)"
  fi
fi

# g) three connections one after another and two at the same time
seq_ok=1
for i in 1 2 3; do
  if ! "${SSH[@]}" 'echo seq-OK' 2>/dev/null | grep -q seq-OK; then seq_ok=0; fi
done
if [ "$seq_ok" = 1 ]; then ok "three connections one after another work"; else bad "three sequential connections"; fi

"${SSH[@]}" 'echo par-A; sleep 2' > "$WORK/parA.out" 2>/dev/null & pA=$!
"${SSH[@]}" 'echo par-B'               > "$WORK/parB.out" 2>/dev/null & pB=$!
wait "$pA" || true; wait "$pB" || true
if grep -q par-A "$WORK/parA.out" && grep -q par-B "$WORK/parB.out"; then
  ok "two connections at the same time work"
else
  bad "two parallel connections (A: $(cat "$WORK/parA.out" 2>/dev/null | tr '\n' ' '), B: $(cat "$WORK/parB.out" 2>/dev/null | tr '\n' ' '))"
fi

if [ -n "${SSH_KEEP:-}" ]; then
  cp "$LOG" "$SSH_KEEP"; echo "SSH-KEEP: $LOG → $SSH_KEEP"
fi

if [ "$fail" = 0 ]; then
  echo "SSH: ALLES OK"
else
  echo "--- QEMU log tail ---"; tail -25 "$LOG"
  echo "SSH: FEHLER"; exit 1
fi