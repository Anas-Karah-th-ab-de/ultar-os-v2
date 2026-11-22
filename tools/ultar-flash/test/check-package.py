#!/usr/bin/env python3
"""Prüft ein von Ultar Flash gebautes Paket gegen die Ausgangsdaten: check-package.py <firmware-ordner> <paketordner>"""
import struct, sys
fw, pkg = sys.argv[1], sys.argv[2]
o, n = open(f"{fw}/boot.img","rb").read(), open(f"{pkg}/boot.img","rb").read()
rd = open(f"{fw}/initramfs.cpio.gz","rb").read()
page = struct.unpack_from("<I", n, 36)[0]; pad = lambda x: (x + page - 1) // page * page
ks, _, rs = struct.unpack_from("<3I", n, 8); oks, _, ors = struct.unpack_from("<3I", o, 8)
k = n[page:page+ks]; ok_ = o[page:page+oks]
r = n[page+pad(ks):page+pad(ks)+rs]
ds, doff = struct.unpack_from("<IQ", n, 1632); ods, odoff = struct.unpack_from("<IQ", o, 1632)
checks = {
 "Kennung ANDROID!": n[:8] == b"ANDROID!",
 "Kernel gleich lang": ks == oks,
 "skip_initramfs entfernt": b"skip_initramfs" not in k,
 "want_initramfs 2x": k.count(b"want_initramfs") == 2,
 "Kernel sonst unverändert": k.replace(b"want_initramfs", b"skip_initramfs") == ok_,
 "Ramdisk = Ultar-Kernsystem": r == rd,
 "recovery_dtbo erhalten": n[doff:doff+ds] == o[odoff:odoff+ods] and ds == ods and doff == page+pad(ks)+pad(rs),
 "Kommandozeile erhalten": n[64:576] == o[64:576],
 "Endmarke SEANDROIDENFORCE": n.rstrip(b"\0").endswith(b"SEANDROIDENFORCE"),
 "passt in die Partition": len(n) <= len(o),
}
v, ov = open(f"{pkg}/vbmeta.img","rb").read(), open(f"{fw}/vbmeta.img","rb").read()
checks["vbmeta leer, Flags = 2"] = struct.unpack_from(">I", v, 120)[0] == 2 and len(v) == 256 and v[:4] == b"AVB0"
checks["vbmeta ohne Signatur"] = v[12:120] == bytes(108)
import tarfile, glob
t = tarfile.open(glob.glob(f"{pkg}/ultar-*-odin-AP.tar")[0])
checks["Odin-tar enthält boot.img und vbmeta.img"] = sorted(t.getnames()) == ["boot.img", "vbmeta.img"] and t.extractfile("boot.img").read() == n
bad = [k for k, ok in checks.items() if not ok]
for k, ok in checks.items(): print(("ok     " if ok else "FAIL   ") + k)
sys.exit(1 if bad else 0)
