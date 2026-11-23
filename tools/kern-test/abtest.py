#!/usr/bin/env python3
"""A/B gate in QEMU (called by ab-test.sh): abtest.py <work dir with medium.img, target.img, stick.img>.
   Every check prints "ok    …" or "FAIL  …"; the last line is the verdict (see ab-test.sh)."""
import os, re, select, shutil, struct, subprocess, sys, time, zlib

W = sys.argv[1]
CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"
TARGET, MEDIUM, STICK = f"{W}/target.img", f"{W}/medium.img", f"{W}/stick.img"
VARS = f"{W}/vars.fd"
PROMPT = re.compile(rb"ultar-kern[^\n>]*> ")
START_PAGE = b"[oberflaeche] Startseite"
PROMPT_OR_PAGE = re.compile(rb"ultar-kern[^\n>]*> |\[oberflaeche\] Startseite")
AGENT = "/bin/ab-agent - /usr/bin/ultar-updated /usr/bin/uupdate /etc/ultar/update.conf"
BUNDLE = "/media/vda-0"
PARTS = "/dev/ultar/part"
LOADER_BUDGET_US = 50_000
results, loader_us = [], []
LOG = open(f"{W}/serial.log", "ab")


def check(name, ok, detail=""):
    results.append(ok)
    print(("ok    " if ok else "FAIL  ") + name + ("" if ok or not detail else f"  [{detail}]"), flush=True)
    return ok


class Vm:
    def __init__(self, disks, install=False):
        # fresh firmware variables per start: the firmware then finds the disk's start partition itself
        shutil.copy("/usr/share/OVMF/OVMF_VARS_4M.fd", VARS)
        args = ["qemu-system-x86_64", "-machine", "q35", "-m", "1024", "-smp", os.environ.get("SMP", "2"), "-cpu", "max",
                "-display", "none", "-no-reboot", "-serial", "stdio",
                "-drive", f"if=pflash,format=raw,readonly=on,file={CODE}", "-drive", f"if=pflash,format=raw,file={VARS}"]
        args += ["-netdev", "user,id=n", "-device", "virtio-net-pci,netdev=n"] if install else ["-nic", "none"]
        for i, (f, dev) in enumerate(disks):
            args += ["-drive", f"file={f},if=none,id=d{i},format=raw", "-device", f"{dev},drive=d{i}" + (",serial=ultar" if dev == "nvme" else "")]
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = b""
        self.started = time.time()

    def read_until(self, pat, timeout):
        """pat: bytes, compiled regex, or None (until exit). True when found; 'exit' when QEMU ended."""
        end = time.time() + timeout
        while time.time() < end:
            r, _, _ = select.select([self.p.stdout], [], [], 0.05)
            if r:
                d = os.read(self.p.stdout.fileno(), 65536)
                if not d:
                    self.p.wait()
                    return "exit"
                self.buf += d
                LOG.write(d)
                if pat is not None and (pat.search(self.buf) if hasattr(pat, "search") else pat in self.buf):
                    return True
            elif self.p.poll() is not None:
                return "exit"
        return False

    def cmd(self, line, timeout=60):
        """Runs a console command; returns its output (until the next prompt)."""
        self.buf = b""
        self.p.stdin.write(line.encode() + b"\r")
        self.p.stdin.flush()
        self.read_until(PROMPT, timeout)
        return self.buf.decode("utf-8", "replace")

    def key(self, k, pause=1.2):
        self.p.stdin.write(k.encode())
        self.p.stdin.flush()
        self.read_until(b"\x00never", pause)

    def kill(self):
        if self.p.poll() is None:
            self.p.kill()
        self.p.wait()

    def reboot(self):
        self.p.stdin.write(b"neustart\r")
        self.p.stdin.flush()
        ok = self.read_until(None, 60) == "exit"
        self.kill()
        return ok


def table():
    """Newest valid slot table copy on the target disk (host side), or None."""
    with open(TARGET, "rb") as f:
        f.seek(512)
        h = f.read(512)
        f.seek(struct.unpack_from("<Q", h, 72)[0] * 512)
        ents = f.read(128 * 128)
        first = None
        for i in range(128):
            e = ents[i * 128:(i + 1) * 128]
            if e[56:128].decode("utf-16-le").rstrip("\0") == "ULTAR-BOOTCTL":
                first = struct.unpack_from("<Q", e, 32)[0]
        copies = []
        for off in (0, 32768):
            f.seek(first * 512 + off)
            b = f.read(64)
            ok = b[:5] == b"USLT\x01" and zlib.crc32(b[:60]) & 0xffffffff == struct.unpack_from("<I", b, 60)[0]
            copies.append(dict(default="ab"[b[5]], trial=None if b[6] == 0xFF else ("ab"[b[6]], b[7]),
                               flags=(b[8], b[9]), last="ab"[b[10]] if b[10] < 2 else "-", seq=struct.unpack_from("<I", b, 12)[0]) if ok else None)
    valid = [c for c in copies if c]
    if not valid:
        return None
    if len(valid) == 2:
        a, b = valid
        return b if 0 < (b["seq"] - a["seq"]) % 2**32 < 2**31 else a
    return valid[0]


