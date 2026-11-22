#!/usr/bin/env python3
"""Ultar OS (Ultar-Kern, ohne Linux) unter WSL installieren und starten – Bildschirm über X11 (VcXsrv/Xming/WSLg).

   scripts/wsl-x11.py bauen                  Kern + Installationsmedium bauen (UBUNTU=1: mit Ubuntu-26.04-Programmen)
   scripts/wsl-x11.py installieren [--auto]  Medium starten und auf die Platte der VM installieren
                                             (--auto: Schritte laufen von selbst, im X11-Fenster zu sehen)
   scripts/wsl-x11.py start                  installiertes System starten
   scripts/wsl-x11.py geraete                Kern ohne Fenster starten, Geräte/Treiber/Werkstatt ausgeben
   scripts/wsl-x11.py foto [datei.png]       Bildschirmfoto der laufenden VM (über den QEMU-Monitor)

Die Maschine („WSL-Rechner“) – jedes Gerät hat einen Treiber im Ultar-Kern:
   q35 (Brücken: bruecke) · NVMe-Platte (nvme) · Installationsmedium virtio-blk · Bochs-Grafik (gen-bochs-vga)
   Intel e1000e (gen-intel-eth) · xHCI (gen-xhci) · HD-Audio (gen-hda) · SMBus (gen-smbus) · AHCI (gen-ahci)
   virtio-rng · PS/2-Tastatur · vmmouse (absolute Maus)

Umgebung: ULTAR_VM=~/ultar-wsl-vm  X11=127.0.0.1:0  RES=1280x800  MEM=4096  SMP=4  PLATTE_GB=16  SPRACHE=ar|de|en
KVM wird benutzt, wenn /dev/kvm beschreibbar ist (sudo usermod -aG kvm $USER; danach WSL neu starten), sonst TCG."""
import os, select, shutil, signal, subprocess, sys, time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VM = os.path.expanduser(os.environ.get("ULTAR_VM", "~/ultar-wsl-vm"))
MEDIUM = f"{VM}/ultar-install.img"
PLATTE = f"{VM}/ultar.img"
CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"


def x11():
    """X-Server suchen: X11=… (ausdrücklich), dann VcXsrv/Xming auf der Windows-Seite (gespiegeltes Netz: 127.0.0.1,
    sonst Wirt), dann DISPLAY, zuletzt WSLg (:0)."""
    kandidaten = [os.environ.get("X11"), "127.0.0.1:0"]
    try:
        for z in subprocess.run(["ip", "route", "show", "default"], capture_output=True, text=True).stdout.splitlines():
            kandidaten.append(z.split()[2] + ":0")
        for z in open("/etc/resolv.conf"):
            if z.startswith("nameserver"):
                kandidaten.append(z.split()[1] + ":0")
    except OSError:
        pass
    kandidaten += [os.environ.get("DISPLAY"), ":0"]
    for d in dict.fromkeys(k for k in kandidaten if k):
        try:
            if subprocess.run(["xdpyinfo", "-display", d], capture_output=True, timeout=4).returncode == 0:
                return d
        except (OSError, subprocess.TimeoutExpired):
            continue
    sys.exit("Kein X-Server erreichbar. VcXsrv starten (Zugriffskontrolle aus / „Disable access control“) oder DISPLAY setzen.")


