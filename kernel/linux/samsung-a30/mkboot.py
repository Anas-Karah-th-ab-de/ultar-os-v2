#!/usr/bin/env python3
"""Boot-Abbild fürs Galaxy A30 aus einem selbst gebauten Kernel (Startabbild ist im Kernel eingebaut).
Aufruf: mkboot.py <Image> <boot.img>   – Kopf wie im Samsung-Original (Version 1), Endmarke SEANDROIDENFORCE."""
import struct, hashlib, sys
k = open(sys.argv[1], 'rb').read()
page = 2048
h = bytearray(page)
h[0:8] = b'ANDROID!'
struct.pack_into('<10I', h, 8, len(k), 0x10008000, 0, 0x11000000, 0, 0x10f00000, 0x10000100, page, 1, 0x16000171)
name = b'SRPRL14A006KU'; h[48:48 + len(name)] = name
cmd = b'androidboot.selinux=permissive androidboot.selinux=permissive'; h[64:64 + len(cmd)] = cmd
s = hashlib.sha1()
for part in (k, b'', b'', b''):
    s.update(part); s.update(struct.pack('<I', len(part)))
h[576:596] = s.digest()
struct.pack_into('<IQI', h, 1632, 0, 0, 1648)
img = bytes(h) + k + bytes((-len(k)) % page) + b'SEANDROIDENFORCE'
if len(img) > 37748736: sys.exit("Abbild groesser als die Boot-Partition (36 MiB)")
open(sys.argv[2], 'wb').write(img)
print(len(img), "Bytes,", 37748736 - len(img), "frei")
