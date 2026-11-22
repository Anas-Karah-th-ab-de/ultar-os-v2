#!/bin/bash
# Lokale Angriffsfläche: kaputte Datenträger und kaputte Programme dürfen den Ultar-Kern nicht umwerfen.
#  1. ext4-Partition mit zufällig verfälschten Verwaltungsdaten (Superblock, Gruppen, Inodes, Verzeichnisse) – N Starts
#  2. 40 verfälschte ELF-Programme starten (falsche Kopfdaten, Segmente, Längen) – der Kern lehnt ab oder der
#     Prozess stirbt, der Kern bleibt stehen.
#   tools/kern-test/medien-fuzz.sh [N]
set -u
R=$(cd "$(dirname "$0")/../.." && pwd); cd $R
N=${1:-12}
T=/tmp/medienfuzz; rm -rf $T; mkdir -p $T
[ -f out/ultar-kern-disk.img ] || tools/kern-test/make-test-disk.sh
DISK=out/ultar-kern-disk.img
S2=$(sgdisk -i 2 $DISK | awk '/First sector/{print $3}')
ok=0; fehl=0
c() { if eval "$2"; then echo "OK      $1"; ok=$((ok+1)); else echo "FEHLER  $1"; fehl=$((fehl+1)); fi; }

echo "== ext4 verfälscht ($N Starts)"
ueberlebt=0
for i in $(seq $N); do
  cp $DISK $T/d.img
  python3 - $T/d.img $S2 $i <<'PY'
import random, sys
p, s2, seed = sys.argv[1], int(sys.argv[2]) * 512, int(sys.argv[3])
random.seed(seed)
with open(p, "r+b") as f:
    # Superblock (1024..2048), Gruppendeskriptoren (4096..8192), Bereich der Inode-Tabellen/Verzeichnisse (bis 4 MB)
    for lo, hi, n in ((1024, 2048, 6), (4096, 8192, 20), (8192, 4 << 20, 400)):
        for _ in range(n if seed % 3 else n * 3):
            o = s2 + random.randrange(lo, hi)
            f.seek(o); f.write(bytes([random.randrange(256)]))
PY
  WAIT=25 timeout 120 python3 tools/kern-test/qtest.py $T/d.img "dateien /" "dateien /usr/bin" "bash -c 'ls / | head -3; cat /etc/os-release'" "zeige /etc/passwd" "kerne" > $T/e$i.txt 2>&1
  if grep -aq "Prozessorkernen aktiv" $T/e$i.txt && ! grep -aq "PANIK\|AUSNAHME im Kern" $T/e$i.txt; then ueberlebt=$((ueberlebt+1)); else echo "  Start $i: siehe $T/e$i.txt"; fi
done
c "kaputtes ext4: $ueberlebt/$N Starts ohne Kernabsturz" "[ $ueberlebt -eq $N ]"

echo "== kaputte Programme"
python3 - kernel/ultar-kern/user/hallo-glibc $T/elf <<'PY'
import random, os, struct, sys
src = open(sys.argv[1], "rb").read(); os.makedirs(sys.argv[2], exist_ok=True)
random.seed(7)
for i in range(40):
    b = bytearray(src)
    art = i % 5
    if art == 0:   # ELF-Kopf
        for _ in range(8): b[random.randrange(4, 64)] = random.randrange(256)
    elif art == 1: # Programmkopf-Tabelle
        phoff = struct.unpack_from("<Q", b, 32)[0]
        for _ in range(16): b[phoff + random.randrange(0, 56 * 8)] = random.randrange(256)
    elif art == 2: # abgeschnitten
        b = b[:random.randrange(16, len(b))]
    elif art == 3: # riesige Segmentgrößen
        phoff = struct.unpack_from("<Q", b, 32)[0]
        e = phoff + 56 * random.randrange(0, 4)
        struct.pack_into("<Q", b, e + 40, random.choice([2**40, 2**63, 0xFFFFFFFFFFFFF000]))
    else:          # Zufall überall
        for _ in range(200): b[random.randrange(len(b))] = random.randrange(256)
    open(os.path.join(sys.argv[2], "e%02d" % i), "wb").write(b)
PY
chmod +x $T/elf/*
# auf die FAT-Startpartition legen (dort liegen auch die anderen Testprogramme)
cp $DISK $T/p.img
S1=$(sgdisk -i 1 $DISK | awk '/First sector/{print $3}')
mmd -i $T/p.img@@$((S1*512)) ::/fuzz 2>/dev/null
for f in $T/elf/*; do mcopy -i $T/p.img@@$((S1*512)) $f ::/fuzz/; done
CMDS=(); for i in $(seq -w 0 39); do CMDS+=("start /boot/efi/fuzz/e$i"); done
WAIT=15 timeout 600 python3 tools/kern-test/qtest.py $T/p.img "${CMDS[@]}" "kerne" > $T/elf.txt 2>&1
c "40 kaputte Programme: Kern bleibt stehen" "grep -aq 'Prozessorkernen aktiv' $T/elf.txt && ! grep -aq 'PANIK\|AUSNAHME im Kern' $T/elf.txt"
echo "  abgelehnt: $(grep -ac 'start\|ELF\|ungültig\|Fehler' $T/elf.txt), abgestürzte Prozesse: $(grep -ac '\[proc\].*beendet' $T/elf.txt)"
echo "$ok bestanden, $fehl fehlgeschlagen"
[ $fehl -eq 0 ]