def boot(expect_slot=None, trial=None, stick=True, timeout=180):
    """Starts the installed disk; returns (vm, slot line) once the console is there (vm None if QEMU ended)."""
    disks = ([(STICK, "virtio-blk-pci")] if stick else []) + [(TARGET, "nvme")]
    vm = Vm(disks)
    got = vm.read_until(PROMPT_OR_PAGE, timeout)
    if got is True and START_PAGE in vm.buf:
        # an installed system may open the start page first: tile 1 is the terminal
        vm.p.stdin.write(b"1")
        vm.p.stdin.flush()
        got = vm.read_until(PROMPT, 60)
    m = re.search(rb"\[boot\] slot ([ab])( \(trial\))?, loader (\d+) us", vm.buf)
    if m:
        loader_us.append(int(m.group(3)))
    if got is not True:
        vm.kill()
        return None, m
    if expect_slot:
        slot = vm.cmd("zeige /proc/ultar/boot_slot")
        check(f"start: slot {expect_slot}{' (trial)' if trial else ''}", m is not None and m.group(1).decode() == expect_slot
              and bool(m.group(2)) == bool(trial) and f"\n{expect_slot}" in slot.replace("\r", ""),
              (m.group(0).decode() if m else "no [boot] line") + " / " + slot.strip()[-40:])
    return vm, m


def agent(vm, *groups, timeout=900):
    line = AGENT + " " + " -- ".join(groups)
    return vm.cmd(line, timeout)


