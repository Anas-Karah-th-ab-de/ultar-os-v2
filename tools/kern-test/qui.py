#!/usr/bin/env python3
"""Ultar-Kern mit Bildschirm in QEMU: Befehle über die serielle Konsole, Bildschirmfotos und Maus/Tastatur
über den QEMU-Monitor. VMware-ähnlich: q35, vmport (absolute vmmouse), PS/2-Tastatur, e1000e, AHCI/NVMe.

   qui.py <platte.img> <schritte.txt|-> [ausgabe-ordner]
Schritte (je Zeile):
   seriell <text>        Zeile an die serielle Konsole (mit Enter)
   warte <sek>           warten
   warte-auf <text> <s>  bis <text> seriell erscheint (höchstens s Sekunden)
   foto <name>           Bildschirmfoto → <ordner>/<name>.png
   klick <x> <y>         Mausklick an Bildpunkt (vmmouse, absolut)
   taste <qemu-taste>    z. B. ret, a, shift-a, alt-shift_l
   text <text>           Text tippen (US-Tasten)
Umgebung: MEM=2048 SMP=4 DISK=nvme|ahci|virtio, RES=1280x800 (über -device VGA,xres/yres)"""
import os, re, socket, subprocess, sys, time, select

img = sys.argv[1]
schritte = (sys.stdin.read() if sys.argv[2] == "-" else open(sys.argv[2]).read()).splitlines()
aus = sys.argv[3] if len(sys.argv) > 3 else "/tmp/qui"
os.makedirs(aus, exist_ok=True)
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = "/tmp/qui-vars.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
mon = "/tmp/qui-mon.sock"
qmp_pfad = "/tmp/qui-qmp.sock"
for x in (mon, qmp_pfad):
    try:
        os.unlink(x)
    except OSError:
        pass
w, h = os.environ.get("RES", "1280x800").split("x")
disk = os.environ.get("DISK", "nvme")
dev = {"nvme": ["-device", "nvme,drive=b,serial=ultar"], "virtio": ["-device", "virtio-blk-pci,drive=b"],
       "ahci": ["-device", "ide-hd,drive=b,bus=ide.0"]}[disk]
args = ["qemu-system-x86_64", "-machine", "q35,vmport=on", "-m", os.environ.get("MEM", "2048"), "-smp", os.environ.get("SMP", "4"),
        "-cpu", "max", "-display", "none", "-no-reboot",
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
        "-drive", f"file={img},if=none,id=b,format=raw"] + dev + [
        "-device", f"VGA,xres={w},yres={h}", "-netdev", "user,id=n", "-device", "e1000e,netdev=n",
        "-serial", "stdio", "-monitor", f"unix:{mon},server,nowait", "-qmp", f"unix:{qmp_pfad},server,nowait"] + os.environ.get("QARGS", "").split()
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
log = open(os.path.join(aus, "seriell.log"), "wb")
buf = b""

def lesen(t):
    global buf
    ende = time.time() + t
    while time.time() < ende:
        r, _, _ = select.select([p.stdout], [], [], 0.1)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                return
            buf += d
            log.write(d); log.flush()

def warte_auf(text, t):
    ende = time.time() + t
    while time.time() < ende:
        if text.encode() in buf:
            return True
        lesen(0.3)
    return False

for _ in range(50):
    if os.path.exists(mon):
        break
    time.sleep(0.1)
m = socket.socket(socket.AF_UNIX); m.connect(mon)

import json
for _ in range(50):
    if os.path.exists(qmp_pfad):
        break
    time.sleep(0.1)
q = socket.socket(socket.AF_UNIX); q.connect(qmp_pfad); q.recv(65536)
q.sendall(b'{"execute":"qmp_capabilities"}\n'); time.sleep(0.2); q.recv(65536)

def qmp(ereignisse):
    q.sendall((json.dumps({"execute": "input-send-event", "arguments": {"events": ereignisse}}) + "\n").encode())
    time.sleep(0.1)
    q.setblocking(False)
    try:
        q.recv(65536)
    except BlockingIOError:
        pass
    q.setblocking(True)

def maus(x, y, knopf=None):
    e = [{"type": "abs", "data": {"axis": "x", "value": x * 0x7fff // breite}}, {"type": "abs", "data": {"axis": "y", "value": y * 0x7fff // hoehe}}]
    if knopf is not None:
        e.append({"type": "btn", "data": {"down": knopf, "button": "left"}})
    qmp(e)

def monitor(cmd):
    m.sendall((cmd + "\n").encode()); time.sleep(0.15)
    try:
        m.setblocking(False); m.recv(65536)
    except BlockingIOError:
        pass
    m.setblocking(True)

ok = warte_auf("bereit nach", 90)
print("Kern bereit:", ok, flush=True)
breite, hoehe = int(w), int(h)
for z in schritte:
    z = z.strip()
    if not z or z.startswith("#"):
        continue
    befehl, _, rest = z.partition(" ")
    if befehl == "seriell":
        p.stdin.write(rest.encode() + b"\r"); p.stdin.flush(); lesen(0.5)
    elif befehl == "warte":
        lesen(float(rest))
    elif befehl == "warte-auf":
        t, _, s = rest.rpartition(" ")
        print(f"warte-auf {t!r}:", warte_auf(t, float(s)), flush=True)
    elif befehl == "foto":
        ppm = f"/tmp/qui-{rest}.ppm"
        monitor(f"screendump {ppm}"); time.sleep(0.6)
        subprocess.run(["python3", "-c", f"from PIL import Image; Image.open('{ppm}').save('{aus}/{rest}.png')"])
        print("foto", f"{aus}/{rest}.png", flush=True)
    elif befehl == "klick":
        x, y = map(int, rest.split())
        # vmmouse: absolute Koordinaten (QMP, Position und Taste in einem Ereignis)
        maus(x, y); lesen(0.3); maus(x, y, True); lesen(0.2); maus(x, y, False); lesen(0.5)
    elif befehl == "zieh":
        x0, y0, x1, y1 = map(int, rest.split())
        maus(x0, y0); lesen(0.2); maus(x0, y0, True)
        for i in range(1, 9):
            maus(x0 + (x1 - x0) * i // 8, y0 + (y1 - y0) * i // 8); lesen(0.1)
        maus(x1, y1, False); lesen(0.5)
    elif befehl == "taste":
        monitor(f"sendkey {rest}"); lesen(0.3)
    elif befehl == "text":
        for c in rest:
            k = {" ": "spc", ".": "dot", "/": "slash", "-": "minus", ",": "comma"}.get(c, c)
            if c.isupper():
                k = "shift-" + c.lower()
            monitor(f"sendkey {k}"); lesen(0.08)
lesen(1)
p.kill()
print("fertig", flush=True)
