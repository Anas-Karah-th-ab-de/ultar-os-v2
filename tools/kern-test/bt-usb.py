#!/usr/bin/env python3
"""A USB Bluetooth controller for QEMU, behind `-device usb-redir` (QEMU 8.2 has no `usb-bt-dongle`).

    bt-usb.py <unix socket path> <log file>

Listens on the socket; QEMU connects with `-chardev socket,id=bt,path=<socket> -device usb-redir,chardev=bt,...`.
Speaks the usbredir protocol (usbredirproto.h, version 0.13; 64-bit ids and 32-bit bulk lengths, which QEMU needs
behind xHCI) as the device
side and is a full-speed Bluetooth primary controller (class E0/01/01, Bluetooth Core Vol 4 Part B):
interface 0 with interrupt IN 0x81 (events, 16-octet packets), bulk IN 0x82 and bulk OUT 0x02 (ACL, 64 octets),
interface 1 with two isochronous endpoints (SCO, unused).

The controller: every HCI command is answered with Command Complete, status 0 (Read BD_ADDR: C0:FF:EE:00:B7:01,
Read Buffer Size: 1021 octets x 8, LE Read Buffer Size: 251 x 8); vendor command 0xFC77 first sends a vendor event
with 255 parameter octets (0..254), to cut an event over 17 interrupt packets. ACL data is sent back unchanged
(loopback). Every command is written to the log as `BTEMU cmd 0xNNNN`, every ACL packet as `BTEMU acl <len>`.

The air (for a scan): Inquiry is answered with Command Status, an Inquiry Result with RSSI of a nameless phone
(11:22:33:44:55:A1, asked for its name "Telefon A" by Remote Name Request), an Extended Inquiry Result of a keyboard
"BT-Tastatur" twice (RSSI -60, then -48) and Inquiry Complete. LE Set Scan Enable (on) brings advertising reports:
the mouse "BT-Maus" (ADV_IND, complete name, RSSI -70 then -62; a 32-octet event, two full interrupt packets and
no zero-length packet), a nameless pen (ADV_IND, a 16-octet event) whose scan response names it "Stift" (only to
an active scan) and a non-connectable beacon (not to be listed). Logged: `BTEMU scan active|passive`.
"""
import os, socket, struct, sys, threading

SOCK, LOG = sys.argv[1], sys.argv[2]
ADDR = bytes([0x01, 0xB7, 0x00, 0xEE, 0xFF, 0xC0])  # C0:FF:EE:00:B7:01, little endian
EV_MPS = 16
PHONE = bytes([0xA1, 0x55, 0x44, 0x33, 0x22, 0x11])     # 11:22:33:44:55:A1
KEYBOARD = bytes([0xB2, 0x55, 0x44, 0x33, 0x22, 0x11])  # 11:22:33:44:55:B2
MOUSE = bytes([0xC3, 0x00, 0x00, 0xAA, 0xBB, 0xC0])     # C0:BB:AA:00:00:C3/r
PEN = bytes([0xD4, 0x00, 0x00, 0xAA, 0xBB, 0xC0])       # C0:BB:AA:00:00:D4/r
BEACON = bytes([0xE5, 0x00, 0x00, 0xAA, 0xBB, 0xC0])

# message types (usbredirproto.h)
HELLO, CONNECT, DISCONNECT, RESET, IFACE_INFO, EP_INFO = 0, 1, 2, 3, 4, 5
SET_CONFIG, GET_CONFIG, CONFIG_STATUS, SET_ALT, GET_ALT, ALT_STATUS = 6, 7, 8, 9, 10, 11
START_ISO, STOP_ISO, ISO_STATUS, START_INT, STOP_INT, INT_STATUS = 12, 13, 14, 15, 16, 17
CANCEL = 21
CONTROL, BULK, ISO, INTERRUPT = 100, 101, 102, 103
CAP_CONNECT_VERSION, CAP_EP_MPS, CAP_64BIT_IDS, CAP_32BIT_BULK = 1, 4, 5, 6

DEVICE = bytes([18, 1, 0x00, 0x02, 0xE0, 0x01, 0x01, 64, 0x12, 0x0A, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 1])
IFACE0 = bytes([9, 4, 0, 0, 3, 0xE0, 0x01, 0x01, 0])
EPS0 = bytes([7, 5, 0x81, 3, EV_MPS, 0, 1, 7, 5, 0x82, 2, 64, 0, 0, 7, 5, 0x02, 2, 64, 0, 0])
IFACE1 = bytes([9, 4, 1, 0, 2, 0xE0, 0x01, 0x01, 0])
EPS1 = bytes([7, 5, 0x83, 1, 0, 0, 1, 7, 5, 0x03, 1, 0, 0, 1])
BODY = IFACE0 + EPS0 + IFACE1 + EPS1
CONFIG = bytes([9, 2]) + struct.pack("<H", 9 + len(BODY)) + bytes([2, 1, 0, 0x80, 50]) + BODY

HDR = 16

def bulk_hdr(ep, status, length):
    return struct.pack("<BBHIH", ep, status, length & 0xffff, 0, length >> 16)

