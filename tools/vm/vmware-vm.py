#!/usr/bin/env python3
"""QEMU on VMware-like hardware (q35, OVMF, AHCI/NVMe, e1000e, xHCI, no virtio) for tools/vm/make-vmware.sh.

   vmware-vm.py install MEDIUM.img TARGET.raw ar|de|en LOG
       starts the installation medium (AHCI) with the empty target (NVMe) and drives the kernel's graphical
       installer over the serial console; exit 0 when it reported the A/B layout with slot A active.
   vmware-vm.py check DISK raw|vmdk nvme|ahci PREFIX
       starts the disk (on a copy-on-write layer: the file is not changed), waits until the service manager
       reports every service of the image as started and every system app as installed, takes a screenshot
       through the QEMU monitor (PREFIX.ppm, PREFIX.png when Pillow is there; serial log PREFIX.log) and prints
       one verdict line: "VMWARE-CHECK: OK …" when the slot, the services, the apps and the locked session
       (lock screen, no desktop bar) are there. Input is driven by tools/vm/vmware-input.py.
   vmware-vm.py zip FOLDER OUT.zip
       packs the folder (with its name as the top entry) without needing the zip program.
   Environment: SMP (4, as the .vmx), MEM (1024), WAIT (seconds per step, 600), VGA (std | vmware: the VMware
   SVGA II adapter, for which the kernel has no driver and keeps the firmware's frame buffer),
   ACCEL (tcg | kvm; kvm uses the processor of the build computer)."""
import os, re, select, shutil, socket, subprocess, sys, tempfile, time, zipfile

CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"
VARS = "/usr/share/OVMF/OVMF_VARS_4M.fd"
WAIT = float(os.environ.get("WAIT", "600"))
LANG_KEY = {"ar": "1", "de": "2", "en": "3"}
SERVICES = ["logd", "bus", "settingsd", "devd", "seat", "authd", "compositor", "lock", "sysdialog", "desktop",
            "launcher", "sessiond", "powerd", "notifyd", "calendard", "audiod", "mediad", "captured", "netd", "btd",
            "volumed", "displayd", "accountd"]
# one-shot services: the system apps are installed with the launcher (each must end with code 0)
APPS = ["settings", "files", "store", "phone", "browser"]
# the session on the screen: it starts locked, so the lock screen (ground colour over the whole screen) with
# the brand mark (accent_text) and no desktop bar (token bar, dark theme)
WINDOWS = {"bar": bytes.fromhex("12151B"), "mark": bytes.fromhex("E3C55A"), "ground": bytes.fromhex("0B0D11")}


