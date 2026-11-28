#!/usr/bin/env python3
"""A GTK program of the Linux area on the Ultar kernel's screen (QEMU): /bin/foreignshow starts the
compositor on fb0 and ultar-wayland with the program (the area of the second disk as its root). After the
start line and WAIT_DRAW seconds a screenshot is taken through the QEMU monitor and written as PNG.

   foreign-gtk-qemu.py <esp.img> <disk.img> <shot.png> -- PROGRAM [ARGS]

Last line: FOREIGN-GTK: SHOT <png> colours=<n> busy=<pixels not the most common colour> (the picture is
looked at; the numbers only say something was drawn). Environment: SMP (2), MEM (2048), WAIT (start, 300 s),
WAIT_DRAW (120 s)."""
import os, re, select, socket, struct, subprocess, sys, time, zlib

img, disk, shot = sys.argv[1], sys.argv[2], sys.argv[3]
program = sys.argv[sys.argv.index("--") + 1:]
mon = f"/tmp/ultar-fg-mon-{os.getpid()}.sock"
ppm = f"/tmp/ultar-fg-{os.getpid()}.ppm"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = f"/tmp/ultar-fg-vars-{os.getpid()}.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
accel = ["-enable-kvm"] if os.path.exists("/dev/kvm") else []
args = ["qemu-system-x86_64", *accel, "-machine", "q35", "-m", os.environ.get("MEM", "2048"),
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


def monitor(command):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(mon)
    s.settimeout(10)
    s.recv(4096)
    s.sendall(command.encode() + b"\n")
    time.sleep(1.5)
    s.close()


def png(path, width, height, rgb):
    raw = b"".join(b"\x00" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                           + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


try:
    if read_until(PROMPT, 120) is None:
        print("\nFOREIGN-GTK: FAILED no shell prompt")
        sys.exit(2)
    log = b""
    command = "/bin/foreignshow --bin /bin --area /data/foreign/linux -- " + " ".join(program)
    p.stdin.write(command.encode() + b"\r")
    p.stdin.flush()
    if read_until(rb"foreignshow: started", float(os.environ.get("WAIT", "300"))) is None:
        print("\nFOREIGN-GTK: FAILED the bridge did not start")
        sys.exit(1)
    read_until(None, float(os.environ.get("WAIT_DRAW", "120")))
    monitor(f"screendump {ppm}")
    time.sleep(2)
    data = open(ppm, "rb").read()
    head = data.split(b"\n", 3)
    width, height = map(int, head[1].split())
    rgb = head[3][:width * height * 3]
    png(shot, width, height, rgb)
    counts = {}
    for i in range(0, len(rgb) - 2, 3 * 7):
        k = rgb[i:i + 3]
        counts[k] = counts.get(k, 0) + 1
    top = max(counts.values())
    busy = (sum(counts.values()) - top) * 7
    print(f"\nFOREIGN-GTK: SHOT {shot} {width}x{height} colours={len(counts)} busy={busy}")
finally:
    p.kill()
    for f in (vars_, ppm):
        try:
            os.remove(f)
        except OSError:
            pass
