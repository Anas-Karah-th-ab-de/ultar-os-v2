#!/usr/bin/env python3
"""USB hot-plug test of the Ultar kernel's device events: boots the kernel in QEMU with an xHCI controller,
starts `/bin/geraetetest --hotplug` and adds / removes a USB keyboard through the QEMU monitor when the
program says it is ready (and types one key while the keyboard's own input node is open); then adds a USB
mouse and a USB tablet, moves and clicks each (`mouse_set`) and removes them. Prints the serial log; the verdict is the program's marker "GERAETETEST-USB: ALLES OK".
   geraete-hotplug.py <esp.img>      (environment: SMP, MEM, WAIT = seconds for the whole run)"""
import codecs, os, re, socket, subprocess, sys, tempfile, time, select

img = sys.argv[1]
work = tempfile.mkdtemp()
mon_path = os.path.join(work, "monitor")
vars_ = os.path.join(work, "vars.fd")
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS_4M.fd", vars_], check=True)
args = ["qemu-system-x86_64", "-machine", "q35", "-m", os.environ.get("MEM", "1024"), "-smp", os.environ.get("SMP", "2"),
        "-cpu", "max", "-display", "none", "-no-reboot",
        "-drive", "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd", "-drive", f"if=pflash,format=raw,file={vars_}",
        "-drive", f"file={img},if=none,id=b,format=raw", "-device", "virtio-blk-pci,drive=b",
        "-netdev", "user,id=n", "-device", "virtio-net-pci,netdev=n", "-device", "qemu-xhci,id=xhci",
        "-monitor", f"unix:{mon_path},server,nowait", "-serial", "stdio"]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
buf = b""
# decode incrementally: a character split across two reads stays whole (the log is grepped for umlauts)
decoder = codecs.getincrementaldecoder("utf-8")("replace")

def read_until(pat, timeout):
    """Read the serial output until `pat` is in what was read since the last reset of `buf`."""
    global buf
    end = time.time() + timeout
    while time.time() < end:
        r, _, _ = select.select([p.stdout], [], [], 0.2)
        if r:
            d = os.read(p.stdout.fileno(), 65536)
            if not d:
                return False
            buf += d
            sys.stdout.write(decoder.decode(d)); sys.stdout.flush()
            if pat.encode() in buf:
                return True
    return False

def monitor(cmd):
    """Send one monitor command; returns what the monitor answered."""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(mon_path)
    s.settimeout(5)
    time.sleep(0.3)
    s.recv(65536)
    s.sendall(cmd.encode() + b"\n")
    time.sleep(0.5)
    try:
        out = s.recv(65536).decode("utf-8", "replace")
    except socket.timeout:
        out = ""
    s.close()
    return out

def click_on(name):
    """Make the mouse called `name` in `info mice` the active one, move it and click the left button."""
    mice = monitor("info mice")
    for line in mice.splitlines():
        m = re.search(r"Mouse #(\d+): (.*)", line)
        if m and name in m.group(2):
            monitor(f"mouse_set {m.group(1)}")
    monitor("mouse_move 7 5")
    monitor("mouse_button 1")
    monitor("mouse_button 0")

deadline = time.time() + float(os.environ.get("WAIT", "120"))
try:
    if read_until("ultar-kern:/> ", 60):
        p.stdin.write(b"/bin/geraetetest --hotplug\r"); p.stdin.flush()
        if read_until("USB BEREIT (add)", 30):
            monitor("device_add usb-kbd,bus=xhci.0,id=hotkbd")
            # the keyboard's input node is open: one key, pressed and released (QEMU sends it to the newest keyboard)
            if read_until("USB BEREIT (taste)", max(5, deadline - time.time())):
                monitor("sendkey a")
            if read_until("USB BEREIT (remove)", max(5, deadline - time.time())):
                monitor("device_del hotkbd")
                # a mouse and a tablet: each gets its own node; moved and clicked one after the other
                if read_until("USB BEREIT (zeiger)", max(5, deadline - time.time())):
                    monitor("device_add usb-mouse,bus=xhci.0,id=hotmouse")
                    monitor("device_add usb-tablet,bus=xhci.0,id=hottab")
                if read_until("USB BEREIT (klick)", max(5, deadline - time.time())):
                    click_on("QEMU HID Mouse")
                    click_on("QEMU HID Tablet")
                if read_until("USB BEREIT (zeiger weg)", max(5, deadline - time.time())):
                    monitor("device_del hotmouse")
                    monitor("device_del hottab")
                # the whole verdict line and the next prompt: QEMU is stopped only after both arrived
                if read_until("GERAETETEST-USB: ", max(5, deadline - time.time())):
                    buf = buf[buf.rfind(b"GERAETETEST-USB: "):]
                    if read_until("\n", 30):
                        buf = b""
                        read_until("ultar-kern:/> ", 30)
finally:
    # say why the run ended if QEMU ended by itself (a crash of the emulator is not the kernel's result)
    code = p.poll()
    if code is not None:
        sys.stdout.write(f"\n[harness] QEMU ended by itself with code {code}\n"); sys.stdout.flush()
    p.kill()
    p.wait()
    subprocess.run(["rm", "-rf", work])
