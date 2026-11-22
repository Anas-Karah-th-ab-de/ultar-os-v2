#!/usr/bin/env python3
"""Erzeugt für die Tests eine nachgebaute Samsung-Firmware (AP_….tar.md5) mit boot.img.lz4 und vbmeta.img.lz4.
Kein echtes Samsung-Material: Kernel und Ramdisk sind Zufallsdaten mit den Merkmalen, auf die es ankommt."""
import os, struct, subprocess, sys, tarfile
out = sys.argv[1]; os.makedirs(out, exist_ok=True)
page = 2048
kernel = os.urandom(31_000_000) + b"\0skip_initramfs\0" + os.urandom(5000) + b"console=ram skip_initramfs rootwait" + os.urandom(100_000)
ramdisk = os.urandom(40_000)
dtbo = os.urandom(3000)
pad = lambda b: b + b"\0" * ((page - len(b) % page) % page)
h = bytearray(page)
h[0:8] = b"ANDROID!"
struct.pack_into("<10I", h, 8, len(kernel), 0x10008000, len(ramdisk), 0x11000000, 0, 0x10f00000, 0x10000100, page, 1, 0x16000144)
h[48:56] = b"SRPSA30A"; h[64:64+20] = b"androidboot.test=1\0\0"
struct.pack_into("<IQI", h, 1632, len(dtbo), page + len(pad(kernel)) + len(pad(ramdisk)), 1648)
img = bytes(h) + pad(kernel) + pad(ramdisk) + pad(dtbo) + b"SEANDROIDENFORCE"
img += b"\0" * (34 * 1024 * 1024 - len(img))          # wie eine Partition von 34 MiB aufgefüllt (mehr als ein Übertragungsabschnitt)
open(f"{out}/boot.img", "wb").write(img)
vb = bytearray(4096); vb[0:4] = b"AVB0"; struct.pack_into(">II", vb, 4, 1, 0); vb[200:232] = os.urandom(32)
open(f"{out}/vbmeta.img", "wb").write(vb)
subprocess.check_call(["lz4", "-q", "-f", "-B4", "-BD", f"{out}/boot.img", f"{out}/boot.img.lz4"])   # abhängige Blöcke wie bei Samsung
subprocess.check_call(["lz4", "-q", "-f", f"{out}/vbmeta.img", f"{out}/vbmeta.img.lz4"])
ap = f"{out}/AP_A305FDDU6CUI3_CL22232766_QB45311024_REV00_user_low_ship_meta_OS11.tar.md5"
with tarfile.open(ap, "w", format=tarfile.USTAR_FORMAT) as t:
    for n in ("boot.img.lz4", "vbmeta.img.lz4"): t.add(f"{out}/{n}", arcname=n)
open(ap, "ab").write(b"0123456789abcdef0123456789abcdef  AP.tar\n")       # Samsung hängt eine md5-Zeile an
open(f"{out}/initramfs.cpio.gz", "wb").write(b"\x1f\x8b" + os.urandom(60_000))
print(ap)