def qemu(platten, fenster=True, titel="Ultar OS", extra=()):
    os.makedirs(VM, exist_ok=True)
    vars_ = f"{VM}/ovmf-vars-{'-'.join(os.path.basename(p) for p, _ in platten)}.fd"
    if not os.path.exists(vars_):
        shutil.copy("/usr/share/OVMF/OVMF_VARS_4M.fd", vars_)
    kvm = os.access("/dev/kvm", os.W_OK)
    w, h = os.environ.get("RES", "1280x800").split("x")
    a = ["qemu-system-x86_64", "-name", titel, "-machine", "q35,vmport=on", "-accel", "kvm" if kvm else "tcg,thread=multi",
         "-cpu", "host" if kvm else "max", "-m", os.environ.get("MEM", "4096"), "-smp", os.environ.get("SMP", "4"),
         "-drive", f"if=pflash,format=raw,readonly=on,file={CODE}", "-drive", f"if=pflash,format=raw,file={vars_}",
         "-device", f"VGA,xres={w},yres={h},vgamem_mb=32", "-netdev", "user,id=n", "-device", "e1000e,netdev=n",
         "-device", "qemu-xhci,id=usb", "-device", "virtio-rng-pci", "-serial", "stdio"]
    for i, (datei, geraet) in enumerate(platten):
        a += ["-drive", f"file={datei},if=none,id=d{i},format=raw", "-device", f"{geraet},drive=d{i}" + (",serial=ultar" if geraet == "nvme" else "")]
    env = dict(os.environ)
    if fenster:
        env["DISPLAY"] = x11()
        # GTK nähme sonst WSLg (Wayland), sobald WAYLAND_DISPLAY gesetzt ist – das Fenster soll auf den X-Server
        env["GDK_BACKEND"] = "x11"
        env.pop("WAYLAND_DISPLAY", None)
        print(f"[wsl-x11] Bildschirm: X11 {env['DISPLAY']} · {'KVM' if kvm else 'TCG (ohne KVM)'}", flush=True)
        a += ["-display", "gtk,zoom-to-fit=off", "-monitor", f"unix:{VM}/monitor.sock,server,nowait"]
        puls = "/mnt/wslg/PulseServer"
        ton = f"pa,id=a,server=unix:{puls}" if os.path.exists(puls) else "none,id=a"
    else:
        a += ["-display", "none"]
        ton = "none,id=a"
    a += ["-audiodev", ton, "-device", "ich9-intel-hda", "-device", "hda-duplex,audiodev=a"]
    return a + list(extra), env


