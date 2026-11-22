#!/usr/bin/env python3
"""Automatischer Oberflächentest: startet QEMU ohne Fenster, tippt und macht Bildschirmfotos.

  KERNEL=/pfad/vmlinuz scripts/ui-test.py OUTDIR "wait 12" "shot home" "tap 90 560" "shot settings"

Befehle: wait SEK | shot NAME | tap X Y | hold X Y SEK | drag X0 Y0 X1 Y1 | key QCODE | type TEXT (nur a-z0-9 . -)
Koordinaten in Bildschirmpixeln. Die serielle Konsole landet in OUTDIR/serial.log.
"""
import json, os, socket, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
W, H = int(os.environ.get("XRES", 720)), int(os.environ.get("YRES", 1280))


class Qmp:
    def __init__(self, path):
        for _ in range(100):
            try:
                self.s = socket.socket(socket.AF_UNIX)
                self.s.connect(path)
                break
            except OSError:
                time.sleep(0.1)
        self.f = self.s.makefile("rw")
        self.f.readline()
        self.cmd("qmp_capabilities")

    def cmd(self, name, **args):
        self.f.write(json.dumps({"execute": name, "arguments": args}) + "\n")
        self.f.flush()
        while True:
            r = json.loads(self.f.readline())
            if "return" in r or "error" in r:
                return r

    def pos(self, x, y):
        ev = [{"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / (W - 1))}},
              {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / (H - 1))}}]
        self.cmd("input-send-event", events=ev)

    def btn(self, down):
        self.cmd("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": "left"}}])


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    sock = os.path.join(out, "qmp.sock")
    if os.path.exists(sock):
        os.remove(sock)
    env = dict(os.environ, DISPLAY_MODE="none")
    log = open(os.path.join(out, "serial.log"), "w")
    extra = ["-qmp", f"unix:{sock},server,nowait"] + os.environ.get("QEMU_EXTRA", "").split()
    p = subprocess.Popen([os.path.join(ROOT, "scripts/run-qemu.sh")] + extra, env=env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    try:
        q = Qmp(sock)
        for c in sys.argv[2:]:
            a = c.split()
            if a[0] == "wait":
                time.sleep(float(a[1]))
            elif a[0] == "shot":
                ppm = os.path.join(out, a[1] + ".ppm")
                q.cmd("screendump", filename=ppm)
                time.sleep(0.3)
                from PIL import Image
                Image.open(ppm).save(os.path.join(out, a[1] + ".png"))
                os.remove(ppm)
            elif a[0] == "tap":
                q.pos(float(a[1]), float(a[2])); time.sleep(0.15)
                q.btn(True); time.sleep(0.15)
                q.btn(False); time.sleep(float(os.environ.get("TAP_WAIT", 0.8)))
            elif a[0] == "hold":
                q.pos(float(a[1]), float(a[2])); time.sleep(0.15)
                q.btn(True); time.sleep(float(a[3]))
                q.btn(False); time.sleep(float(os.environ.get("TAP_WAIT", 0.8)))
            elif a[0] == "drag":
                x0, y0, x1, y1 = map(float, a[1:5])
                q.pos(x0, y0); time.sleep(0.1); q.btn(True)
                for i in range(1, 11):
                    q.pos(x0 + (x1 - x0) * i / 10, y0 + (y1 - y0) * i / 10); time.sleep(0.06)
                q.btn(False); time.sleep(0.8)
            elif a[0] == "key":
                q.cmd("send-key", keys=[{"type": "qcode", "data": a[1]}]); time.sleep(0.3)
            elif a[0] == "type":
                names = {".": "dot", "-": "minus", " ": "spc", "/": "slash"}
                for ch in c[5:]:
                    q.cmd("send-key", keys=[{"type": "qcode", "data": names.get(ch, ch)}]); time.sleep(0.25)
            else:
                raise SystemExit(f"unbekannter Befehl: {c}")
    finally:
        p.terminate()
        try:
            p.wait(5)
        except subprocess.TimeoutExpired:
            p.kill()


if __name__ == "__main__":
    main()
