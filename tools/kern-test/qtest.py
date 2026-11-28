#!/usr/bin/env python3
"""Ultar-Kern in QEMU starten und Befehle über die serielle Konsole schicken.
   qtest.py <esp.img> "befehl1" "befehl2" ...   (Umgebung: PREARGS=Geräte vor dem NIC (z. B. ein USB-Controller), SMP=4, MEM=1024, MACHINE=q35, DEV=…, NETDEV=tap,id=n,…, NIC=e1000, WAIT=s)
   "!befehl" geht an den QEMU-Monitor (MONITOR=<unix-socket>, z. B. "!set_link nic0 off"); QARGS braucht dann
   "-monitor unix:<socket>,server,nowait"."""
import codecs, os, re, socket, subprocess, sys, time, select

img = sys.argv[1]
cmds = sys.argv[2:]
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = "/tmp/qtest-vars.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
args = ["qemu-system-x86_64", "-machine", os.environ.get("MACHINE", "q35"), "-m", os.environ.get("MEM", "1024"), "-smp", os.environ.get("SMP", "4"),
        "-cpu", os.environ.get("CPU", "max"), "-display", "none", "-no-reboot",
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
        *os.environ.get("PREARGS", "").split(), "-drive", f"file={img},if=none,id=b,format=raw", "-device", os.environ.get("DEV", "virtio-blk-pci") + ",drive=b",
        "-netdev", os.environ.get("NETDEV", "user,id=n"), "-device", os.environ.get("NIC", "virtio-net-pci") + ",netdev=n", "-serial", "stdio"] + os.environ.get("QARGS", "").split()
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
buf = b""
# The serial output arrives in arbitrary pieces: decode incrementally, so a UTF-8 character split across
# two reads is not turned into replacement characters (the gate greps the log for texts with umlauts).
decoder = codecs.getincrementaldecoder("utf-8")("replace")

# Eingabeaufforderung: "ultar-kern:/pfad> " (optional mit Tastatur-Kennung)
PROMPT = re.compile(rb"ultar-kern[^\n>]*> ")

def read_until(pat, timeout):
    global buf
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.2)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                break
            buf += d
            sys.stdout.write(decoder.decode(d)); sys.stdout.flush()
            if (re.search(PROMPT, buf) if pat == "PROMPT" else pat.encode() in buf):
                return True
    return False

ok = read_until("PROMPT", 60)
for c in cmds:
    buf = b""
    if c.startswith("!"):  # QEMU-Monitor (Kabel ziehen/stecken, Geräte)
        m = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        m.connect(os.environ["MONITOR"]); m.sendall(c[1:].encode() + b"\n"); time.sleep(0.5); m.close()
        print(f"\n[qtest] Monitor: {c[1:]}", flush=True)
        continue
    if c.startswith("@"):  # Eingabe ohne auf Bereitschaft zu warten
        time.sleep(1.0)
        p.stdin.write(c[1:].encode().replace(b"\\n", b"\n") + b"\r"); p.stdin.flush()
        read_until("PROMPT", 5)  # Bereitschaft abholen, sonst gilt sie für den nächsten Befehl
        continue
    p.stdin.write(c.encode() + b"\r"); p.stdin.flush()
    read_until("PROMPT", float(os.environ.get("WAIT", "40")))
# QEMU ended by itself (power off, or reset with -no-reboot): report it for tools/kern-test/ausschalten.sh
rc = p.poll()
if rc is None:
    time.sleep(0.5); rc = p.poll()
print(f"\nQEMU-ENDE: {'selbst ' + str(rc) if rc is not None else 'lief noch'}", flush=True)
p.kill()