class Seriell:
    """QEMU mit serieller Konsole als Rohr: mitlesen, auf Text warten, Tasten senden."""

    def __init__(self, args, env):
        self.p = subprocess.Popen(args, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = b""

    def lies(self, muster, t):
        ende = time.time() + t
        while time.time() < ende:
            if muster and muster.encode() in self.buf:
                return True
            if select.select([self.p.stdout], [], [], 0.2)[0]:
                d = os.read(self.p.stdout.fileno(), 65536)
                if not d:
                    return False
                self.buf += d
                sys.stdout.write(d.decode("utf-8", "replace"))
                sys.stdout.flush()
        return bool(muster) and muster.encode() in self.buf

    def sende(self, s, pause=1.5):
        self.p.stdin.write(s.encode())
        self.p.stdin.flush()
        self.lies(None, pause)

    def ende(self):
        self.p.send_signal(signal.SIGTERM)
        try:
            self.p.wait(10)
        except subprocess.TimeoutExpired:
            self.p.kill()


def bauen():
    env = dict(os.environ, IMG_OUT=MEDIUM)
    if os.path.isdir("/usr/lib/rust-1.97/bin"):
        env["PATH"] = "/usr/lib/rust-1.97/bin:" + env["PATH"]
    os.makedirs(VM, exist_ok=True)
    subprocess.run(["scripts/build-kern.sh"], cwd=REPO, env=dict(env, IMG="1"), check=True)
    wurzel = os.environ.get("UBUNTU_ROOT") or f"{REPO}/out/compat-root-amd64-u26"
    if os.environ.get("UBUNTU") or os.environ.get("UBUNTU_ROOT"):
        env["UBUNTU_ROOT"] = wurzel
    subprocess.run(["cargo", "build", "--release", "--target", "x86_64-unknown-linux-gnu", "-p", "ultar-init", "-p", "ultar-shell"],
                   cwd=REPO, env=dict(env, RUSTFLAGS="-C target-feature=+crt-static"), check=True)
    medium = ["tools/kern-test/make-install-medium.sh"]
    if "UBUNTU_ROOT" in env and not os.access(f"{wurzel}/root", os.R_OK):
        # die Ubuntu-Wurzel gehört root (debootstrap) – nur dieser Schritt läuft mit sudo
        medium = ["sudo", "env", f"PATH={env['PATH']}", f"UBUNTU_ROOT={wurzel}", f"IMG_OUT={MEDIUM}"] + medium
    subprocess.run(medium, cwd=REPO, env=env, check=True)
    if medium[0] == "sudo":
        subprocess.run(["sudo", "chown", f"{os.getuid()}:{os.getgid()}", MEDIUM], check=True)


def installieren(auto):
    if not os.path.exists(MEDIUM):
        bauen()
    if os.path.exists(PLATTE) and "--neu" not in sys.argv and os.path.getsize(PLATTE) > 0 and platte_belegt():
        sys.exit(f"{PLATTE} enthält schon ein System. Neu installieren: scripts/wsl-x11.py installieren --neu")
    with open(PLATTE, "wb") as f:
        f.truncate(int(os.environ.get("PLATTE_GB", "16")) << 30)
    args, env = qemu([(MEDIUM, "virtio-blk-pci"), (PLATTE, "nvme")], titel="Ultar OS – Installation", extra=["-no-reboot"])
    if not auto:
        print("[wsl-x11] Installation im X11-Fenster bedienen (Maus/Tastatur). Nach „Fertig“ das Fenster schließen, dann: scripts/wsl-x11.py start")
        return subprocess.run(args, env=env).returncode
    q = Seriell(args, env)
    if not q.lies("[installation] gestartet", 180):
        q.ende()
        sys.exit("Installation ist nicht gestartet")
    q.sende({"ar": "1", "de": "2", "en": "3"}.get(os.environ.get("SPRACHE", "de"), "2"))  # Sprache
    q.sende("4", 5)   # Weiter → Geräte (Netzprüfung)
    q.sende("3")      # Weiter → Zielplatte
    q.sende("1")      # Platte wählen
    q.sende("3")      # Weiter → Bestätigen
    q.sende("2")      # Löschen und installieren
    ok = q.lies("[installation] fertig", 1800)
    q.lies(None, 3)
    q.ende()
    print(f"\n[wsl-x11] Installation {'abgeschlossen' if ok else 'NICHT abgeschlossen'}: {PLATTE}")
    return 0 if ok else 1


def platte_belegt():
    with open(PLATTE, "rb") as f:
        f.seek(512)
        return f.read(8) == b"EFI PART"


def start():
    if not os.path.exists(PLATTE) or not platte_belegt():
        sys.exit("Noch nicht installiert: scripts/wsl-x11.py installieren --auto")
    args, env = qemu([(PLATTE, "nvme")])
    if not sys.stdin.isatty():
        # ohne Terminal (Start per Verknüpfung): serielle Konsole in eine Datei
        i = args.index("-serial")
        args[i + 1] = f"file:{VM}/seriell.log"
    return subprocess.run(args, env=env).returncode


def geraete():
    ziel = f"{REPO}/out/ultar-kern-esp.img"   # nur der Kern (Konsole), nicht das installierte System mit Oberfläche
    if not os.path.exists(ziel):
        subprocess.run(["scripts/build-kern.sh"], cwd=REPO, env=dict(os.environ, IMG="1"), check=True)
    args, env = qemu([(ziel, "nvme")], fenster=False, extra=["-snapshot", "-no-reboot"])
    q = Seriell(args, env)
    q.lies("bereit nach", 120) or q.lies("ready after", 1)
    q.lies(None, float(os.environ.get("WARTE", "3")))
    q.buf = b""
    for befehl in ("geraete", "treiber", "werkstatt"):
        q.sende(befehl + "\r", 2)
    q.ende()
    return 0 if b"alle Ger" in q.buf or b"all devices" in q.buf else 1


if __name__ == "__main__":
    befehl = sys.argv[1] if len(sys.argv) > 1 else ""
    if befehl == "bauen":
        bauen()
    elif befehl == "installieren":
        sys.exit(installieren("--auto" in sys.argv))
    elif befehl == "start":
        sys.exit(start())
    elif befehl == "geraete":
        sys.exit(geraete())
    elif befehl == "foto":
        ziel = os.path.abspath(sys.argv[2] if len(sys.argv) > 2 else f"{VM}/foto.png")
        import socket
        s = socket.socket(socket.AF_UNIX)
        s.connect(f"{VM}/monitor.sock")
        time.sleep(0.3)
        s.recv(4096)
        s.send(f"screendump {VM}/foto.ppm\n".encode())
        time.sleep(1.5)
        s.close()
        from PIL import Image
        Image.open(f"{VM}/foto.ppm").save(ziel)
        print(ziel)
    else:
        print(__doc__)
