#!/usr/bin/env python3
"""The installed VMware disk in QEMU, driven ONLY through emulated input devices (QMP input-send-event).

No test hook of the system is used: keys and pointer go through QEMU's emulated USB keyboard and tablet
(xHCI) or its PS/2 keyboard and VMware absolute pointer (i8042 + vmport), exactly the devices the kernel's
drivers see in VMware Workstation. Screenshots come from QEMU (QMP screendump).

   vmware-input.py gate  DISK raw|vmdk|qcow2 PREFIX
       boot -> the lock screen covers everything (no bar) -> password typed -> the desktop bar ->
       click the start button -> the start menu -> click Settings, Files, Store, Phone, Browser in it:
       each app's window appears. Verdict line "VMWARE-INPUT: ALLES OK" or "VMWARE-INPUT: FEHLER <step>".
   vmware-input.py probe DISK raw|vmdk|qcow2 PREFIX STEP...
       boot, then the steps: shot:NAME  wait:S  type:TEXT  key:QCODE[+QCODE]  click:X,Y  move:X,Y
       (X, Y in screen pixels), password (the PASSWORD), until:REGEX (serial).

Environment: INPUT (usb | ps2: no USB at all, i440FX, VMware SVGA II), CONTROLLER (nvme | ahci),
PASSWORD or PASSWORD_FILE, SMP (4), MEM (2048), ACCEL (kvm when /dev/kvm is usable, else tcg), WAIT (600),
VGA (std for usb, vmware for ps2). The services log to the log service, not to the console: after a failed
run (or with LOGS=1) the log directory is read from the data partition of the run's disk layer
(PREFIX-log/, and with ULOG=<ulog program> as text in PREFIX-log.txt)."""
import json, os, re, select, shutil, socket, subprocess, sys, tempfile, time

CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"
VARS = "/usr/share/OVMF/OVMF_VARS_4M.fd"
WAIT = float(os.environ.get("WAIT", "600"))
INPUT = os.environ.get("INPUT", "usb")
# tokens of the desktop (dark theme): the bar, the brand mark (accent_text), the ground of the lock screen
BAR, MARK, GROUND = bytes.fromhex("12151B"), bytes.fromhex("E3C55A"), bytes.fromhex("0B0D11")
QCODE = {" ": "spc", "-": "minus", "=": "equal", ".": "dot", ",": "comma", "/": "slash", ";": "semicolon",
         "'": "apostrophe", "[": "bracket_left", "]": "bracket_right", "\\": "backslash", "`": "grave_accent"}