def run():
    # a fresh, empty target disk (a disk with a system on it would make the medium start that system)
    with open(TARGET, "wb") as f:
        f.truncate(512 << 20)
    # --- installation (graphical installer driven over the serial console)
    vm = Vm([(MEDIUM, "virtio-blk-pci"), (TARGET, "nvme")], install=True)
    check("installer started", vm.read_until(b"[installation] gestartet", 180) is True)
    for k, pause in (("2", 1.5), ("4", 5), ("3", 1.5), ("1", 1.5), ("3", 1.5), ("2", 1)):
        vm.key(k, pause)
    done = vm.read_until(b"slot A active", 900)
    check("installation in the A/B layout", done is True and b"A/B layout" in vm.buf, vm.buf[-300:].decode("utf-8", "replace"))
    vm.kill()
    t = table()
    check("factory slot table on ULTAR-BOOTCTL", t is not None and t["default"] == "a" and t["trial"] is None and t["flags"] == (3, 0), str(t))

    # --- slot A, raw access rules, offline update to B
    vm, _ = boot("a")
    if vm is None:
        return check("installed system starts", False)
    out = vm.cmd(f"/bin/ab-agent rawcheck {PARTS}/ultar-system-a {PARTS}/ultar-system-b {PARTS}/ULTAR-BOOTCTL")
    check("raw: running slot not writable (EBUSY)", "running-write refused 16" in out, out[-200:])
    check("raw: other slot writable, fsync = device flush", "other-write ok size" in out and "fsync true" in out, out[-200:])
    check("raw: slot table readable", "AGENT RAW table USLT" in out, out[-200:])
    out = agent(vm, "status", f"offline {BUNDLE}/b.uup", "apply", "status")
    check("slot A runs 0.7.0", "Running version: 0.7.0 (slot a)" in out, out[-400:])
    check("update B applied (offline .uup)", "AGENT EXIT apply 0" in out and "Pending version: 0.8.0" in out, out[-600:])
    t = table()
    check("B is a pending trial with 3 attempts", t and t["trial"] == ("b", 3) and t["flags"][1] == 1, str(t))
    check("restart", vm.reboot())
    vm, _ = boot("b", trial=True)
    if vm is None:
        return check("B starts", False)
    out = agent(vm, "status")
    check("B confirmed after its health check", "Running version: 0.8.0 (slot b)" in out, out[-400:])
    t = table()
    check("B is default and successful, trial over", t and t["default"] == "b" and t["trial"] is None and t["flags"][1] == 3, str(t))
    vm.reboot()
    vm, _ = boot("b")
    if vm is None:
        return check("B starts again", False)

    # --- C: the new kernel panics; 3 attempts, then back to B
    out = agent(vm, f"offline {BUNDLE}/c.uup", "apply")
    check("C applied to slot A", "AGENT EXIT apply 0" in out, out[-400:])
    vm.reboot()
    panics = 0
    for _ in range(3):
        vm = Vm([(STICK, "virtio-blk-pci"), (TARGET, "nvme")])
        r = vm.read_until(PROMPT, 180)
        panics += int(r == "exit" and b"ab-test-panic" in vm.buf and b"trial start of slot a failed" in vm.buf)
        vm.kill()
    check("C: kernel panic restarts the machine on each of its 3 attempts", panics == 3, f"{panics} of 3")
    vm, _ = boot("b")
    if vm is None:
        return check("fallback to B after C", False)
    out = agent(vm, "status", "history 1")
    check("C given up: B runs, history says it went back", "Running version: 0.8.0 (slot b)" in out and "returned to the previous version" in out, out[-500:])

    # --- D: starts, but never confirms (no health report); 3 attempts, then back to B
    out = agent(vm, f"offline {BUNDLE}/d.uup", "apply")
    check("D applied to slot A", "AGENT EXIT apply 0" in out, out[-400:])
    vm.reboot()
    for attempt in range(3):
        vm, _ = boot("a", trial=True)
        if vm is None:
            return check(f"D attempt {attempt + 1} starts", False)
        vm.reboot()
    vm, _ = boot("b")
    if vm is None:
        return check("fallback to B after D", False)
    out = agent(vm, "status", "history 1")
    check("D given up after its attempts: B runs, history says it went back", "Running version: 0.8.0 (slot b)" in out and "returned to the previous version" in out, out[-500:])

    # --- E: power loss while the bundle is staged and while the slot is written (apply)
    # Every attempt stages again from the stick: a staged bundle that the kernel's ext4 writer could not
    # put on the disk (see boot-ab.md, findings) does not survive the cut – the updater then refuses to
    # apply (safe), and the next attempt fetches it again.
    e_running = False
    for marker, delay in ((b"AGENT DAEMON ready", 2.0), (b"Installing the update", 0.3), (b"Installing the update", 1.5),
                          (b"Installing the update", 4.0)):
        vm.buf = b""
        vm.p.stdin.write((AGENT + f" offline {BUNDLE}/e.uup -- apply\r").encode())
        vm.p.stdin.flush()
        seen = vm.read_until(marker, 600) is True
        vm.read_until(b"\x00never", delay)
        vm.kill()
        where = f"{delay:.1f} s after '{marker.decode()}'"
        t = table()
        check(f"power loss {where}: table valid, A not startable or E complete",
              seen and t is not None and (t["flags"][0] == 0 or t["trial"] == ("a", 3)), str(t))
        vm, m = boot()
        check(f"power loss {where}: device starts a valid slot", vm is not None and m is not None and
              (m.group(1) == b"b" or (m.group(1) == b"a" and m.group(2))), m.group(0).decode() if m else "no start")
        if vm is None:
            return
        if m.group(1) == b"a":
            e_running = True  # the cut came after the apply had completed: E is on its trial start
            break
    if not e_running:
        out = agent(vm, f"offline {BUNDLE}/e.uup", "apply")
        check("E applied after the interruptions", "AGENT EXIT apply 0" in out and "Pending version: 0.11.0" in out, out[-400:])
        vm.reboot()
        vm, _ = boot("a", trial=True)
        if vm is None:
            return check("E starts", False)
    out = agent(vm, "status")
    check("E confirmed in slot A", "Running version: 0.11.0 (slot a)" in out, out[-300:])

    # --- F pending in slot B: power loss while the loader writes the slot table
    out = agent(vm, f"offline {BUNDLE}/f.uup", "apply")
    check("F applied to slot B", "AGENT EXIT apply 0" in out, out[-400:])
    vm.reboot()
    for delay in (0.0, 0.003, 0.01, 0.03, 0.1, 0.3):
        vm = Vm([(STICK, "virtio-blk-pci"), (TARGET, "nvme")])
        seen = vm.read_until(re.compile(rb"BdsDxe: starting Boot"), 120) is True
        time.sleep(delay)
        vm.kill()
        t = table()
        check(f"power loss {delay * 1000:.0f} ms after the firmware starts the loader: a valid table "
              f"({'trial ' + str(t['trial']) if t else '-'})", seen and t is not None, str(t))
    t = table()
    vm, m = boot()
    want = "b" if t and t["trial"] and t["trial"][0] == "b" and t["trial"][1] > 0 else (t["default"] if t else "?")
    check("after the loader cuts the device starts the slot the table names", vm is not None and m is not None and
          m.group(1).decode() == want, f"{m.group(0).decode() if m else '-'} {t}")
    if vm is None:
        return
    out = agent(vm, "status")
    if want == "b":
        check("F confirmed in slot B", "Running version: 0.12.0 (slot b)" in out, out[-300:])
    else:
        check("F given up (the cuts spent its attempts), E runs", "Running version: 0.11.0 (slot a)" in out, out[-300:])
    vm.reboot()
    # QEMU without KVM on a shared host: single starts vary a lot; the median is the loader's cost
    times = sorted(loader_us)
    median = times[len(times) // 2] if times else None
    check(f"loader adds < {LOADER_BUDGET_US // 1000} ms (median {median} us, min {times[0] if times else '-'}, "
          f"max {times[-1] if times else '-'} us over {len(times)} starts, QEMU TCG)",
          median is not None and median < LOADER_BUDGET_US)


if __name__ == "__main__":
    try:
        run()
    finally:
        LOG.close()
    failed = results.count(False)
    if failed == 0 and results:
        print(f"AB-TEST: ALLES OK ({len(results)} checks)")
        sys.exit(0)
    print(f"AB-TEST: FEHLER ({failed} of {len(results)} checks failed)")
    sys.exit(1)
