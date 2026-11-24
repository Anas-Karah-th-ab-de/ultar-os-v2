#!/usr/bin/env python3
"""The desktop session on the Ultar kernel in QEMU, judged from screenshots and driven by keys.

The image carries the static programs of the session and examples/desktoptest (the driver):
ultar-init starts logd, bus, the seat helper, authd, the compositor, the lock screen, the system
dialogs and the desktop shell from the shipped service files. When the driver prints
`DESKTOP-TREIBER: BEREIT` this script
  1. takes a screenshot: the desktop's bar (token `bar` #12151B over the bottom 56 points) with the
     brand mark (accent_text #E3C55A) must be on the screen,
  2. presses Super+L through the virtual USB keyboard (QEMU monitor `sendkey`): the compositor
     locks, the lock screen covers everything (no bar; the mark of the sign-in heading shows),
  3. presses Space (the cover opens the sign-in), types a wrong password and Enter: still locked,
  4. types the test password and Enter: authd checks it (Argon2id), the lock screen unlocks, and the
     desktop's bar is back,
and prints one verdict line, `DESKTOP: ALLES OK` or `DESKTOP: FEHLER <step>`.

   desktop-qemu.py <esp.img> <shot-prefix>

Environment: SMP (2), MEM (1536), WAIT (seconds for the driver, 600), COMMAND (driver line),
MACHINE (q35), DEV (virtio-blk-pci), NIC (virtio-net-pci), VGA (std).
Screenshots: <shot-prefix>-desktop.ppm, -locked.ppm, -wrong.ppm, -unlocked.ppm."""
import os, re, select, socket, subprocess, sys, time

img, prefix = sys.argv[1], sys.argv[2]
PASSWORD = os.environ.get("PASSWORD", "24680135792")
mon = f"/tmp/ultar-desk-mon-{os.getpid()}.sock"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = f"/tmp/ultar-desk-vars-{os.getpid()}.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
# MACHINE, DEV (disk device), NIC, VGA: VMware-like hardware without virtio (tools/kern-test/vmware-like.sh)
args = ["qemu-system-x86_64", "-machine", os.environ.get("MACHINE", "q35"), "-m", os.environ.get("MEM", "1536"),
        "-smp", os.environ.get("SMP", "2"), "-cpu", "max", "-display", "none", "-no-reboot",
        "-vga", os.environ.get("VGA", "std"),
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
        "-drive", f"file={img},if=none,id=b,format=raw", "-device", os.environ.get("DEV", "virtio-blk-pci") + ",drive=b",
        "-netdev", "user,id=n", "-device", os.environ.get("NIC", "virtio-net-pci") + ",netdev=n",
        "-device", "qemu-xhci,id=xh", "-device", "usb-kbd,bus=xh.0", "-device", "usb-tablet,bus=xh.0",
        "-monitor", f"unix:{mon},server,nowait", "-serial", "stdio"]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
log = b""
PROMPT = re.compile(rb"ultar-kern[^\n>]*> ")
BAR, MARK, GROUND = bytes.fromhex("12151B"), bytes.fromhex("E3C55A"), bytes.fromhex("0B0D11")


def read_until(pattern, timeout):
    global log
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.2)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                return None
            log += d
            sys.stdout.write(d.decode("utf-8", "replace"))
            sys.stdout.flush()
            m = re.search(pattern, log)
            if m:
                return m
    return None


def monitor(command):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(mon)
    s.settimeout(10)
    s.recv(4096)  # greeting
    s.sendall(command.encode() + b"\n")
    time.sleep(0.5)
    s.close()


def shot(name):
    path = f"{prefix}-{name}.ppm"
    monitor(f"screendump {path}")
    time.sleep(2)
    data = open(path, "rb").read() if os.path.exists(path) else b""
    head = data.split(b"\n", 3)
    if len(head) < 4:
        return None
    width, height = map(int, head[1].split())
    return width, height, head[3]


def facts(screen):
    """Share of bar-coloured pixels in the bottom 56 rows, mark pixels in them and on the screen."""
    width, height, px = screen
    row = width * 3
    bottom = px[(height - 56) * row:height * row]
    def count(data, c, tol=0):
        n = 0
        for i in range(0, len(data) - 2, 3):
            if all(abs(data[i + k] - c[k]) <= tol for k in range(3)):
                n += 1
        return n
    bar = count(bottom, BAR) / max(1, width * 56)
    # the mark is drawn with edge smoothing: near the token colour counts
    return {"bar": round(bar, 2), "mark_bar": count(bottom, MARK, 40), "mark": count(px, MARK, 40),
            "ground": round(count(px, GROUND) / max(1, width * height), 2)}