class Vm:
    def __init__(self, disk, fmt, prefix):
        self.dir = tempfile.mkdtemp(prefix="ultar-vmin-")
        self.qmp_path = os.path.join(self.dir, "qmp")
        shutil.copy(VARS, os.path.join(self.dir, "vars.fd"))
        # a copy-on-write layer: the disk stays as it is, the layer is read after the run (logs)
        self.overlay = os.path.join(self.dir, "layer.qcow2")
        subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", os.path.abspath(disk), "-F", fmt, self.overlay],
                       check=True)
        kvm = os.environ.get("ACCEL") == "kvm" or (os.environ.get("ACCEL") is None and os.access("/dev/kvm", os.W_OK))
        controller = os.environ.get("CONTROLLER", "nvme")
        ps2 = INPUT == "ps2"
        args = ["qemu-system-x86_64", "-machine", ("pc" if ps2 else "q35") + ",vmport=on",
                "-accel", "kvm" if kvm else "tcg", "-cpu", "host" if kvm else "max",
                "-m", os.environ.get("MEM", "2048"), "-smp", os.environ.get("SMP", "4"),
                "-display", "none",
                "-drive", f"if=pflash,format=raw,readonly=on,file={CODE}",
                "-drive", f"if=pflash,format=raw,file={self.dir}/vars.fd",
                "-netdev", "user,id=n", "-device", ("e1000" if ps2 else "e1000e") + ",netdev=n",
                "-qmp", f"unix:{self.qmp_path},server,nowait", "-serial", "stdio",
                "-drive", f"file={self.overlay},if=none,id=d0,format=qcow2",
                "-device", "nvme,serial=ultar0,drive=d0" if controller == "nvme" else "ide-hd,bus=ide.0,drive=d0"]
        # VGA_DEVICE: a display device with its own firmware resolution (e.g. VGA,xres=1920,yres=1080)
        if os.environ.get("VGA_DEVICE"):
            args += ["-vga", "none", "-device", os.environ["VGA_DEVICE"]]
        else:
            args += ["-vga", os.environ.get("VGA", "vmware" if ps2 else "std")]
        if not ps2:
            args += ["-device", "qemu-xhci,id=xh", "-device", "usb-kbd,bus=xh.0", "-device", "usb-tablet,bus=xh.0"]
        env = dict(os.environ, TMPDIR=self.dir)
        self.p = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
        self.buf = b""
        self.log = open(prefix + ".log", "wb")
        self.prefix = prefix
        self.qmp = None
        self.size = (0, 0)

    def pump(self, timeout):
        end = time.time() + timeout
        while True:
            left = end - time.time()
            r, _, _ = select.select([self.p.stdout], [], [], max(0, min(0.2, left)))
            if r:
                data = os.read(self.p.stdout.fileno(), 65536)
                if not data:
                    return False
                self.buf += data
                self.log.write(data)
                self.log.flush()
            if left <= 0:
                return True

    def until(self, pattern, timeout):
        rx = re.compile(pattern)
        end = time.time() + timeout
        while time.time() < end:
            if rx.search(self.buf):
                return True
            if not self.pump(0.5) or self.p.poll() is not None:
                return rx.search(self.buf) is not None
        return False

    def command(self, name, **arguments):
        if self.qmp is None:
            for _ in range(50):
                try:
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    s.connect(self.qmp_path)
                    break
                except OSError:
                    time.sleep(0.2)
            s.settimeout(30)
            self.qmp = s.makefile("rw")
            self.qmp.readline()  # greeting
            self.command("qmp_capabilities")
        self.qmp.write(json.dumps({"execute": name, "arguments": arguments}) + "\n")
        self.qmp.flush()
        while True:
            reply = json.loads(self.qmp.readline())
            if "return" in reply or "error" in reply:
                if "error" in reply:
                    print(f"qmp {name}: {reply['error']}")
                return reply

    def events(self, events):
        self.command("input-send-event", events=events)

    def key(self, combo):
        codes = combo.split("+")
        for c in codes:
            self.events([{"type": "key", "data": {"down": True, "key": {"type": "qcode", "data": c}}}])
            self.pump(0.04)
        for c in reversed(codes):
            self.events([{"type": "key", "data": {"down": False, "key": {"type": "qcode", "data": c}}}])
            self.pump(0.04)
        # a person's pace: the kernel polls the keyboards, a press and release within one poll of a busy
        # machine can be lost (tools/kern-test/desktop-qemu.py waits for an acknowledgement instead)
        self.pump(0.25)

    def type(self, text):
        for ch in text:
            if ch.isalpha() and ch.isupper():
                self.key("shift+" + ch.lower())
            elif ch.isalnum():
                self.key(ch)
            elif ch in QCODE:
                self.key(QCODE[ch])
            else:
                raise ValueError(f"no key for {ch!r}")

    def move(self, x, y):
        w, h = self.size
        ax = min(0x7FFF, int(x * 0x8000 / max(1, w)))
        ay = min(0x7FFF, int(y * 0x8000 / max(1, h)))
        self.events([{"type": "abs", "data": {"axis": "x", "value": ax}},
                     {"type": "abs", "data": {"axis": "y", "value": ay}}])
        self.pump(0.2)

    def click(self, x, y):
        self.move(x, y)
        self.move(x, y)  # a second report: the pointer is there before the button
        for down in (True, False):
            self.events([{"type": "btn", "data": {"down": down, "button": "left"}}])
            self.pump(0.12)

    def shot(self, name):
        path = f"{self.prefix}-{name}.ppm"
        if os.path.exists(path):
            os.remove(path)
        self.command("screendump", filename=path)
        for _ in range(20):
            if os.path.exists(path) and os.path.getsize(path) > 16:
                break
            self.pump(0.1)
        data = open(path, "rb").read()
        head = data.split(b"\n", 3)
        w, h = map(int, head[1].split())
        self.size = (w, h)
        try:
            from PIL import Image
            Image.open(path).save(f"{self.prefix}-{name}.png")
        except Exception:
            pass
        return Screen(w, h, head[3])

    def end(self, logs=False):
        try:
            if self.p.poll() is None:
                if self.qmp is not None:
                    try:
                        self.command("stop")  # no more writes: the layer is consistent as it is
                    except Exception:
                        pass
                self.p.kill()
            self.p.wait()
            if logs:
                self.logs()
        finally:
            self.log.close()
            shutil.rmtree(self.dir, ignore_errors=True)

    def logs(self):
        """The log service's directory from the data partition (ultar-data) of the run's disk layer to
        PREFIX-log/, and with ULOG=<ulog program> its records as text (PREFIX-log.txt)."""
        out = self.prefix + "-log"
        raw = os.path.join(self.dir, "disk.raw")
        try:
            subprocess.run(["qemu-img", "convert", "-O", "raw", self.overlay, raw], check=True)
            with open(raw, "rb") as f:
                f.seek(512)
                head = f.read(92)
                entries, count, size = (int.from_bytes(head[72:80], "little"), int.from_bytes(head[80:84], "little"),
                                        int.from_bytes(head[84:88], "little"))
                f.seek(entries * 512)
                table = f.read(count * size)
            part = None
            for i in range(count):
                e = table[i * size:(i + 1) * size]
                name = e[56:128].decode("utf-16-le", "ignore").rstrip("\0")
                if name == "ultar-data":
                    part = (int.from_bytes(e[32:40], "little"), int.from_bytes(e[40:48], "little"))
            if not part:
                print("logs: no ultar-data partition")
                return
            data = os.path.join(self.dir, "data.ext4")
            subprocess.run(["dd", f"if={raw}", f"of={data}", "bs=512", f"skip={part[0]}",
                            f"count={part[1] - part[0] + 1}", "status=none"], check=True)
            os.remove(raw)
            shutil.rmtree(out, ignore_errors=True)
            os.makedirs(out)
            subprocess.run(["debugfs", "-R", f"rdump /ultar/log {out}", data], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)
            ulog = os.environ.get("ULOG")
            if ulog and os.path.exists(ulog):
                subprocess.run([ulog, "export", os.path.join(out, "log"), self.prefix + "-log.txt", "--include-kernel-text"],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            print(f"logs: {out}" + (f", {self.prefix}-log.txt" if ulog else ""))
        except Exception as e:  # the logs help, they never decide a verdict
            print(f"logs: {e}")


class Screen:
    def __init__(self, w, h, px):
        self.w, self.h, self.px = w, h, px

    def count(self, colour, rows=None, tol=0):
        """Pixels of `colour` (within tol) in the rows range (all rows when None); every 2nd pixel."""
        a, b = rows if rows else (0, self.h)
        data = self.px[a * self.w * 3:b * self.w * 3]
        if tol == 0:
            n, at = 0, data.find(colour)
            while at != -1:
                if at % 3 == 0:
                    n += 1
                at = data.find(colour, at + 1)
            return n
        n = 0
        for i in range(0, len(data) - 2, 6):
            if abs(data[i] - colour[0]) <= tol and abs(data[i + 1] - colour[1]) <= tol and abs(data[i + 2] - colour[2]) <= tol:
                n += 2
        return n

    def facts(self):
        bar = self.count(BAR, (self.h - 56, self.h)) / max(1, self.w * 56)
        return {"bar": round(bar, 2), "mark_bar": self.count(MARK, (self.h - 56, self.h), 40),
                "mark": self.count(MARK, None, 40), "ground": round(self.count(GROUND) / max(1, self.w * self.h), 2)}

    def diff(self, other, box=None):
        """Share of differing pixels (sampled every 4th) inside box (x0, y0, x1, y1)."""
        x0, y0, x1, y1 = box or (0, 0, self.w, self.h)
        n = d = 0
        for y in range(y0, y1, 4):
            row = y * self.w * 3
            for x in range(x0, x1, 4):
                i = row + x * 3
                n += 1
                if self.px[i:i + 3] != other.px[i:i + 3]:
                    d += 1
        return d / max(1, n)


def desktop(f):
    return f["bar"] >= 0.5 and f["mark_bar"] >= 20


def locked(f):
    return f["bar"] < 0.3 and f["ground"] >= 0.6


def boot(vm):
    if not vm.until(rb"\[ultar-init[^\]]*\] settled", WAIT):
        return "no 'settled' from ultar-init"
    return None


def password():
    if os.environ.get("PASSWORD"):
        return os.environ["PASSWORD"]
    f = os.environ.get("PASSWORD_FILE")
    return open(f).read().split("\n")[0] if f else ""


def wait_for(vm, name, test, seconds):
    end = time.time() + seconds
    s = None
    while time.time() < end:
        s = vm.shot(name)
        if test(s):
            return True, s
        vm.pump(1.5)
    return False, s


def probe(vm, steps):
    err = boot(vm)
    if err:
        print("PROBE:", err)
    vm.pump(5)
    last = vm.shot("boot")
    for step in steps:
        kind, _, arg = step.partition(":")
        if kind == "shot":
            now = vm.shot(arg)
            print(arg, now.facts(), "changed", round(now.diff(last), 3))
            last = now
        elif kind == "wait":
            vm.pump(float(arg))
        elif kind == "type":
            vm.type(arg)
        elif kind == "password":
            vm.type(password())
        elif kind == "key":
            vm.key(arg)
        elif kind == "click":
            x, y = map(float, arg.split(","))
            vm.click(x, y)
        elif kind == "move":
            x, y = map(float, arg.split(","))
            vm.move(x, y)
        elif kind == "until":
            print("until", arg, vm.until(arg.encode(), 120))
    print("PROBE: done")
    return 0


def main():
    a = sys.argv[1:]
    if len(a) >= 4 and a[0] in ("gate", "probe"):
        vm = Vm(a[1], a[2], a[3])
        rc = 1
        try:
            if a[0] == "probe":
                rc = probe(vm, a[4:])
                return rc
            import importlib.util
            spec = importlib.util.spec_from_file_location("gate", os.path.join(os.path.dirname(__file__), "vmware_input_gate.py"))
            gate = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(gate)
            rc = gate.run(vm, sys.modules[__name__])
            return rc
        finally:
            vm.end(logs=rc != 0 or os.environ.get("LOGS") == "1")
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
