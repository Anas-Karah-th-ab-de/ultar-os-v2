#!/usr/bin/env python3
"""Recovery-Abbild fürs Galaxy A30: Samsungs Original-Recovery, nur das Startabbild (ramdisk) wird getauscht.
Kernel, Ladeadressen und recovery_dtbo bleiben unverändert – der Bootloader startet die Recovery ohne
skip_initramfs, deshalb braucht es hier keinen Kernel-Eingriff. Danach avbfoot.py für den AVB-Fuß.
Aufruf: mkrecovery.py <original-recovery.img> <ramdisk.cpio.gz> <ausgabe.img>"""
import struct, hashlib, sys
o = open(sys.argv[1], 'rb').read(); rd = open(sys.argv[2], 'rb').read()
assert o[:8] == b'ANDROID!'
ks, ka, rs, ra, ss, sa, tags, page, ver, osv = struct.unpack_from('<10I', o, 8)
assert ver == 1, "nur Kopf-Version 1"
dsz, doff, hsz = struct.unpack_from('<IQI', o, 1632)
al = lambda n: (n + page - 1) // page * page
k = o[page:page + ks]
sec = o[page + al(ks) + al(rs):][:ss]
dt = o[doff:doff + dsz]
h = bytearray(o[:page])
struct.pack_into('<I', h, 16, len(rd))
ndoff = page + al(ks) + al(len(rd)) + al(ss)
struct.pack_into('<IQI', h, 1632, dsz, ndoff, hsz)
s = hashlib.sha1()
for part in (k, rd, sec, dt):
    s.update(part); s.update(struct.pack('<I', len(part)))
h[576:608] = s.digest() + bytes(12)
pad = lambda b: b + bytes((-len(b)) % page)
img = bytes(h) + pad(k) + pad(rd) + (pad(sec) if ss else b'') + pad(dt) + b'SEANDROIDENFORCE'
open(sys.argv[3], 'wb').write(img)
print(f"Kernel {ks}, ramdisk {rs} -> {len(rd)} @ {ra:#x}, dtbo {dsz} @ {doff} -> {ndoff}, gesamt {len(img)}")