class Vm:
    """One QEMU run; the serial console is its standard input and output."""

    def __init__(self, disks, log):
        self.dir = tempfile.mkdtemp(prefix="ultar-vmware-")
        self.monitor_path = os.path.join(self.dir, "monitor")
        shutil.copy(VARS, os.path.join(self.dir, "vars.fd"))
        kvm = os.environ.get("ACCEL", "tcg") == "kvm"
        args = ["qemu-system-x86_64", "-machine", "q35", "-accel", "kvm" if kvm else "tcg", "-cpu", "host" if kvm else "max",
                "-m", os.environ.get("MEM", "1024"), "-smp", os.environ.get("SMP", "4"),
                "-display", "none", "-no-reboot", "-vga", os.environ.get("VGA", "std"),
                "-drive", f"if=pflash,format=raw,readonly=on,file={CODE}",
                "-drive", f"if=pflash,format=raw,file={self.dir}/vars.fd",
                "-netdev", "user,id=n", "-device", "e1000e,netdev=n",
                "-device", "qemu-xhci,id=xh", "-device", "usb-kbd,bus=xh.0", "-device", "usb-tablet,bus=xh.0",
                "-monitor", f"unix:{self.monitor_path},server,nowait", "-serial", "stdio"]
        for i, (path, fmt, controller, snapshot) in enumerate(disks):
            args += ["-drive", f"file={path},if=none,id=d{i},format={fmt}" + (",snapshot=on" if snapshot else "")]
            args += ["-device", f"nvme,serial=ultar{i},drive=d{i}" if controller == "nvme" else f"ide-hd,bus=ide.{i},drive=d{i}"]
        env = dict(os.environ, TMPDIR=self.dir)  # the copy-on-write layer lives and dies with this run
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
        self.buf = b""
        self.log = open(log, "wb")

    def read_until(self, pattern, timeout):
        """True when the compiled pattern is in the output, False on timeout, None when QEMU ended."""
        end = time.time() + timeout
        while time.time() < end:
            ready, _, _ = select.select([self.p.stdout], [], [], 0.2)
            if ready:
                data = os.read(self.p.stdout.fileno(), 65536)
                if not data:
                    return None
                self.buf += data
                self.log.write(data)
                self.log.flush()
            if pattern is not None and pattern.search(self.buf):
                return True
            if not ready and self.p.poll() is not None:
                return None
        return False

    def key(self, key, pause):
        self.p.stdin.write(key.encode())
        self.p.stdin.flush()
        self.read_until(None, pause)

    def monitor(self, command):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(self.monitor_path)
        s.settimeout(10)
        s.recv(4096)  # greeting
        s.sendall(command.encode() + b"\n")
        time.sleep(2.0)
        s.close()

    def end(self):
        if self.p.poll() is None:
            self.p.kill()
        self.p.wait()
        self.log.close()
        shutil.rmtree(self.dir, ignore_errors=True)


def install(medium, target, lang, log):
    vm = Vm([(medium, "raw", "ahci", False), (target, "raw", "nvme", False)], log)
    try:
        if vm.read_until(re.compile(rb"\[installation\] gestartet"), WAIT) is not True:
            print("VMWARE-INSTALL: FEHLER (the installer did not start)")
            return 1
        # language, next · devices: next · target: the only disk, next · confirm: erase and install
        for key, pause in ((LANG_KEY[lang], 1.5), ("4", 5), ("3", 1.5), ("1", 1.5), ("3", 1.5), ("2", 1)):
            vm.key(key, pause)
        done = vm.read_until(re.compile(rb"\[installation\] fertig: A/B layout, \d+ partitions, slot A active"), 6 * WAIT)
        virtio = re.search(rb"\[treiber\] .* -> virtio", vm.buf) is not None
        if done is not True or virtio:
            print("VMWARE-INSTALL: FEHLER (" + ("a virtio device is present" if virtio else "no A/B installation reported") + ")")
            return 1
        # let the last writes reach the disk file before QEMU is ended
        vm.read_until(None, 3)
        print("VMWARE-INSTALL: OK (A/B layout, slot A active, medium on AHCI, target on NVMe)")
        return 0
    finally:
        vm.end()


def windows_on(path):
    """How many pixels of each window colour a binary PPM (P6) screenshot holds."""
    data = open(path, "rb").read()
    head = data.split(b"\n", 3)
    width, height = map(int, head[1].split())
    pixels = head[3]
    counts = {name: 0 for name in WINDOWS}
    for name, colour in WINDOWS.items():
        at = pixels.find(colour)
        while at != -1:
            if at % 3 == 0:
                counts[name] += 1
            at = pixels.find(colour, at + 1)
    return width, height, counts


def lock_screen(width, height, counts):
    """The lock screen covers everything: ground nearly everywhere, the mark, no desktop bar."""
    return counts.get("ground", 0) >= 0.6 * width * height and counts.get("mark", 0) >= 10 \
        and counts.get("bar", 0) < 0.3 * width * 56


