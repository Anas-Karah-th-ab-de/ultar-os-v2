#!/usr/bin/env python3
"""Samsung/AVB-Fuß an ein selbst gebautes Boot- oder Recovery-Abbild setzen.

Samsungs Original-Abbilder tragen am Ende der Partition einen AVB-Fuß ("AVBf", 64 Byte) und davor einen
vbmeta-Block. Schreibt man ein kürzeres eigenes Abbild, bleibt der alte Fuß in der Partition stehen und passt nicht
mehr zum Inhalt – der Bootloader startet dann sofort neu, ohne den Kernel auszuführen.
Dieses Skript macht es wie Magisk: eigenes Abbild, dahinter der vbmeta-Block des Originals, am Ende ein Fuß, der
die neue Größe nennt. Ergebnis ist genau so groß wie das Original (= Partition).

Aufruf: avbfoot.py <original.img> <eigenes.img> <ausgabe.img>
"""
import struct, sys
orig = open(sys.argv[1], 'rb').read()
new = open(sys.argv[2], 'rb').read()
if orig[-64:-60] != b'AVBf':
    sys.exit("Original hat keinen AVB-Fuß – nichts zu tun")
major, minor, osize, voff, vsize = struct.unpack_from('>IIQQQ', orig, len(orig) - 60)
blob = orig[voff:voff + vsize]
assert blob[:4] == b'AVB0', "vbmeta-Block im Original nicht gefunden"
new = new.rstrip(b'\0') if new.endswith(b'\0' * 64) else new
nvoff = (len(new) + 4095) // 4096 * 4096
if nvoff + vsize + 64 > len(orig):
    sys.exit("Abbild zu groß für die Partition")
out = bytearray(len(orig))
out[:len(new)] = new
out[nvoff:nvoff + vsize] = blob
out[-64:] = b'AVBf' + struct.pack('>IIQQQ', major, minor, len(new), nvoff, vsize) + bytes(28)
open(sys.argv[3], 'wb').write(out)
print(f"Original: Größe {osize}, vbmeta bei {voff} ({vsize} Byte) | neu: Größe {len(new)}, vbmeta bei {nvoff}, gesamt {len(out)}")
