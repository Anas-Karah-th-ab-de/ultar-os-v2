#!/usr/bin/env python3
"""Compositor on the Ultar kernel in QEMU: the compositor on fb0 and two ultar-ui apps as separate
processes, a screenshot through the QEMU monitor and one verdict line from the screenshot: each app
paints its background in its own token colour (info #7FB2FF, success #6FD08C, dark theme); both
must be on the screen with at least 1 000 visible pixels (the lower window is mostly covered). The in-kernel line of `kerncheck` (capture over
the protocol) is printed when it arrives.

   compositor-qemu.py <esp.img> <shot.ppm>

The image must carry /bin/ultar-compositor and /bin/ultar-compositor-testclient (static builds,
see docs/integration/ultar-compositor.md). Exit 0 and the line `COMPOSITOR-CHECK: OK ...` on success.
Environment: SMP (2), MEM (1024), WAIT (seconds for the verdict, 240)."""
import os, re, select, socket, subprocess, sys, time

img, shot = sys.argv[1], sys.argv[2]
mon = f"/tmp/ultar-comp-mon-{os.getpid()}.sock"
code = "/usr/share/OVMF/OVMF_CODE_4M.fd"
vars_ = f"/tmp/ultar-comp-vars-{os.getpid()}.fd"
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
    """Counts the two app colours in a binary PPM (P6) screenshot."""
    if not os.path.exists(path):
        return None
    data = open(path, "rb").read()
    head = data.split(b"\n", 3)
    width, height = map(int, head[1].split())
    pixels = head[3]
    wanted = {"info": bytes.fromhex("7FB2FF"), "success": bytes.fromhex("6FD08C")}
    counts = {k: 0 for k in wanted}
    for i in range(0, len(pixels) - 2, 3):
        px = pixels[i:i + 3]
        for k, v in wanted.items():
            if px == v:
                counts[k] += 1
    windows = sum(1 for c in counts.values() if c >= 1000)
    state = "OK" if windows == 2 else "FAILED"
    return f"COMPOSITOR-CHECK: {state} windows={windows} frame={width}x{height} pixels={counts}"


verdict = None
inner = None
try:
    if read_until(PROMPT, 90) is None:
        print("\nCOMPOSITOR-QEMU: no shell prompt")
        sys.exit(2)
    log = b""
    command = os.environ.get("COMMAND", "/bin/ultar-compositor-testclient /tmp/ultar-ui kerncheck /bin/ultar-compositor --stay")
    p.stdin.write(command.encode() + b"\r")
    p.stdin.flush()
    wait = float(os.environ.get("WAIT", "240"))
    read_until(rb"drawn[\s\S]*drawn", wait)
    m = read_until(rb"COMPOSITOR-CHECK: [^\r\n]*", 20)
    inner = m.group(0).decode() if m else "kerncheck line not seen"
    time.sleep(3)
    monitor(f"screendump {shot}")
    time.sleep(2)
    verdict = screen_verdict(shot)
finally:
    p.kill()
    for f in (mon, vars_):
        if os.path.exists(f):
            os.remove(f)
print()
print(f"in kernel: {inner}")
print(verdict or "COMPOSITOR-CHECK: FAILED no screenshot")
print(f"screenshot: {shot} ({os.path.getsize(shot) if os.path.exists(shot) else 0} bytes)")
sys.exit(0 if verdict and ": OK " in verdict else 1)