def desktop(f):
    return f["bar"] >= 0.5 and f["mark_bar"] >= 20


def locked(f):
    # the cover: no bar (its two quiet buttons at the bottom use a similar fill), the ground everywhere
    return f["bar"] < 0.3 and f["mark"] >= 20 and f["ground"] >= 0.8


def wait_screen(name, test, seconds):
    end = time.time() + seconds
    f = None
    while time.time() < end:
        s = shot(name)
        if s:
            f = facts(s)
            if test(f):
                return True, f
        time.sleep(3)
    return False, f


ACK = re.compile(rb"EINGABE: (\d+)")


def acked():
    """The compositor's count of key events taken (desktoptest relays `--input-ack`)."""
    read_until(rb"(?!)", 0.05)
    found = ACK.findall(log)
    return int(found[-1]) if found else 0


def keys(names):
    """Each key is sent once and the next waits until the compositor took it: press and release
    of every key of the combination (`meta_l-l`: 4 events). A key lost on its way (QEMU, USB,
    the kernel under load) never reaches the count: only then it is sent again."""
    for k in names:
        want = acked() + 2 * len(k.split("-"))
        for attempt in range(3):
            monitor(f"sendkey {k}")
            end = time.time() + 5
            while time.time() < end and acked() < want:
                pass
            if acked() >= want:
                break
            print(f"key {k}: no input ack after {attempt + 1} send(s)")


verdict = "DESKTOP: FEHLER no shell prompt"
try:
    if read_until(PROMPT, 120) is not None:
        log = b""
        command = os.environ.get("COMMAND", "/bin/desktoptest --bin /bin --work /tmp/dt --group-root /sys/fs/cgroup --hold 900")
        p.stdin.write(command.encode() + b"\r")
        p.stdin.flush()
        m = read_until(rb"DESKTOP-TREIBER: (BEREIT|FEHLER[^\r\n]*)", float(os.environ.get("WAIT", "600")))
        if m is None or b"FEHLER" in m.group(0):
            verdict = "DESKTOP: FEHLER " + (m.group(0).decode() if m else "driver not ready")
        else:
            ok, f = wait_screen("desktop", desktop, 120)
            print(f"\nscreen 1 (desktop): {f}")
            if not ok:
                verdict = f"DESKTOP: FEHLER no desktop bar {f}"
            else:
                keys(["meta_l-l"])
                ok, f = wait_screen("locked", locked, 60)
                print(f"screen 2 (locked): {f}")
                if not ok:
                    verdict = f"DESKTOP: FEHLER not locked by Super+L {f}"
                else:
                    # Space opens the sign-in (the compositor acknowledges the key; the
                    # screenshot shows that the lock screen took it)
                    cover = f
                    keys(["spc"])
                    ok, f = wait_screen("signin", lambda g: g != cover, 30)
                    print(f"screen 2b (sign-in): {f}")
                    # a wrong password first: the session stays locked
                    keys(list("13579") + ["ret"])
                    time.sleep(20)
                    s = shot("wrong")
                    f = facts(s) if s else None
                    print(f"screen 3 (wrong password): {f}")
                    if not f or not locked(f):
                        verdict = f"DESKTOP: FEHLER a wrong password unlocked {f}"
                    else:
                        time.sleep(5)  # the wait after one failure (1 s, doubling)
                        keys(["ctrl-a", "backspace"] + list(PASSWORD) + ["ret"])
                        ok, f = wait_screen("unlocked", desktop, 180)
                        print(f"screen 4 (unlocked): {f}")
                        verdict = "DESKTOP: ALLES OK" if ok else f"DESKTOP: FEHLER not unlocked {f}"
        if b"PANIK" in log or b"kernel panic" in log.lower():
            verdict = "DESKTOP: FEHLER kernel panic"
finally:
    p.kill()
    for f in (mon, vars_):
        if os.path.exists(f):
            os.remove(f)
print()
print(verdict)
sys.exit(0 if verdict == "DESKTOP: ALLES OK" else 1)
