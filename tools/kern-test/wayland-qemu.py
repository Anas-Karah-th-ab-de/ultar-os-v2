#!/usr/bin/env python3
"""Wayland bridge on the Ultar kernel in QEMU: the compositor on fb0, ultar-wayland and the static
raw-protocol app uwl-demo (window colour #3366CC), started by /bin/uwl-qemu.sh with busybox sh.
A screenshot through the QEMU monitor gives the verdict: at least 10 000 pixels of #3366CC.

   wayland-qemu.py <esp.img> <shot.ppm>

The image must carry /bin/ultar-compositor, /bin/ultar-wayland, /bin/uwl-demo (static builds),
/bin/busybox and /bin/uwl-qemu.sh (tools/kern-test/uwl-qemu.sh). Last line: WAYLAND-QEMU: OK|FAILED.
Environment: SMP (2), MEM (1024), WAIT (seconds for the start line, 120)."""
import os, re, select, socket, subprocess, sys, time

img, shot = sys.argv[1], sys.argv[2]
mon = f"/tmp/ultar-wl-mon-{os.getpid()}.sock"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = f"/tmp/ultar-wl-vars-{os.getpid()}.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
args = ["qemu-system-x86_64", "-machine", "q35", "-m", os.environ.get("MEM", "1024"),
        "-smp", os.environ.get("SMP", "2"), "-cpu", "max", "-display", "none", "-no-reboot", "-vga", "std",
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
        "-drive", f"file={img},if=none,id=b,format=raw", "-device", "virtio-blk-pci,drive=b",
        "-netdev", "user,id=n", "-device", "virtio-net-pci,netdev=n",
        "-monitor", f"unix:{mon},server,nowait", "-serial", "stdio"]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
log = b""
PROMPT = re.compile(rb"ultar-kern[^\n>]*> ")


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
    time.sleep(1.0)
    s.close()


def screen_verdict(path):
    """Counts the demo colour in a binary PPM (P6) screenshot."""
    if not os.path.exists(path):
        return None
    data = open(path, "rb").read()
    head = data.split(b"\n", 3)
    width, height = map(int, head[1].split())
    pixels = head[3]
    want = bytes.fromhex("3366CC")
    n = sum(1 for i in range(0, len(pixels) - 2, 3) if pixels[i:i + 3] == want)
    state = "OK" if n >= 10000 else "FAILED"
    return f"WAYLAND-QEMU: {state} frame={width}x{height} demo_pixels={n}"


verdict = None
try:
    if read_until(PROMPT, 90) is None:
        print("\nWAYLAND-QEMU: FAILED no shell prompt")
        sys.exit(2)
    log = b""
    command = os.environ.get("COMMAND", "/bin/busybox sh /bin/uwl-qemu.sh")
    p.stdin.write(command.encode() + b"\r")
    p.stdin.flush()
    seen = read_until(rb"uwl-demo: drawn", float(os.environ.get("WAIT", "120")))
    print(f"\n(demo drawn line {'seen' if seen else 'not seen'})")
    time.sleep(5)
    monitor(f"screendump {shot}")
    time.sleep(2)
    verdict = screen_verdict(shot)
finally:
    p.kill()
    for f in (mon, vars_):
        if os.path.exists(f):
            os.remove(f)
print()
print(f"screenshot: {shot} ({os.path.getsize(shot) if os.path.exists(shot) else 0} bytes)")
print(verdict or "WAYLAND-QEMU: FAILED no screenshot")
sys.exit(0 if verdict and "OK" in verdict.split()[1] else 1)
