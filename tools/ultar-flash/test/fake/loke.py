#!/usr/bin/env python3
"""TEST-ATTRAPPE: Samsung-Handy im Download-Modus (Loke) über TCP statt COM-Port.
Hält sich streng an das Protokoll, wie es Ultar Flash spricht, und schreibt in $FAKE_DEV/dev/block/by-name.
  loke.py <port> [fehler]     fehler = "nak-end": meldet beim Abschluss des ersten Abschnitts einen Schreibfehler"""
import os, socket, struct, sys
D = os.environ["FAKE_DEV"]; BN = f"{D}/dev/block/by-name"
PORT = int(sys.argv[1]); FAIL = sys.argv[2] if len(sys.argv) > 2 else ""
PARTS = [("BOOTLOADER", 80), ("PIT", 70), ("EFS", 1), ("BOOT", 5), ("RECOVERY", 6), ("VBMETA", 12), ("SYSTEM", 20), ("USERDATA", 24)]

def pit():
    b = struct.pack("<II", 0x12349876, len(PARTS)) + b"\0" * 20
    for name, pid in PARTS:
        b += struct.pack("<9I", 0, 2, pid, 5, 5, 0, 0, 0, 0) + name.encode().ljust(32, b"\0") + (name.lower() + ".img").encode().ljust(32, b"\0") + b"\0" * 32
    return b

def rd(c, n):
    b = b""
    while len(b) < n:
        x = c.recv(n - len(b))
        if not x: raise EOFError
        b += x
    return b

s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1", PORT)); s.listen(1)
while True:
    c, _ = s.accept()
    try:
        if open(f"{D}/state").read().strip() != "download": c.close(); continue
        assert rd(c, 4) == b"ODIN"; c.sendall(b"LOKE")
        psize, P = 131072, pit(); data = b""; begun = False
        while True:
            p = rd(c, 1024); t, r = struct.unpack_from("<II", p); a = struct.unpack_from("<6I", p, 8)
            if t == 0x64 and r == 0: assert a[0] == 4; c.sendall(struct.pack("<II", 0x64, 131072))
            elif t == 0x64 and r == 5: psize = a[0]; assert psize == 1048576; c.sendall(struct.pack("<II", 0x64, 0))
            elif t == 0x64 and r == 2: total = a[0] | a[1] << 32; c.sendall(struct.pack("<II", 0x64, 0))
            elif t == 0x65 and r == 1: c.sendall(struct.pack("<II", 0x65, len(P)))
            elif t == 0x65 and r == 2: c.sendall(P[a[0] * 500:a[0] * 500 + 500])
            elif t == 0x65 and r == 3: c.sendall(struct.pack("<II", 0x65, 0))
            elif t == 0x66 and r == 0: data = b""; begun = True; c.sendall(struct.pack("<II", 0x66, 0))
            elif t == 0x66 and r == 2:
                assert begun and a[0] % psize == 0 and a[0] // psize <= 30, "Abschnitt zu groß oder nicht auf Paketgröße"
                c.sendall(struct.pack("<II", 0x66, 0))
                seq = b""
                for i in range(a[0] // psize):
                    seq += rd(c, psize); c.sendall(struct.pack("<II", 0, i))
                p = rd(c, 1024); t, r = struct.unpack_from("<II", p); dest, nbytes, _, devtype, pid, last = struct.unpack_from("<6I", p, 8)
                assert (t, r) == (0x66, 3) and dest == 0 and devtype == 2 and nbytes <= len(seq) and seq[nbytes:].strip(b"\0") == b""
                if FAIL == "nak-end": c.sendall(struct.pack("<II", 0x66, 1)); continue
                data += seq[:nbytes]
                if last:
                    name = [n for n, i in PARTS if i == pid][0].lower()
                    assert name not in ("bootloader", "pit", "efs"), "geschützte Partition!"
                    open(f"{BN}/{name}", "wb").write(data); begun = False
                c.sendall(struct.pack("<II", 0x66, 0))
            elif t == 0x67 and r == 0: c.sendall(struct.pack("<II", 0x67, 0))
            elif t == 0x67 and r == 1: c.sendall(struct.pack("<II", 0x67, 0)); open(f"{D}/state", "w").write("adb"); break
            else: raise AssertionError(f"unbekanntes Paket {t:#x}/{r}")
    except (EOFError, AssertionError) as e:
        print("loke-attrappe:", repr(e), flush=True)
    finally:
        c.close()