log_file = open(LOG, "a", buffering=1)
def log(*a):
    log_file.write(" ".join(str(x) for x in a) + "\n")

class Dev:
    def __init__(self, conn):
        self.c = conn
        self.lock = threading.Lock()
        self.bulk_in = []      # pending bulk IN requests: ids
        self.acl_out = []      # ACL packets waiting for a bulk IN request
        self.int_on = False
        self.int_q = []        # interrupt packets waiting for start_interrupt_receiving
        self.active = False    # the LE scan asked for scan responses

    def send(self, typ, ident, hdr=b"", data=b""):
        body = hdr + data
        with self.lock:
            # the hello goes out before the peer's capabilities are known: 32-bit id there
            head = struct.pack("<III", typ, len(body), ident) if typ == HELLO else struct.pack("<IIQ", typ, len(body), ident)
            self.c.sendall(head + body)

    def hello(self):
        caps = (1 << CAP_CONNECT_VERSION) | (1 << CAP_EP_MPS) | (1 << CAP_64BIT_IDS) | (1 << CAP_32BIT_BULK)
        self.send(HELLO, 0, b"ultar bt-usb 1".ljust(64, b"\0") + struct.pack("<I", caps))

    def describe(self):
        ifs = struct.pack("<I", 2) + bytes([0, 1] + [0] * 30) + bytes([0xE0] * 2 + [0] * 30) \
            + bytes([1] * 2 + [0] * 30) + bytes([1] * 2 + [0] * 30)
        self.send(IFACE_INFO, 0, ifs)
        typ, interval, iface, mps = [255] * 32, [0] * 32, [0] * 32, [0] * 32
        for ep, t, iv, i, m in [(0x00, 0, 0, 0, 64), (0x80, 0, 0, 0, 64), (0x81, 3, 1, 0, EV_MPS), (0x82, 2, 0, 0, 64),
                                (0x02, 2, 0, 0, 64), (0x83, 1, 1, 1, 0), (0x03, 1, 1, 1, 0)]:
            k = (ep & 0x0f) | ((ep & 0x80) >> 3)
            typ[k], interval[k], iface[k], mps[k] = t, iv, i, m
        self.send(EP_INFO, 0, bytes(typ) + bytes(interval) + bytes(iface) + struct.pack("<32H", *mps))

    def connect(self):
        self.send(CONNECT, 0, struct.pack("<BBBBHHH", 1, 0xE0, 0x01, 0x01, 0x0A12, 0x0001, 0x0100))

    # ---------------------------------------------------------------- the controller
    def event(self, code, params):
        pkt = bytes([code, len(params)]) + params
        for i in range(0, len(pkt), EV_MPS):
            self.interrupt(pkt[i:i + EV_MPS])

    def interrupt(self, data):
        if not self.int_on:
            self.int_q.append(data)
            return
        self.send(INTERRUPT, 0, struct.pack("<BBH", 0x81, 0, len(data)), data)

    def command(self, pkt):
        if len(pkt) < 3 or len(pkt) != 3 + pkt[2]:
            log("BTEMU bad command", pkt.hex())
            return
        op = pkt[0] | pkt[1] << 8
        log("BTEMU cmd 0x%04x" % op)
        ret = {0x1009: ADDR, 0x1005: struct.pack("<HBHH", 1021, 0x40, 8, 0), 0x2002: struct.pack("<HB", 251, 8),
               0x1001: bytes([12, 0, 0, 12, 0x0a, 0, 0, 0]), 0x1003: bytes([0xff] * 8)}.get(op, b"")
        if op == 0xFC77:
            self.event(0xFF, bytes(range(255)))
        if op in (0x0401, 0x0419):  # Inquiry, Remote Name Request: Command Status, results later
            self.event(0x0F, bytes([0, 1, pkt[0], pkt[1]]))
            if op == 0x0401:
                self.inquiry()
            elif bytes(pkt[3:9]) == PHONE:
                self.event(0x07, (bytes([0]) + PHONE + b"Telefon A").ljust(255, b"\0"))
            else:
                self.event(0x07, (bytes([0x04]) + bytes(pkt[3:9])).ljust(255, b"\0"))
            return
        if op == 0x200B and len(pkt) > 3:
            self.active = pkt[3] == 1
            log("BTEMU scan", "active" if self.active else "passive")
        self.event(0x0E, bytes([1, pkt[0], pkt[1], 0]) + ret)
        if op == 0x200C and len(pkt) > 3 and pkt[3] == 1:
            self.advertise()

    def inquiry(self):
        # Inquiry Result with RSSI: one response (address, R1, reserved, class phone, clock offset, RSSI -72)
        self.event(0x22, bytes([1]) + PHONE + bytes([1, 0, 0x0C, 0x02, 0x5A, 0x34, 0x12, 0xB8]))
        for rssi in (-60, -48):
            name = b"BT-Tastatur"
            eir = KEYBOARD + bytes([1, 0, 0x40, 0x25, 0x00, 0x34, 0x12, rssi & 0xff, len(name) + 1, 0x09]) + name
            self.event(0x2F, (bytes([1]) + eir).ljust(255, b"\0"))
        self.event(0x01, bytes([0]))

    def adv(self, kind, addr, data, rssi):
        self.event(0x3E, bytes([0x02, 1, kind, 1]) + addr + bytes([len(data)]) + data + bytes([rssi & 0xff]))

    def advertise(self):
        # flags, complete name, appearance mouse, an empty manufacturer record: 18 octets, a 32-octet event
        mouse = bytes([2, 0x01, 0x06, 8, 0x09]) + b"BT-Maus" + bytes([3, 0x19, 0xC2, 0x03, 1, 0xFF])
        for rssi in (-70, -62):
            self.adv(0, MOUSE, mouse, rssi)
        self.adv(0, PEN, bytes([1, 0x01]), -75)  # an empty flags record: a 16-octet event
        if self.active:
            self.adv(4, PEN, bytes([6, 0x09]) + b"Stift", -74)
        self.adv(3, BEACON, bytes([2, 0x01, 0x06, 7, 0x09]) + b"Beacon", -40)

    def acl(self, pkt):
        log("BTEMU acl", len(pkt))
        self.acl_out.append(pkt)
        self.flush_bulk()

    def flush_bulk(self):
        while self.bulk_in and self.acl_out:
            ident, want = self.bulk_in.pop(0)
            data = self.acl_out.pop(0)
            if len(data) > want:  # does not happen with this kernel (1024-octet requests)
                self.acl_out.insert(0, data[want:])
                data = data[:want]
            self.send(BULK, ident, bulk_hdr(0x82, 0, len(data)), data)

    # ---------------------------------------------------------------- usbredir messages
    def control(self, ident, hdr, data):
        ep, req, rtype, _, value, index, length = struct.unpack("<BBBBHHH", hdr)
        reply = lambda st, d=b"": self.send(CONTROL, ident, struct.pack("<BBBBHHH", ep, req, rtype, st, value, index, len(d) if rtype & 0x80 else length), d)
        if rtype == 0x80 and req == 6:
            d = {1: DEVICE, 2: CONFIG}.get(value >> 8)
            return reply(0, d[:length]) if d else reply(4)
        if rtype == 0x80 and req == 0:
            return reply(0, b"\0\0"[:length])
        if rtype == 0x20 and req == 0:
            self.command(data)
            return reply(0)
        log("BTEMU control stalled", hex(rtype), req, value, index, length)
        reply(4)

    def handle(self, typ, ident, body):
        if typ == HELLO:
            self.describe()
            self.connect()
        elif typ == CONTROL:
            self.control(ident, body[:10], body[10:])
        elif typ == SET_CONFIG:
            self.send(CONFIG_STATUS, ident, bytes([0, body[0]]))
            self.describe()
        elif typ == GET_CONFIG:
            self.send(CONFIG_STATUS, ident, bytes([0, 1]))
        elif typ == SET_ALT:
            self.send(ALT_STATUS, ident, bytes([0, body[0], body[1]]))
        elif typ == GET_ALT:
            self.send(ALT_STATUS, ident, bytes([0, body[0], 0]))
        elif typ == START_INT:
            self.send(INT_STATUS, ident, bytes([0, body[0]]))
            self.int_on = True
            for d in self.int_q:
                self.interrupt(d)
            self.int_q = []
        elif typ == STOP_INT:
            self.int_on = False
            self.send(INT_STATUS, ident, bytes([0, body[0]]))
        elif typ == BULK:
            ep, _, low, _, high = struct.unpack("<BBHIH", body[:10])
            length = low | high << 16
            if ep == 0x82:
                self.bulk_in.append((ident, length))
                self.flush_bulk()
            else:
                self.send(BULK, ident, bulk_hdr(ep, 0, length))
                self.acl(body[10:10 + length])
        elif typ == CANCEL:
            for i, (pid, _) in enumerate(self.bulk_in):
                if pid == ident:
                    self.bulk_in.pop(i)
                    self.send(BULK, ident, bulk_hdr(0x82, 1, 0))
                    break
        elif typ in (RESET, START_ISO, STOP_ISO, ISO):
            pass
        else:
            log("BTEMU unknown message", typ)

    def run(self):
        self.hello()
        buf = b""
        while True:
            d = self.c.recv(65536)
            if not d:
                return
            buf += d
            while len(buf) >= HDR:
                typ, length = struct.unpack("<II", buf[:8])
                # QEMU's hello comes before the capabilities are known: 32-bit id there
                hdr = 12 if typ == HELLO else HDR
                ident = struct.unpack("<I" if hdr == 12 else "<Q", buf[8:hdr])[0]
                if len(buf) < hdr + length:
                    break
                body, buf = buf[hdr:hdr + length], buf[hdr + length:]
                self.handle(typ, ident, body)

if os.path.exists(SOCK):
    os.unlink(SOCK)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(SOCK)
srv.listen(1)
log("BTEMU listening")
conn, _ = srv.accept()
log("BTEMU QEMU connected")
try:
    Dev(conn).run()
except OSError as e:
    log("BTEMU connection ended", e)
log("BTEMU end")
