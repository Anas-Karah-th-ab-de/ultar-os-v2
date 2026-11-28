#!/usr/bin/env python3
"""QEMU driver of foreign-app-kern.sh: boots the Ultar kernel with the data disk, types the command at the
kernel's prompt, takes a screenshot through the QEMU monitor when the program says "FOREIGN-APP: SHOT" (plus
WAIT_DRAW seconds) and waits for its last line.

   foreign-app-qemu.py <esp.img> <disk.img> <shot.png> "<command>"

Prints "FOREIGN-APP-SHOT: <png> <w>x<h> colours=<n>" after the screenshot and passes the program's own lines
through. Environment: SMP (2), MEM (3072), WAIT (until the shot, 4200 s), WAIT_DRAW (30 s), WAIT_END (1200 s)."""
import os, re, select, socket, struct, subprocess, sys, time, zlib

img, disk, shot, command = sys.argv[1:5]
mon = f"/tmp/ultar-fa-mon-{os.getpid()}.sock"
ppm = f"/tmp/ultar-fa-{os.getpid()}.ppm"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = f"/tmp/ultar-fa-vars-{os.getpid()}.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
accel = ["-enable-kvm"] if os.path.exists("/dev/kvm") else []
args = ["qemu-system-x86_64", *accel, "-machine", "q35", "-m", os.environ.get("MEM", "3072"),
        "-smp", os.environ.get("SMP", "2"), "-cpu", "max", "-display", "none", "-no-reboot", "-vga", "std",
        "-drive", f"if=pflash,format=raw,readonly=on,file={code}", "-drive", f"if=pflash,format=raw,file={vars_}",
        "-drive", f"file={img},if=none,id=b,format=raw", "-device", "virtio-blk-pci,drive=b",
        "-drive", f"file={disk},if=none,id=c,format=raw", "-device", "virtio-blk-pci,drive=c",
        "-netdev", "user,id=n", "-device", "virtio-net-pci,netdev=n",
        "-monitor", f"unix:{mon},server,nowait", "-serial", "stdio"]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
log = b""
PROMPT = re.compile(rb"ultar-kern[^\n>]*> ")


def read_until(pattern, timeout):
    global log
    end = time.time() + timeout
    # what already came (the program may have said its last line before this wait)
    if pattern is not None and re.search(pattern, log):
        return True
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.2)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                return None
            log += d
            sys.stdout.write(d.decode("utf-8", "replace"))
            sys.stdout.flush()
            if pattern is not None and re.search(pattern, log):
                return True
    return None


def monitor(text):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(mon)
    s.settimeout(10)
    s.recv(4096)
    s.sendall(text.encode() + b"\n")
    time.sleep(1.5)
    s.close()


def png(path, width, height, rgb):
    raw = b"".join(b"\x00" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                           + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def screenshot():
    monitor(f"screendump {ppm}")
    time.sleep(2)
    data = open(ppm, "rb").read()
    head = data.split(b"\n", 3)
    width, height = map(int, head[1].split())
    rgb = head[3][:width * height * 3]
    png(shot, width, height, rgb)
    colours = {rgb[i:i + 3] for i in range(0, len(rgb) - 2, 3 * 7)}
    print(f"\nFOREIGN-APP-SHOT: {shot} {width}x{height} colours={len(colours)}", flush=True)


try:
    if read_until(PROMPT, 120) is None:
        print("\nFOREIGN-APP-KERN: FEHLER no shell prompt")
        sys.exit(2)
    log = b""
    p.stdin.write(command.encode() + b"\r")
    p.stdin.flush()
    if read_until(rb"FOREIGN-APP: SHOT|FOREIGN-APP-KERN: [^\n]*\n", float(os.environ.get("WAIT", "4200"))) is None:
        print("\nFOREIGN-APP-KERN: FEHLER no window in time")
        sys.exit(1)
    if b"FOREIGN-APP: SHOT" in log:
        read_until(None, float(os.environ.get("WAIT_DRAW", "30")))
        screenshot()
    read_until(rb"FOREIGN-APP-KERN: [^\n]*\n", float(os.environ.get("WAIT_END", "1200")))
finally:
    p.kill()
    for f in (vars_, ppm):
        try:
            os.remove(f)
        except OSError:
            pass
