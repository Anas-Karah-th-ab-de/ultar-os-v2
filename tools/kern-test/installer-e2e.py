#!/usr/bin/env python3
"""The graphical installation from A to Z on VMware-like hardware (q35, OVMF, AHCI + NVMe, e1000e, xHCI with
USB tablet, PS/2 keyboard, no virtio):

   installer-e2e.py MEDIUM.img TARGET.raw LOGPREFIX

1. The installation medium (AHCI) starts with the empty target (NVMe). The Ultar kernel starts the installer
   session (ultar-init, profile installer: logd, bus, devd, seat, compositor, ultar-installer-ui). The installer
   reports step, focus and the Next button on the console (--report); this driver presses keys through the QEMU
   monitor (sendkey: the PS/2 keyboard -> the kernel's event node -> the compositor -> the installer) and follows
   those lines: Tab until Next has the focus, Enter; on the account step it types the full name, the user name
   (Latin layout: Super+Space), the password twice; on the summary it ticks the erase line (Space) and presses
   Install. Then it waits for "installation written".
2. The target alone (NVMe) starts: loader (A/B) -> kernel -> ultar-init (profile system) -> ultar-firstboot turns
   the installer's answers into the first account; the image's check service lists the accounts.
Verdict line: "INSTALLER-E2E: OK …" (exit 0) or "INSTALLER-E2E: FEHLER (…)".
Environment: SMP (2), MEM (1024), WAIT (seconds per phase, 900), USER_NAME (anas)."""
import os, re, select, shutil, socket, subprocess, sys, tempfile, time

CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"
VARS = "/usr/share/OVMF/OVMF_VARS_4M.fd"
WAIT = float(os.environ.get("WAIT", "900"))
USER = os.environ.get("USER_NAME", "anas")
FULL = "Anas Kara"
PASSWORD = "km-Secret-2026"
# The kernel polls the USB keyboard (no interrupts): a key is held long enough to be seen down and up.
HOLD_MS = int(os.environ.get("HOLD_MS", "300"))
# Pause after each typed character: under TCG one frame of the installer takes long, and the kernel polls the
# keyboard (no interrupts) - characters typed faster than the frames are lost (open point, installer-boot.md).
TYPE_GAP = float(os.environ.get("TYPE_GAP", "2.5"))
REPORT = re.compile(rb"\[installer-ui\] step=(\w+) focus=(\S+) next=(\S+)")


class Vm:
    def __init__(self, disks, log):
        self.dir = tempfile.mkdtemp(prefix="ultar-inst-e2e-")
        self.mon = os.path.join(self.dir, "monitor")
        shutil.copy(VARS, os.path.join(self.dir, "vars.fd"))
        args = ["qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-cpu", "max",
                "-m", os.environ.get("MEM", "1024"), "-smp", os.environ.get("SMP", "2"),
                "-display", "none", "-no-reboot", "-vga", "std",
                "-drive", f"if=pflash,format=raw,readonly=on,file={CODE}",
                "-drive", f"if=pflash,format=raw,file={self.dir}/vars.fd",
                "-netdev", "user,id=n", "-device", "e1000e,netdev=n",
                # keys through the i8042 (PS/2) keyboard, which a VMware machine has too; the USB keyboard path
                # loses keys typed through the monitor (open point in docs/integration/installer-boot.md)
                "-device", "qemu-xhci,id=xh", "-device", "usb-tablet,bus=xh.0",
                "-monitor", f"unix:{self.mon},server,nowait", "-serial", "stdio"]
        for i, (path, controller) in enumerate(disks):
            args += ["-drive", f"file={path},if=none,id=d{i},format=raw"]
            args += ["-device", f"nvme,serial=ultar{i},drive=d{i}" if controller == "nvme" else f"ide-hd,bus=ide.{i},drive=d{i}"]
        self.p = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = b""
        self.log = open(log, "wb")
        self.sock = None

    def pump(self, timeout):
        """Reads output for at most `timeout` seconds; False when QEMU ended."""
        ready, _, _ = select.select([self.p.stdout], [], [], timeout)
        if ready:
            data = os.read(self.p.stdout.fileno(), 65536)
            if not data:
                return False
            self.buf += data
            self.log.write(data)
            self.log.flush()
        return self.p.poll() is None or bool(ready)

    def wait_for(self, pattern, timeout, start=0):
        end = time.time() + timeout
        while time.time() < end:
            m = pattern.search(self.buf, start)
            if m:
                return m
            if not self.pump(0.2):
                return pattern.search(self.buf, start)
        return None

    def monitor(self, command):
        if self.sock is None:
            for _ in range(50):
                try:
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    s.connect(self.mon)
                    break
                except OSError:
                    time.sleep(0.2)
            s.settimeout(5)
            self.sock = s
            time.sleep(0.3)
            try:
                s.recv(65536)
            except OSError:
                pass
        self.sock.sendall(command.encode() + b"\n")
        time.sleep(0.15)
        try:
            self.sock.recv(65536)
        except OSError:
            pass

    def end(self):
        if self.sock:
            self.sock.close()
        if self.p.poll() is None:
            self.p.kill()
        self.p.wait()
        self.log.close()
        shutil.rmtree(self.dir, ignore_errors=True)