def check(disk, fmt, controller, prefix):
    vm = Vm([(disk, fmt, controller, True)], prefix + ".log")
    problems = []
    try:
        # the service manager's journal on the console: "ready NAME" for a service that reports readiness,
        # "start NAME" for the others, "settled" when nothing is left to start
        def line(service, word=b"start"):
            return re.compile(rb"\[ultar-init[^\]]*\] " + word + b" " + re.escape(service.encode()) + rb"( |$)", re.M)
        settled = re.compile(rb"\[ultar-init[^\]]*\] settled")
        if vm.read_until(settled, WAIT) is None:
            problems.append("QEMU ended")
        missing = [s for s in SERVICES if not line(s).search(vm.buf)]
        if missing:
            problems.append("services not running: " + " ".join(missing))
        # the app installations end (with code 0) after "settled" was reported: wait for them
        installed = [re.compile(rb"\[ultar-init[^\]]*\] exit app-" + a.encode() + rb" pid=\d+ code=0 ") for a in APPS]
        end = time.time() + WAIT
        while time.time() < end and not all(r.search(vm.buf) for r in installed):
            if vm.read_until(None, 2) is None:
                break
        not_installed = [a for a, r in zip(APPS, installed) if not r.search(vm.buf)]
        if not_installed:
            problems.append("apps not installed: " + " ".join(not_installed))
        ended = [m.group(0).decode(errors="replace") for m in re.finditer(rb"\[ultar-init[^\]]*\] (refused|exit|failed) [^\r\n]*", vm.buf)
                 if not re.search(rb"\] exit app-[a-z]+ pid=\d+ code=0 ", m.group(0))]
        if ended:
            problems.append("a service was refused or ended: " + "; ".join(ended[:3]))
        slot = re.search(rb"\[boot\] slot ([ab])", vm.buf)
        if not slot:
            problems.append("no [boot] slot line (the loader did not start the kernel from its slot)")
        if re.search(rb"kernel panic|PANIK", vm.buf, re.I):
            problems.append("kernel panic")
        if re.search(rb"\[treiber\] .* -> virtio", vm.buf):
            problems.append("a virtio device is present")
        width = height = 0
        counts = {}
        if vm.p.poll() is None:
            # give the session time for its first frame, then look at the screen
            shot_end = time.time() + WAIT
            while time.time() < shot_end:
                vm.read_until(None, 5)
                vm.monitor(f"screendump {prefix}.ppm")
                if os.path.exists(prefix + ".ppm"):
                    width, height, counts = windows_on(prefix + ".ppm")
                    if lock_screen(width, height, counts):
                        break
            if not counts or not lock_screen(width, height, counts):
                problems.append(f"the session's lock screen is not on the screen: {counts}")
            try:
                from PIL import Image
                Image.open(prefix + ".ppm").save(prefix + ".png")
            except Exception:  # Pillow is optional: the PPM stays
                pass
    finally:
        vm.end()
    detail = f"{fmt} on {controller}, display {os.environ.get('VGA', 'std')}, slot {slot.group(1).decode() if slot else '?'}, screen {width}x{height}, window pixels {counts}"
    if problems:
        print(f"VMWARE-CHECK: FEHLER ({'; '.join(problems)}) [{detail}]")
        return 1
    print(f"VMWARE-CHECK: OK ({len(SERVICES)} services, {len(APPS)} apps installed, locked session; {detail})")
    return 0


def pack(folder, out):
    folder = os.path.normpath(folder)
    top = os.path.basename(folder)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for name in sorted(os.listdir(folder)):
            z.write(os.path.join(folder, name), f"{top}/{name}")
    return 0


def main():
    a = sys.argv[1:]
    if len(a) == 3 and a[0] == "zip" and os.path.isdir(a[1]):
        return pack(a[1], a[2])
    if len(a) == 5 and a[0] == "install" and a[3] in LANG_KEY:
        return install(a[1], a[2], a[3], a[4])
    if len(a) == 5 and a[0] == "check" and a[2] in ("raw", "vmdk") and a[3] in ("nvme", "ahci"):
        return check(a[1], a[2], a[3], a[4])
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
