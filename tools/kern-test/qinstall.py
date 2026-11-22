#!/usr/bin/env python3
"""Grafische Installation des Ultar-Kerns in QEMU durchspielen: Installationsmedium (virtio) + leere Zielplatte (NVMe).
   Schritte über die serielle Konsole (Ziffer = n-te Schaltfläche), Bildschirmfotos je Schritt nach out/inst-*.png.
   Danach: nur von der Zielplatte starten und prüfen, dass der Kern hochkommt (ohne Installation)."""
import os, socket, subprocess, sys, time, select
from PIL import Image

OUT = "out"
MED = os.environ.get("MED", f"{OUT}/ultar-install.img")
ZIEL = f"{OUT}/ziel.img"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"


def qemu(disks, mon, tag):
    vars_ = f"/tmp/vars-{tag}.fd"
    subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
    a = ["qemu-system-x86_64", "-machine", "q35", "-m", "1024", "-smp", "2", "-cpu", "max", "-display", "none", "-no-reboot",
         "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
         "-netdev", "user,id=n", "-device", "virtio-net-pci,netdev=n", "-serial", "stdio", "-monitor", f"unix:{mon},server,nowait"]
    for i, (f, dev) in enumerate(disks):
        a += ["-drive", f"file={f},if=none,id=d{i},format=raw", "-device", f"{dev},drive=d{i}" + (",serial=ziel" if dev == "nvme" else "")]
    if os.path.exists(mon):
        os.unlink(mon)
    return subprocess.Popen(a, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

buf = b""
def lies(p, muster, t):
    global buf
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.2)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                break
            buf += d
            sys.stdout.write(d.decode("utf-8", "replace")); sys.stdout.flush()
        if muster and muster.encode() in buf:
            return True
    return False

def foto(mon, name):
    s = socket.socket(socket.AF_UNIX); s.connect(mon); time.sleep(0.3); s.recv(4096)
    s.send(f"screendump /tmp/{name}.ppm\n".encode()); time.sleep(1.5); s.close()
    Image.open(f"/tmp/{name}.ppm").save(f"{OUT}/{name}.png")
    print(f"\n[foto] {OUT}/{name}.png")

def taste(p, s):
    p.stdin.write(s.encode()); p.stdin.flush(); time.sleep(1.2)

# Lauf 1: Installation
p = qemu([(MED, "virtio-blk-pci"), (ZIEL, "nvme")], "/tmp/qi.sock", "a")
ok = lies(p, "[installation] gestartet", 90)
print("\n### Installation gestartet:", ok)
time.sleep(1.5); foto("/tmp/qi.sock", "inst-1-sprache")
taste(p, os.environ.get("SPRACHE", "2"))  # 1=Arabisch 2=Deutsch
foto("/tmp/qi.sock", "inst-1b-deutsch")
taste(p, "4")                      # Weiter
time.sleep(4)                      # Netzprüfung (DHCP/Ping)
foto("/tmp/qi.sock", "inst-2-geraete")
taste(p, "3")                      # Weiter
foto("/tmp/qi.sock", "inst-3-ziel")
taste(p, "1")                      # Platte wählen
taste(p, "3")                      # Weiter
foto("/tmp/qi.sock", "inst-4-bestaetigen")
taste(p, "2")                      # Löschen und installieren
lies(p, "[installation] fertig", 600)
time.sleep(1.5); foto("/tmp/qi.sock", "inst-6-fertig")
taste(p, "1")                      # Zur Konsole
lies(p, "> ", 10)
p.stdin.write(b"partitionen 1\r"); p.stdin.flush(); lies(p, "ultar-compat", 10)
p.kill(); p.wait()
if os.environ.get("NUR_INSTALLATION"):
    sys.exit(0)

# Lauf 2: nur die Zielplatte
buf = b""
p = qemu([(ZIEL, "nvme")], "/tmp/qz.sock", "b")
# installiertes System: Startbild, dann Startseite in der gewählten Sprache
lies(p, "[pci]", 60); time.sleep(0.3); foto("/tmp/qz.sock", "inst-7a-startbild")
seite = lies(p, "[oberflaeche] Startseite", 90)
time.sleep(1.5); foto("/tmp/qz.sock", "inst-7b-startseite")
print(f"\n### Startseite: {seite}")
taste(p, "2"); time.sleep(1); foto("/tmp/qz.sock", "inst-7c-geraete")   # Geräte
taste(p, "1")                                                           # Zurück
taste(p, "1")                                                           # Terminal
ok = lies(p, "ultar-kern:", 30)
time.sleep(1); foto("/tmp/qz.sock", "inst-7d-terminal")
inst = b"[installation] gestartet" in buf
print(f"\n### Zielplatte startet: {ok}, Installation erneut gestartet: {inst}")
p.stdin.write(b"partitionen 0\r"); p.stdin.flush(); lies(p, "ultar-compat", 10)
for c in [b"cat /etc/os-release", b"cat /etc/ultar/sprache", b"bash -c 'echo bash $BASH_VERSION; ls /usr/bin | wc -l'", b"apt-get --version", b"dpkg -l | wc -l", b"hilfe", b"gibtsnicht"]:
    buf = b""
    p.stdin.write(c + b"\r"); p.stdin.flush(); lies(p, "> ", 25)
time.sleep(1); foto("/tmp/qz.sock", "inst-7-ziel-startet")
p.kill()

# Lauf 3: Installationsmedium zuerst, installierte Platte dahinter – das Medium muss die Platte starten
buf = b""
p = qemu([(MED, "virtio-blk-pci"), (ZIEL, "nvme")], "/tmp/q3.sock", "c")
lies(p, "[oberflaeche] Startseite", 120); time.sleep(1); taste(p, "1")
ok = lies(p, "ultar-kern:", 30)
time.sleep(2)
kette = b"bereits installiert" in buf or b"already installed" in buf or b"installed - starting" in buf
inst = b"[installation] gestartet" in buf
print(f"\n### Medium + Platte: Kettenstart={kette}, Installation gestartet={inst}, Konsole={ok}")
p.stdin.write(b"partitionen 0\r"); p.stdin.flush(); lies(p, "> ", 10)
p.stdin.write(b"partitionen 1\r"); p.stdin.flush(); lies(p, "> ", 10)
p.kill()