def last_report(vm):
    found = list(REPORT.finditer(vm.buf))
    if not found:
        return None
    m = found[-1]
    return m.group(1).decode(), m.group(2).decode(), m.group(3).decode(), len(found)


KEYS = {" ": "spc", "-": "minus", ".": "dot", "_": "shift-minus"}


def keyname(c):
    if c.isdigit() or ("a" <= c <= "z"):
        return c
    if "A" <= c <= "Z":
        return "shift-" + c.lower()
    return KEYS[c]


class Driver:
    def __init__(self, vm):
        self.vm = vm

    def press(self, key, settle=1.5):
        """Sends a key and waits until the installer reported something new (or `settle` passed)."""
        before = len(REPORT.findall(self.vm.buf))
        self.vm.monitor(f"sendkey {key} {HOLD_MS}")
        end = time.time() + settle
        while time.time() < end:
            if len(REPORT.findall(self.vm.buf)) > before:
                # a burst of reports can follow one key: let it finish
                self.vm.pump(0.3)
                return
            self.vm.pump(0.1)

    def focus(self, want, guard=120):
        for _ in range(guard):
            r = last_report(self.vm)
            if r and r[1] == want:
                return True
            self.press("tab")
        r = last_report(self.vm)
        return bool(r and r[1] == want)

    def type(self, text):
        for c in text:
            self.vm.monitor(f"sendkey {keyname(c)} {HOLD_MS}")
            self.vm.pump(TYPE_GAP)
        self.vm.pump(1.0)


def install(medium, target, prefix):
    vm = Vm([(medium, "ahci"), (target, "nvme")], prefix + "-install.log")
    d = Driver(vm)
    try:
        if not vm.wait_for(REPORT, WAIT):
            return "the installer did not report (session not started)"
        vm.pump(3)
        latin = False
        for _ in range(60):
            r = last_report(vm)
            step, focus, nxt, _ = r
            if step == "Account" and nxt.startswith("off"):
                if not latin:
                    vm.monitor(f"sendkey meta_l-spc {HOLD_MS}")  # Super+Space: the Latin layout for the names
                    vm.pump(TYPE_GAP)
                    latin = True
                for field, text in (("AccountField(0)", FULL), ("AccountField(1)", USER), ("Password(0)", PASSWORD), ("Password(1)", PASSWORD)):
                    if not d.focus(field):
                        return f"Tab does not reach {field}"
                    d.type(text)
                d.press("tab")
                if last_report(vm)[2] != "on":
                    vm.monitor(f"screendump {prefix}-account.ppm")
                    vm.pump(2)
                    return "the account step stays blocked after typing name and password: " + last_report(vm)[2]
                continue
            if step == "Summary" and nxt.startswith("off"):
                if not d.focus("Confirm"):
                    return "Tab does not reach the erase confirmation"
                d.press("spc")
                if last_report(vm)[2] != "on":
                    return "the summary stays blocked after the confirmation"
                continue
            if step == "Install":
                break
            if nxt != "on":
                return f"Next is disabled on step {step}"
            if not d.focus("Next"):
                return f"Tab does not reach Next on step {step}"
            d.press("ret", settle=5)
            if step == "Summary":
                break
        done = vm.wait_for(re.compile(rb"\[installer-ui\] installation (written|failed[^\n]*)"), WAIT)
        if not done:
            return "the installation did not end"
        if done.group(1) != b"written":
            return done.group(1).decode(errors="replace").strip()
        vm.pump(3)  # the last writes reach the disk file
        if re.search(rb"\[treiber\] .* -> virtio", vm.buf):
            return "a virtio device is present"
        return None
    finally:
        vm.end()


def first_start(target, prefix):
    vm = Vm([(target, "nvme")], prefix + "-first.log")
    try:
        created = vm.wait_for(re.compile(rb"\[firstboot\] account (\S+) created"), WAIT)
        listed = vm.wait_for(re.compile(rb"\[firstboot\] account " + re.escape(USER.encode()) + rb"\r?\n"), WAIT)
        slot = re.search(rb"\[boot\] slot ([ab])", vm.buf)
        problems = []
        if not created or created.group(1).decode() != USER:
            problems.append("no account created at the first start")
        if not listed:
            problems.append(f"the account {USER} is not in the account file")
        if not slot:
            problems.append("not started by the A/B loader from a slot")
        if re.search(rb"kernel panic|PANIK", vm.buf, re.I):
            problems.append("kernel panic")
        return "; ".join(problems) or None
    finally:
        vm.end()


def main():
    medium, target, prefix = sys.argv[1:4]
    t0 = time.time()
    err = install(medium, target, prefix)
    if err:
        print(f"INSTALLER-E2E: FEHLER (installation: {err}; log {prefix}-install.log)")
        return 1
    t1 = time.time()
    err = first_start(target, prefix)
    if err:
        print(f"INSTALLER-E2E: FEHLER (first start of the installed disk: {err}; log {prefix}-first.log)")
        return 1
    print(f"INSTALLER-E2E: OK (installed through the graphical installer in {t1 - t0:.0f} s, "
          f"first start from the installed disk alone with the account {USER}, {time.time() - t1:.0f} s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
